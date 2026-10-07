//! `anina init`: write a tape by using the program, like a codegen. You
//! drive the program in your own terminal; anina forwards every key, mirrors
//! the screen, and writes the tape: typing as `Type`, keys as `Key`, and a
//! `Wait` for the text each step brought up, so the tape replays at the
//! program's pace, not yours.

pub mod input;
pub mod print;
pub mod relay;

use anina_runner::engine::Spec;
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

pub fn run(a: Args) -> Result<()> {
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
        spec.env.push((k.into(), v.into()));
    }
    let out = a.out.clone().unwrap_or_else(|| default_out(&a.command[0]));
    eprintln!("anina init: recording {}; {} stops", a.command.join(" "), a.stop_key);
    let follow_size = a.cols.is_none() && a.rows.is_none();
    let mut session = relay::run(relay::Options { spec, stop, follow_size })?;
    // the waits come from these screens; `anina record` redacts what it
    // reads, so a wait on the raw text would never match on replay
    let red = anina_runner::redact::Redactor::defaults();
    for i in &mut session.inputs {
        red.apply(&mut i.before);
    }
    red.apply(&mut session.last);
    let tape = print::write(&session);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, &tape)?;
    let steps = tape.lines().filter(|l| !l.starts_with('#') && !l.starts_with("Set ")).count();
    eprintln!("anina init: {} inputs -> {} steps in {}", session.inputs.len(), steps, out.display());
    eprintln!("anina init: replay with: anina record {}", out.display());
    Ok(())
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
