//! Isolation for programs under test, installers above all: a fresh HOME and
//! XDG dirs, a clean environment, an OS sandbox when available, and a report
//! of what changed on disk.
//!
//! - `env` (default): the program gets an empty environment except PATH,
//!   LANG and TERM, with HOME, XDG_*_HOME and TMPDIR under the root. It can
//!   still write anywhere it has permission to; it just has no reason to.
//! - `strict`: the same, inside the OS sandbox (macOS `sandbox-exec`, Linux
//!   `bwrap`), where a write outside the root, /dev and the temp dirs fails
//!   and the network is off unless the preset says `network = true`.
//! - `off`: run as is, with the caller's environment.
//!
//! Everything a run starts goes through one prefix (`Wrap`): the program a
//! tape spawns and the shell lines around it (`Exec`, a preset's `prepare`,
//! explore's `--before`). The fresh environment is the program's alone: a
//! shell line keeps the caller's (see `shell`).

pub mod diffs;
pub mod linux;
pub mod macos;
pub mod shell;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Off,
    #[default]
    Env,
    Strict,
}

impl std::str::FromStr for Mode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "off" | "none" => Ok(Mode::Off),
            "env" | "" => Ok(Mode::Env),
            "strict" => Ok(Mode::Strict),
            other => bail!("unknown sandbox mode {other:?} (off, env, strict)"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sandbox {
    pub mode: Mode,
    /// Everything the program may write lives here.
    pub root: PathBuf,
    pub home: PathBuf,
    pub tmp: PathBuf,
    /// A working directory for the program, empty at first.
    pub work: PathBuf,
    /// Under `strict`: whether the network is reachable (a preset's
    /// `network = true`). Off where nothing declares it.
    pub network: bool,
}

/// How the processes of a run start: the argv prefix that puts each one
/// inside the OS sandbox, and what that sandbox allows, to say so when it
/// refuses. The default confines nothing.
#[derive(Clone, Debug, Default)]
pub struct Wrap {
    /// `sandbox-exec -p PROFILE` or `bwrap ... --`; empty when nothing confines.
    pub prefix: Vec<String>,
    /// The folders a write is allowed under, the root first.
    pub writable: Vec<PathBuf>,
    pub network: bool,
}

impl Wrap {
    /// `argv` behind the prefix: the one way into the sandbox, for the
    /// program and for every shell line.
    pub fn argv<S: AsRef<str>>(&self, argv: &[S]) -> Vec<String> {
        self.prefix.iter().cloned().chain(argv.iter().map(|a| a.as_ref().to_string())).collect()
    }
}

impl Sandbox {
    /// Create (or reuse) the sandbox directories under `root`.
    pub fn new(root: &Path, mode: Mode) -> Result<Sandbox> {
        // a link here would make its target the writable root
        if std::fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("{} is a symbolic link; a sandbox root is a folder of its own", root.display());
        }
        std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        // the real path: macOS's /tmp is /private/tmp, and sandbox rules match real paths
        let root = std::fs::canonicalize(root)?;
        let (home, tmp, work) = (root.join("home"), root.join("tmp"), root.join("work"));
        let sb = Sandbox { mode, home, tmp, work, root, network: false };
        for d in [&sb.home, &sb.tmp, &sb.work] {
            std::fs::create_dir_all(d)?;
        }
        for d in sb.xdg().iter().map(|(_, p)| p) {
            std::fs::create_dir_all(d)?;
        }
        if mode == Mode::Strict {
            sb.check_tool()?;
        }
        Ok(sb)
    }

    fn xdg(&self) -> [(&'static str, PathBuf); 4] {
        [
            ("XDG_CONFIG_HOME", self.home.join(".config")),
            ("XDG_CACHE_HOME", self.home.join(".cache")),
            ("XDG_DATA_HOME", self.home.join(".local/share")),
            ("XDG_STATE_HOME", self.home.join(".local/state")),
        ]
    }

    /// Whether the program runs with a clean environment.
    pub fn clean_env(&self) -> bool {
        self.mode != Mode::Off
    }

    /// The environment of a sandboxed program (empty with `off`).
    pub fn env(&self) -> Vec<(String, String)> {
        if self.mode == Mode::Off {
            return Vec::new();
        }
        let mut path = vec![
            self.home.join(".local/bin").display().to_string(),
            self.home.join(".cargo/bin").display().to_string(),
        ];
        path.extend(
            std::env::var("PATH")
                .unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into())
                .split(':')
                .map(String::from),
        );
        let mut env = vec![
            ("HOME".to_string(), self.home.display().to_string()),
            ("TMPDIR".into(), self.tmp.display().to_string()),
            ("PATH".into(), path.join(":")),
            (
                "LANG".into(),
                std::env::var("LANG").ok().filter(|l| l.contains("UTF-8")).unwrap_or_else(|| "en_US.UTF-8".into()),
            ),
            ("USER".into(), "user".into()),
            ("LOGNAME".into(), "user".into()),
            ("SHELL".into(), "/bin/sh".into()),
        ];
        env.extend(self.xdg().iter().map(|(k, p)| (k.to_string(), p.display().to_string())));
        env
    }

    /// What puts a process of this run inside the OS sandbox (nothing unless
    /// `strict`).
    pub fn wrap(&self) -> Wrap {
        self.wrap_with(&[])
    }

    /// `wrap`, with `more` folders writable besides the root (real paths): a
    /// preset's prepare clones and builds in anina's cache.
    pub fn wrap_with(&self, more: &[PathBuf]) -> Wrap {
        if self.mode != Mode::Strict {
            return Wrap::default();
        }
        let prefix = if cfg!(target_os = "macos") {
            macos::wrap(&self.root, more, self.network)
        } else {
            linux::wrap(&self.root, more, self.network)
        };
        let writable = std::iter::once(&self.root).chain(more).cloned().collect();
        Wrap { prefix, writable, network: self.network }
    }

    fn check_tool(&self) -> Result<()> {
        let (tool, hint) = if cfg!(target_os = "macos") {
            ("sandbox-exec", "it ships with macOS; is /usr/bin on PATH?")
        } else {
            ("bwrap", "install bubblewrap (apt install bubblewrap / dnf install bubblewrap)")
        };
        if crate::which(tool).is_none() && !Path::new("/usr/bin").join(tool).exists() {
            bail!("--sandbox strict needs {tool}: {hint}");
        }
        Ok(())
    }

    /// The rules that show sandbox paths as a person would read them.
    pub fn redactions(&self) -> Vec<(String, String)> {
        vec![(self.home.display().to_string(), "~".into()), (self.root.display().to_string(), "/sandbox".into())]
    }
}
