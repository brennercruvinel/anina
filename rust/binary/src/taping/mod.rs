//! `anina init`: write a tape by using the program, like a codegen. You
//! drive the program in your own terminal; anina forwards every key, mirrors
//! the screen, and writes the tape: typing as `Type`, keys as `Key`, and a
//! `Wait` for the text each step brought up, so the tape replays at the
//! program's pace, not yours.

pub mod input;
pub mod print;
pub mod relay;

use anina_runner::engine::Spec;
use anina_runner::redact::scope::Scope;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Where to write the tape (default: <program>.tape here)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Terminal columns (default: this terminal's)
    #[arg(long)]
    pub cols: Option<u16>,
    /// Terminal rows (default: this terminal's)
    #[arg(long)]
    pub rows: Option<u16>,
    /// Environment for the program, K=V (repeatable)
    #[arg(long = "env", value_name = "K=V")]
    pub env: Vec<String>,
    /// Working directory of the program
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Start the program from an empty environment plus --env
    #[arg(long)]
    pub clean_env: bool,
    /// The key that ends the recording (not sent to the program)
    #[arg(long, default_value = "ctrl+]")]
    pub stop_key: String,
    /// The program and its arguments
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

pub fn run(a: Args, scope: &Scope) -> Result<()> {
    let stop = relay::stop_byte(&a.stop_key)?;
    let (tc, tr) = crossterm::terminal::size().unwrap_or((100, 30));
    let mut spec = Spec::new(a.command[0].clone(), a.command[1..].to_vec());
    spec.cols = a.cols.unwrap_or(tc);
    spec.rows = a.rows.unwrap_or(tr);
    spec.clean_env = a.clean_env;
    spec.cwd = match &a.cwd {
        Some(d) => Some(std::fs::canonicalize(d).map_err(|e| anyhow!("--cwd {}: {e}", d.display()))?),
        None => None,
    };
    for kv in &a.env {
        let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("--env takes K=V, got {kv:?}"))?;
        spec.set_env(k, v);
    }
    let out = a.out.clone().unwrap_or_else(|| default_out(&a.command[0]));
    eprintln!("anina init: recording {}; {} stops", a.command.join(" "), a.stop_key);
    let follow_size = a.cols.is_none() && a.rows.is_none();
    // the HOME the program runs with: its own when `--env HOME=` gives one
    let home =
        spec.env.iter().rev().find(|(k, _)| k == "HOME").map(|(_, v)| v.clone()).or_else(|| std::env::var("HOME").ok());
    let kept = home.map(|h| settings_of(&a.command[0], std::path::Path::new(&h))).unwrap_or_default();
    let before = stamps(&kept);
    let mut session = relay::run(relay::Options { spec, stop, follow_size })?;
    let saved = changed(&before, &stamps(&kept));
    // the waits come from these screens; `anina record` redacts what it
    // reads, so a wait on the raw text would never match on replay
    let red = scope.redactor();
    for i in &mut session.inputs {
        red.apply(&mut i.before);
    }
    red.apply(&mut session.last);
    let mut tape = print::write(&session);
    if !saved.is_empty() {
        // said in the tape too: whoever replays it later was not here
        let files: Vec<String> = saved.iter().map(|p| anina_screen::lesson::shown(p)).collect();
        tape = format!(
            "# {} wrote {} during this session: a replay starts from what it saved.\n{tape}",
            a.command[0],
            files.join(", ")
        );
        eprintln!(
            "anina init: warning: {} saved its settings while you used it ({}). `anina record` starts it from what \
             it saved, not from what you started with, and a wait on the first state may not hold. For a tape that \
             replays, give both an empty HOME of their own: --env HOME=<an empty folder>",
            a.command[0],
            files.join(", ")
        );
    }
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, &tape)?;
    let steps = tape.lines().filter(|l| !l.starts_with('#') && !l.starts_with("Set ")).count();
    eprintln!(
        "anina init: {} inputs -> {} steps in {}",
        session.inputs.len(),
        steps,
        anina_screen::lesson::shown(&out)
    );
    eprintln!("anina init: replay with: anina record {}", anina_screen::lesson::shown(&out));
    Ok(())
}

/// Where a program keeps what it remembers between runs, by the names
/// programs use: the folders and files called after it under `home`.
fn settings_of(program: &str, home: &std::path::Path) -> Vec<PathBuf> {
    let name = std::path::Path::new(program).file_name().map_or(String::new(), |s| s.to_string_lossy().into_owned());
    if name.is_empty() {
        return Vec::new();
    }
    vec![
        home.join(".config").join(&name),
        home.join(".local/share").join(&name),
        home.join(".local/state").join(&name),
        home.join(format!(".{name}rc")),
        home.join(format!(".{name}")),
    ]
}

/// Every file at or under `roots`, with when it was last written and its
/// size. A folder is read one level deep and a hundred files wide: settings,
/// not a cache.
fn stamps(roots: &[PathBuf]) -> std::collections::BTreeMap<PathBuf, (std::time::SystemTime, u64)> {
    let mut out = std::collections::BTreeMap::new();
    let mut note = |p: PathBuf| {
        if let Ok(m) = std::fs::metadata(&p)
            && m.is_file()
            && let Ok(t) = m.modified()
        {
            out.insert(p, (t, m.len()));
        }
    };
    for root in roots {
        match std::fs::read_dir(root) {
            Ok(dir) => dir.flatten().take(100).for_each(|e| note(e.path())),
            Err(_) => note(root.clone()),
        }
    }
    out
}

/// The files written between two `stamps`: new, or with another time or size.
fn changed<K: Ord + Clone, V: PartialEq>(
    before: &std::collections::BTreeMap<K, V>,
    after: &std::collections::BTreeMap<K, V>,
) -> Vec<K> {
    after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.clone()).collect()
}

/// `<program>.tape`, or `<program>-2.tape`... when it exists.
fn default_out(program: &str) -> PathBuf {
    let stem = std::path::Path::new(program).file_name().map_or("anina".into(), |s| s.to_string_lossy().into_owned());
    let mut p = PathBuf::from(format!("{stem}.tape"));
    let mut n = 2;
    while p.exists() {
        p = PathBuf::from(format!("{stem}-{n}.tape"));
        n += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_a_program_wrote_under_its_own_name_is_noticed() {
        let home = std::env::temp_dir().join(format!("anina-init-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".config/htop")).unwrap();
        std::fs::create_dir_all(home.join(".config/other")).unwrap();
        std::fs::write(home.join(".config/htop/htoprc"), "sort_key=46\n").unwrap();
        let kept = settings_of("/opt/homebrew/bin/htop", &home);
        let before = stamps(&kept);
        assert_eq!(changed(&before, &stamps(&kept)), Vec::<PathBuf>::new());
        // htop saves the order chosen during the session as it leaves; another program's file is not its own
        std::fs::write(home.join(".config/htop/htoprc"), "sort_key=47 and more\n").unwrap();
        std::fs::write(home.join(".htoprc"), "new").unwrap();
        std::fs::write(home.join(".config/other/rc"), "x").unwrap();
        assert_eq!(changed(&before, &stamps(&kept)), [home.join(".config/htop/htoprc"), home.join(".htoprc")]);
        let _ = std::fs::remove_dir_all(&home);
    }
}
