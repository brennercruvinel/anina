//! Isolation for programs under test, installers above all: a fresh HOME and
//! XDG dirs, a clean environment, an OS sandbox when available, and a report
//! of what changed on disk.
//!
//! - `env` (default): the program gets an empty environment except PATH,
//!   LANG and TERM, with HOME, XDG_*_HOME and TMPDIR under the root. It can
//!   still write anywhere it has permission to; it just has no reason to.
//! - `strict`: the same, inside the OS sandbox (macOS `sandbox-exec`, Linux
//!   `bwrap`), where a write outside the root, /dev and the temp dirs fails.
//! - `off`: run as is, with the caller's environment.

pub mod diffs;
pub mod linux;
pub mod macos;

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
}

impl Sandbox {
    /// Create (or reuse) the sandbox directories under `root`.
    pub fn new(root: &Path, mode: Mode) -> Result<Sandbox> {
        std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        // the real path: macOS's /tmp is /private/tmp, and sandbox rules match real paths
        let root = std::fs::canonicalize(root)?;
        let sb = Sandbox { mode, home: root.join("home"), tmp: root.join("tmp"), work: root.join("work"), root };
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

    /// The argv prefix that puts a program inside the OS sandbox (empty
    /// unless `strict`).
    pub fn wrap(&self) -> Vec<String> {
        match self.mode {
            Mode::Strict if cfg!(target_os = "macos") => macos::wrap(&self.root),
            Mode::Strict => linux::wrap(&self.root, &self.home, &self.tmp),
            _ => Vec::new(),
        }
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
