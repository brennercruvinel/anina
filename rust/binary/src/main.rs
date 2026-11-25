mod audits;
mod checks;
mod ending;
mod mapper;
mod taping;
mod versus;

use anina_record::{self as record, preset};
use anina_render::{self as render, brand};
use anina_runner::engine;
use anina_runner::redact::Redactor;
use anina_runner::redact::scope::Scope;
use anina_screen::notice::Notice;
use anina_screen::{self as screen, frames};
use anyhow::Result;
use clap::{Parser, Subcommand};
use ending::Ending;
use std::path::PathBuf;

/// Drive, map, audit and animate any terminal program. No AI needed.
#[derive(Parser, Debug)]
#[command(name = "anina", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run a preset journey (or some of its parts): screenshots, map, demo
    Run(RunArgs),
    /// List the presets anina knows
    Presets,
    /// Play a tape and record it (cast + animated SVG)
    Record(RecordArgs),
    /// Render a recorded cast to an animated SVG
    Render(RenderArgs),
    /// Map a program by itself: every screen and key, as a graph
    Explore(mapper::Args),
    /// Write a tape by using the program
    Init(taping::Args),
    /// Check a program across terminals: sizes, colors, contrast, latency, hygiene
    Audit(audits::Args),
    /// Derive a theme from a program's own colors
    Brand(BrandArgs),
    /// Export a cast to asciicast v3 (asciinema player, agg, asciinema.org)
    Export {
        cast: PathBuf,
        /// Output .cast file (default: next to the input)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Look through anina's output for anything redaction should have hidden
    Check(checks::Args),
    /// Compare two runs, timing aside: files, screens as text, pixels, reports
    Compare(versus::Args),
    /// Check that anina can run here
    Doctor,
}

#[derive(clap::Args, Debug)]
struct RunArgs {
    /// Preset name or directory
    preset: String,
    /// Run only these parts (repeatable; default: all, in order)
    #[arg(long)]
    part: Vec<String>,
    /// Output directory (default: anina-out/<preset>)
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Terminal engine: tui-test (default) or tmux
    #[arg(long, default_value = "tui-test")]
    engine: String,
    /// Sandbox mode, over the preset's: off, env, strict
    #[arg(long)]
    sandbox: Option<String>,
    /// Set a preset variable, K=V (the environment works too)
    #[arg(long = "var", value_name = "K=V")]
    vars: Vec<String>,
    /// Extra redaction, REGEX=REPLACEMENT
    #[arg(long = "redact", value_name = "RE=WITH")]
    redact: Vec<String>,
    /// Keep the sandbox of the last run instead of starting empty
    #[arg(long)]
    keep: bool,
    /// Put the images inside index.html: one file to share
    #[arg(long)]
    embed: bool,
}

#[derive(clap::Args, Debug)]
struct RecordArgs {
    /// The tape to play (anina or vhs dialect)
    tape: PathBuf,
    /// Output directory (default: anina-out/<tape name>)
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Terminal engine: tui-test (default) or tmux
    #[arg(long, default_value = "tui-test")]
    engine: String,
    /// Theme for the SVG (default: the tape's, else derived from the app)
    #[arg(long)]
    theme: Option<String>,
    /// Only record the cast, render nothing
    #[arg(long)]
    no_render: bool,
    /// Variables for the tape, K=V (also read from the environment)
    #[arg(long = "var", value_name = "K=V")]
    vars: Vec<String>,
}

#[derive(clap::Args, Debug)]
struct RenderArgs {
    /// The cast to render (.cast.json or .cast.json.gz)
    cast: PathBuf,
    /// Output file (default: next to the cast, .svg)
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Theme: a built-in name, a path to a theme JSON (anina or vhs), or auto
    #[arg(long)]
    theme: Option<String>,
    /// Longest a still screen stays, in seconds (default: the theme's)
    #[arg(long)]
    max_hold: Option<f64>,
    /// Play everything X times faster (above 0)
    #[arg(long, default_value_t = 1.0, value_parser = above_zero)]
    speed: f64,
    /// Playback frame rate cap (default: the theme's)
    #[arg(long)]
    max_fps: Option<f64>,
    /// Dark window chrome (the theme's own); with --light writes both
    #[arg(long)]
    dark: bool,
    /// Light window chrome around the unchanged terminal
    #[arg(long)]
    light: bool,
    /// Do not embed the font subset (smaller, but looks different per machine)
    #[arg(long)]
    no_font: bool,
    /// Also write the poster frame as a PNG
    #[arg(long)]
    poster: bool,
}

/// A speed: a number above zero. At zero an animation never ends.
fn above_zero(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(v) if v > 0.0 && v.is_finite() => Ok(v),
        _ => Err("a speed is a number above 0".into()),
    }
}

#[derive(clap::Args, Debug)]
struct BrandArgs {
    /// A cast to read colors from
    cast: PathBuf,
    /// Where to write the theme JSON (default: stdout)
    #[arg(short, long)]
    out: Option<PathBuf>,
}

/// Where the crates below report (`Notice`): stderr, a line at a time.
/// `anina: ...` for a command, `anina [NAME] ...` for the lines of a preset;
/// a warning is `anina: warning: ...` in both.
pub enum Stderr {
    Command,
    Preset(String),
}

impl Notice for Stderr {
    fn status(&self, line: &str) {
        match self {
            Stderr::Command => eprintln!("anina: {line}"),
            Stderr::Preset(name) => eprintln!("anina [{name}] {line}"),
        }
    }

    fn warning(&self, line: &str) {
        eprintln!("anina: warning: {line}");
    }
}

fn main() {
    let cli = Cli::parse();
    // before the command writes anything: the check at its end reads what was written since
    let started = std::time::SystemTime::now();
    // the one scope of this command: what it hides, what it hid, where it
    // wrote. A config that does not load is the command's error, and the
    // terminal still hides who and where this is.
    let (scope, ran) = match Scope::load() {
        Ok(scope) => {
            let ran = dispatch(cli.cmd, &scope);
            (scope, ran)
        }
        Err(e) => (Scope::of(Redactor::identity()), Err(e)),
    };
    // what the command wrote is checked whether it worked or not: a failed
    // wait leaves its screen in failed.txt and failed.ansi
    let checked = checks::verify_written(&scope, started);
    let failed: Vec<anyhow::Error> = [ran.err(), checked.err()].into_iter().flatten().collect();
    for e in &failed {
        eprintln!("anina: {}", scope.redactor().quiet(&format!("{e:#}")));
    }
    if !failed.is_empty() {
        std::process::exit(ending::code(&failed));
    }
}

fn dispatch(cmd: Cmd, scope: &Scope) -> Result<()> {
    match cmd {
        Cmd::Run(a) => run(a, scope),
        Cmd::Presets => presets(),
        Cmd::Record(a) => {
            let asked = record::Request {
                tape: a.tape,
                out: a.out,
                engine: a.engine,
                theme: a.theme,
                no_render: a.no_render,
                vars: a.vars,
            };
            match record::run(&asked, scope, &Stderr::Command)? {
                record::Played::Whole(_) => Ok(()),
                record::Played::Stopped(e) => Err(ending::read_as(Ending::Journey, e)),
            }
        }
        Cmd::Render(a) => {
            let asked = render::Request {
                cast: a.cast,
                out: a.out,
                theme: a.theme,
                max_hold: a.max_hold,
                speed: a.speed,
                max_fps: a.max_fps,
                dark: a.dark,
                light: a.light,
                no_font: a.no_font,
                poster: a.poster,
            };
            render::run(&asked, &Stderr::Command)
        }
        Cmd::Explore(a) => mapper::run(a, scope),
        Cmd::Init(a) => taping::run(a, scope),
        Cmd::Audit(a) => audits::run(a, scope),
        Cmd::Brand(a) => {
            // with no file asked, the theme is what the command prints
            if let Some(json) = brand::run(&brand::Request { cast: a.cast, out: a.out }, &Stderr::Command)? {
                println!("{json}");
            }
            Ok(())
        }
        Cmd::Export { cast, out } => {
            let c = frames::Cast::load(&cast)?;
            let out = out.unwrap_or_else(|| {
                let s = cast.to_string_lossy();
                PathBuf::from(format!(
                    "{}.cast",
                    s.trim_end_matches(".gz").trim_end_matches(".json").trim_end_matches(".cast")
                ))
            });
            std::fs::write(&out, c.to_asciicast())?;
            eprintln!("anina: asciicast v3 -> {}", screen::lesson::shown(&out));
            Ok(())
        }
        Cmd::Check(a) => checks::run(a, scope),
        Cmd::Compare(a) => versus::run(a),
        Cmd::Doctor => doctor(),
    }
}

/// `anina run`: the preset is looked up here, so that its lines carry its name.
fn run(a: RunArgs, scope: &Scope) -> Result<()> {
    let (dir, m) = preset::load(&a.preset)?;
    let said = Stderr::Preset(m.name.clone());
    let asked = preset::Request {
        part: a.part,
        out: a.out,
        engine: a.engine,
        sandbox: a.sandbox,
        vars: a.vars,
        redact: a.redact,
        keep: a.keep,
        embed: a.embed,
    };
    let ran = preset::run(dir, m, &asked, scope, &said)?;
    if ran.failed.is_empty() {
        return Ok(());
    }
    Err(ending::ended(Ending::Journey, format!("{} part(s) failed: {}", ran.failed.len(), ran.failed.join(", "))))
}

/// `anina presets`: each preset found, what it is, its parts, and whether
/// this machine has what it needs.
fn presets() -> Result<()> {
    for (name, from) in preset::list() {
        let dir = match preset::resolve(&name) {
            Ok(d) => d,
            Err(e) => {
                println!("{name:10} ({from}) unreadable: {e:#}");
                continue;
            }
        };
        let m = match preset::specs::Manifest::load(&dir) {
            Ok(m) => m,
            Err(e) => {
                println!("{name:10} ({from}) invalid: {e:#}");
                continue;
            }
        };
        let missing: Vec<&str> =
            m.requires.iter().filter(|r| record::which(&r.program).is_none()).map(|r| r.program.as_str()).collect();
        let parts: Vec<&str> = m.parts.iter().map(|p| p.name.as_str()).collect();
        let status = if missing.is_empty() { "ready".to_string() } else { format!("needs {}", missing.join(", ")) };
        println!("{name:10} {}", m.description);
        println!(
            "{:10} parts: {} · {status}{} · {from}",
            "",
            parts.join(", "),
            if m.network { " · network" } else { "" }
        );
    }
    Ok(())
}

/// Spawn a tiny program through each engine and read its screen.
fn doctor() -> Result<()> {
    let mut ok = true;
    for name in ["tui-test", "tmux"] {
        let res = (|| -> Result<String> {
            let mut e = engine::by_name(name)?;
            let mut spec =
                engine::Spec::new("/bin/sh", vec!["-c".into(), "printf 'anina \\033[1;32mok\\033[0m'; sleep 2".into()]);
            spec.cols = 40;
            spec.rows = 5;
            e.spawn(&spec)?;
            let pat = screen::Pattern::parse("anina ok")?;
            let (seen, s) = engine::wait_for(e.as_mut(), &pat, false, std::time::Duration::from_secs(5))?;
            e.close()?;
            if !seen {
                anyhow::bail!("screen was {:?}", s.text());
            }
            Ok(format!("{:?}", s.cell(6, 0).fg))
        })();
        match res {
            Ok(fg) => println!("  ok    engine {name:9} (color read: {fg})"),
            Err(e) => {
                println!("  MISS  engine {name:9} {e:#}");
                if name == "tui-test" {
                    ok = false;
                }
            }
        }
    }
    for (tool, why) in [("bwrap", "strict sandbox on Linux"), ("sandbox-exec", "strict sandbox on macOS")] {
        let found = record::which(tool);
        println!("  {}  {tool:13} {why}", if found.is_some() { "ok   " } else { "-    " });
    }
    if ok { Ok(()) } else { anyhow::bail!("the default engine does not work here") }
}
