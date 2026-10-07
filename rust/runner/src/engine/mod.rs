//! Engines run a program in a pseudo terminal and read its screen. Everything
//! above this module talks to `dyn Engine`, so a new backend (another
//! emulator, a remote terminal) plugs in here and nowhere else.

use anina_screen::{Mask, Pattern, Screen};
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub mod inner;
pub mod outer;

/// What to run and in which terminal.
#[derive(Clone, Debug)]
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Variables set for the program (over the inherited ones, or alone
    /// with `clean_env`).
    pub env: Vec<(String, String)>,
    /// Start from an empty environment plus `env` (TERM and COLORTERM are
    /// added unless `env` sets them).
    pub clean_env: bool,
    pub cols: u16,
    pub rows: u16,
}

impl Spec {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Spec { program: program.into(), args, cwd: None, env: Vec::new(), clean_env: false, cols: 100, rows: 30 }
    }

    /// Program and arguments, wrapped in `env -i K=V...` for a clean
    /// environment, which works the same with every backend.
    pub fn argv(&self) -> (String, Vec<String>) {
        if !self.clean_env {
            return (self.program.clone(), self.args.clone());
        }
        let mut args = vec!["-i".to_string()];
        let has = |k: &str| self.env.iter().any(|(n, _)| n == k);
        if !has("TERM") {
            args.push("TERM=xterm-256color".into());
        }
        if !has("COLORTERM") {
            args.push("COLORTERM=truecolor".into());
        }
        args.extend(self.env.iter().map(|(k, v)| format!("{k}={v}")));
        args.push(self.program.clone());
        args.extend(self.args.iter().cloned());
        ("/usr/bin/env".into(), args)
    }
}

/// Terminal modes by name (`alt_screen`, `cursor_visible`, `mouse_*`,
/// `bracketed_paste`, ...), as the backend reports them.
pub type Modes = BTreeMap<String, bool>;

pub trait Engine {
    fn name(&self) -> &'static str;
    fn spawn(&mut self, spec: &Spec) -> Result<()>;
    /// Send text as typed.
    fn write(&mut self, data: &str) -> Result<()>;
    /// Press one key: `enter`, `tab`, `esc`, `up`, `ctrl+c`, `alt+x`, `f5`, `a`.
    fn key(&mut self, key: &str) -> Result<()>;
    fn screen(&mut self) -> Result<Screen>;
    fn modes(&mut self) -> Result<Modes>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    /// `Some(code)` once the program exited.
    fn exit_code(&mut self) -> Result<Option<i32>>;
    fn close(&mut self) -> Result<()>;
}

/// Build an engine by name: `tui-test` (default, in process) or `tmux`.
pub fn by_name(name: &str) -> Result<Box<dyn Engine>> {
    match name {
        "" | "tui-test" | "tuitest" => Ok(Box::new(inner::TuiTest::default())),
        "tmux" => Ok(Box::new(outer::Tmux::default())),
        other => bail!("unknown engine {other:?} (tui-test, tmux)"),
    }
}

/// Normalize a key name from any common spelling (tape, tmux, vhs) to the
/// `mod+key` form: `C-q`/`Ctrl+Q`/`^Q` -> `ctrl+q`, `BTab` -> `shift+tab`,
/// `Escape` -> `esc`, `BSpace` -> `backspace`, `PgDn` -> `pagedown`.
pub fn normalize_key(k: &str) -> String {
    let k = k.trim();
    if let Some(rest) = k.strip_prefix('^').filter(|r| r.chars().count() == 1) {
        return format!("ctrl+{}", rest.to_lowercase());
    }
    // tmux style C-x, M-x, S-x
    let mut mods = Vec::new();
    let mut rest = k;
    loop {
        let lower = rest.to_ascii_lowercase();
        if lower.len() > 2 && (lower.starts_with("c-") || lower.starts_with("m-") || lower.starts_with("s-")) {
            mods.push(match &lower[..1] {
                "c" => "ctrl",
                "m" => "alt",
                _ => "shift",
            });
            rest = &rest[2..];
        } else {
            break;
        }
    }
    let mut parts: Vec<String> = rest.split('+').map(|s| s.to_string()).collect();
    let key = parts.pop().unwrap_or_default();
    for p in parts {
        mods.push(match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "c" => "ctrl",
            "alt" | "meta" | "option" | "m" => "alt",
            "shift" | "s" => "shift",
            "super" | "cmd" | "command" => "super",
            _ => "ctrl",
        });
    }
    let lower = key.to_ascii_lowercase();
    let name = match lower.as_str() {
        "escape" | "esc" => "esc".to_string(),
        "bspace" | "backspace" | "bs" => "backspace".into(),
        "btab" => {
            mods.push("shift");
            "tab".into()
        }
        "enter" | "return" | "cr" => "enter".into(),
        "pgup" | "ppage" | "pageup" => "pageup".into(),
        "pgdn" | "npage" | "pagedown" => "pagedown".into(),
        "del" | "dc" | "delete" => "delete".into(),
        "ic" | "ins" | "insert" => "insert".into(),
        " " | "space" => "space".into(),
        _ if key.chars().count() == 1 && mods.is_empty() => key.clone(), // keep case: `G`, `?`
        _ => lower,
    };
    mods.dedup();
    if mods.is_empty() { name } else { format!("{}+{}", mods.join("+"), name) }
}

/// Poll until `pat` is (or, with `gone`, is no longer) on screen. Returns the
/// last screen read either way, and whether the condition was met.
pub fn wait_for(engine: &mut dyn Engine, pat: &Pattern, gone: bool, timeout: Duration) -> Result<(bool, Screen)> {
    let end = Instant::now() + timeout;
    loop {
        let s = engine.screen()?;
        if pat.matches(&s) != gone {
            return Ok((true, s));
        }
        if Instant::now() >= end {
            return Ok((false, s));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Wait until the screen stops changing: `quiet` without a change (masked
/// cells ignored), up to `timeout`. Returns the settled screen and whether it
/// settled.
pub fn wait_idle(
    engine: &mut dyn Engine,
    mask: Option<&Mask>,
    quiet: Duration,
    timeout: Duration,
) -> Result<(bool, Screen)> {
    let end = Instant::now() + timeout;
    let mut last = engine.screen()?;
    let mut last_hash = last.hash(mask);
    let mut since = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(20));
        let s = engine.screen()?;
        let h = s.hash(mask);
        if h != last_hash {
            last_hash = h;
            since = Instant::now();
        }
        last = s;
        if since.elapsed() >= quiet {
            return Ok((true, last));
        }
        if Instant::now() >= end {
            return Ok((false, last));
        }
    }
}
