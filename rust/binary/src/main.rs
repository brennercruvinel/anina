mod audits;
mod checks;
mod mapper;
mod taping;

use anina_record::{self as record, preset};
use anina_render::{self as render, brand};
use anina_runner::{engine, redact};
use anina_screen::{self as screen, frames};
use anyhow::Result;
use clap::{Parser, Subcommand};
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
    Run(preset::Args),
    /// List the presets anina knows
    Presets,
    /// Play a tape and record it (cast + animated SVG)
    Record(record::Args),
    /// Render a recorded cast to an animated SVG
    Render(render::Args),
    /// Map a program by itself: every screen and key, as a graph
    Explore(mapper::Args),
    /// Write a tape by using the program
    Init(taping::Args),
    /// Check a program across terminals: sizes, colors, contrast, latency, hygiene
    Audit(audits::Args),
    /// Derive a theme from a program's own colors
    Brand(brand::Args),
    /// Export a cast to asciicast v3 (asciinema player, agg, asciinema.org)
    Export {
        cast: PathBuf,
        /// Output .cast file (default: next to the input)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Look through anina's output for anything redaction should have hidden
    Check(checks::Args),
    /// Check that anina can run here
    Doctor,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = dispatch(cli.cmd) {
        eprintln!("anina: {}", redact::Redactor::shared().quiet(&format!("{e:#}")));
        std::process::exit(1);
    }
}

fn dispatch(cmd: Cmd) -> Result<()> {
    redact::init()?;
    match cmd {
        Cmd::Run(a) => preset::run(a).and_then(|out| checks::verify(&out)),
        Cmd::Presets => preset::list(),
        Cmd::Record(a) => record::run(a).and_then(|out| checks::verify(&out)),
        Cmd::Render(a) => render::run(a),
        Cmd::Explore(a) => mapper::run(a),
        Cmd::Init(a) => taping::run(a),
        Cmd::Audit(a) => audits::run(a),
        Cmd::Brand(a) => brand::run(a),
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
        Cmd::Check(a) => checks::run(a),
        Cmd::Doctor => doctor(),
    }
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
