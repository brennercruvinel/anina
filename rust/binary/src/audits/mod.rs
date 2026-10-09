//! anina audit: a Lighthouse for terminal programs. The same journey (a
//! tape, or a bare command anina opens and quits by itself) runs in several
//! terminals: sizes, color capabilities, locales. Each run is measured
//! (crashes, time to first screen, key latency, WCAG contrast, honoring
//! NO_COLOR and TERM, the terminal state left at exit) and scored.
//!
//! With `--preset`, the journey is a preset's part, played in the preset's
//! sandbox; every terminal starts from the sandbox as prepare left it.

pub mod cases;
pub mod meter;
pub mod print;
pub mod ratio;
pub mod score;

use anina_record::preset::{Stage, StageArgs};
use anina_record::{self as record, Config, Options};
use anina_runner::engine::{self, Spec};
use anina_runner::redact::Redactor;
use anina_runner::script::{Line, Step, Tape};
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[derive(clap::Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["tape", "command", "preset"])))]
pub struct Args {
    /// The journey: a tape (anina or vhs dialect)
    pub tape: Option<PathBuf>,
    /// Or a program to open and quit by itself, after --
    #[arg(last = true)]
    pub command: Vec<String>,
    /// Or a preset: each part played in every terminal, in the preset's sandbox
    #[arg(long)]
    pub preset: Option<String>,
    /// With --preset: only these parts (repeatable)
    #[arg(long, requires = "preset")]
    pub part: Vec<String>,
    /// With --preset: sandbox mode over the preset's: off, env, strict
    #[arg(long, requires = "preset")]
    pub sandbox: Option<String>,
    /// Put the screenshots inside report.html: one file to share
    #[arg(long)]
    pub embed: bool,
    /// Output directory (default: anina-out/audit-<name>)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// A smaller matrix: baseline, smallest size, NO_COLOR, 16 colors
    #[arg(long, conflicts_with = "full")]
    pub quick: bool,
    /// Every combination of size, colors and locale
    #[arg(long)]
    pub full: bool,
    /// Terminal sizes to try besides the 120x40 baseline (COLSxROWS,...)
    #[arg(long, value_delimiter = ',')]
    pub sizes: Vec<String>,
    /// Run every terminal even when the baseline journey fails
    #[arg(long)]
    pub keep_going: bool,
    /// Screen samples per second while recording (timing resolution)
    #[arg(long, default_value_t = 60.0)]
    pub fps: f64,
    /// Terminal engine: tui-test (default) or tmux
    #[arg(long, default_value = "tui-test")]
    pub engine: String,
    /// Variables for the tape, K=V
    #[arg(long = "var", value_name = "K=V")]
    pub vars: Vec<String>,
}

pub fn run(a: Args) -> Result<()> {
    if let Some(p) = &a.preset {
        return run_preset(&a, p);
    }
    let (name, target) = match (&a.tape, a.command.first()) {
        (Some(t), _) => {
            (t.file_stem().map_or("tape".into(), |s| s.to_string_lossy().into_owned()), t.display().to_string())
        }
        (None, Some(p)) => {
            (Path::new(p).file_name().map_or("cmd".into(), |s| s.to_string_lossy().into_owned()), a.command.join(" "))
        }
        _ => bail!("give a tape, or a program after --"),
    };
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("anina-out").join(format!("audit-{name}")));
    std::fs::create_dir_all(&out)?;
    anina_runner::redact::writes_to(&out);

    let tape = match &a.tape {
        Some(path) => {
            let mut vars = anina_runner::script::base_vars(path, &out);
            for kv in &a.vars {
                let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("--var takes K=V"))?;
                vars.insert(k.into(), v.into());
            }
            Tape::load(path, &vars)?
        }
        None => {
            eprintln!("anina: opening {} once to learn how it quits", a.command[0]);
            let p = probe(&a.command, &a.engine)?;
            eprintln!("anina: {}", p.describe());
            let (src, tape) = p.tape(&a.command);
            std::fs::write(out.join("journey.tape"), &src)?;
            tape
        }
    };
    audit(&a, &tape, &target, &out, None)
}

/// Audit every part of a preset (or `--part`), one report per part.
fn run_preset(a: &Args, preset: &str) -> Result<()> {
    let (dir, m) = anina_record::preset::load(preset)?;
    // picked before any work: a wrong --part fails here
    let parts: Vec<anina_record::preset::specs::Part> = m.pick(&a.part)?.into_iter().cloned().collect();
    let name = m.name.clone();
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("anina-out").join(format!("audit-{name}")));
    std::fs::create_dir_all(&out)?;
    let out = std::fs::canonicalize(&out)?;
    anina_runner::redact::writes_to(&out);
    let slot = format!("audit-{name}");
    let stage = Stage::new(
        dir,
        m,
        &StageArgs {
            out: &out,
            sandbox: a.sandbox.as_deref(),
            vars: &a.vars,
            redact: &[],
            keep: false,
            slot: Some(&slot),
        },
    )?;
    stage.freeze()?;
    for part in &parts {
        let gaps = stage.gaps(part);
        if !gaps.is_empty() {
            eprintln!("anina [{name}] part {} skipped: set {}", part.name, gaps.join(", "));
            continue;
        }
        let pout = if parts.len() == 1 { out.clone() } else { out.join(&part.name) };
        std::fs::create_dir_all(&pout)?;
        let tape = stage.tape(part, &pout)?;
        eprintln!("anina [{name}] auditing part {} · {}", part.name, Stage::title(part));
        audit(a, &tape, &format!("{name} · {}", part.name), &pout, Some(&stage))?;
    }
    Ok(())
}

/// Play `tape` in every terminal of the matrix, score it, write the report.
/// With a stage, each terminal runs in the preset's sandbox, restored first.
fn audit(a: &Args, tape: &Tape, target: &str, out: &Path, stage: Option<&Stage>) -> Result<()> {
    let sizes = cases::parse_sizes(&a.sizes)?;
    let variants = cases::variants(&sizes, a.quick, a.full);
    let mut outcomes = Vec::new();
    for (i, v) in variants.iter().enumerate() {
        let dir = out.join("terminals").join(&v.name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let opts = Options {
            engine: a.engine.clone(),
            out_dir: dir.clone(),
            force: v.force(a.fps),
            redactor: Some(stage.map_or_else(Redactor::defaults, |st| st.red.clone())),
            png: false,
            quiet: true,
        };
        let started = Instant::now();
        let cfg = match stage {
            Some(st) => {
                st.restore()?;
                st.config(&v.name)
            }
            None => Config::default(),
        };
        let rec = record::record(tape, cfg, opts);
        let mut o = meter::outcome(v, rec, &dir);
        if let Some(s) = &o.shot {
            let png = dir.join("screen.png");
            if anina_render::pixel::screen_png(s, &v.name, "", &png).is_ok() {
                o.shot_png = png.strip_prefix(out).ok().map(|p| p.display().to_string());
            }
        }
        eprintln!(
            "anina: [{}/{}] {:<22} {:<8} {:.1}s",
            i + 1,
            variants.len(),
            v.name,
            o.status.label(),
            started.elapsed().as_secs_f64()
        );
        let base_failed = v.baseline && !matches!(o.status, meter::Status::Ok);
        outcomes.push(o);
        if base_failed && !a.keep_going {
            eprintln!("anina: the journey fails in the baseline terminal; stopping (--keep-going runs the rest)");
            break;
        }
    }

    let scores = score::all_scores(&outcomes);
    let rep = print::Report {
        tool: "anina audit",
        version: env!("CARGO_PKG_VERSION"),
        generated: anina_screen::frames::now_iso(),
        target: target.to_string(),
        engine: a.engine.clone(),
        scores: &scores,
        variants: &outcomes,
    };
    print::write_json(&rep, &out.join("report.json"))?;
    print::write_html(&rep, &out.join("report.html"), a.embed)?;
    print!("{}", Redactor::shared().quiet(&print::summary(&rep)));
    if let Some(r) = anina_runner::redact::summary() {
        eprintln!("\nanina: redacted {r}");
    }
    eprintln!("\nanina: report -> {}", anina_screen::lesson::shown(&out.join("report.html")));
    Ok(())
}

/// How a bare command behaves on its own: whether it exits by itself, and
/// the keys that quit it.
#[derive(Debug, Default)]
pub struct Probe {
    pub exits_after: Option<f64>,
    pub quit_keys: Vec<String>,
}

impl Probe {
    fn describe(&self) -> String {
        match (self.exits_after, self.quit_keys.is_empty()) {
            (Some(t), true) => format!("it exits by itself after {t:.1}s"),
            (_, false) => format!("it quits with: {}", self.quit_keys.join(", ")),
            (None, true) => "found no way to quit it; the journey ends with it running".into(),
        }
    }

    /// The journey: open, settle, quit. Built as steps (the command's words
    /// are literal, no variable expansion) and also as tape text to keep.
    fn tape(&self, cmd: &[String]) -> (String, Tape) {
        let mut steps = vec![Step::Spawn(cmd.to_vec()), Step::Idle { timeout: Some(5.0) }];
        if let Some(after) = self.exits_after {
            steps.push(Step::Sleep(after + 0.5));
        }
        for k in &self.quit_keys {
            steps.push(Step::Key { keys: vec![(k.clone(), 1)], delay: None });
            steps.push(Step::Idle { timeout: Some(3.0) });
        }
        let argv: Vec<String> = cmd.iter().map(|w| quote(w)).collect();
        let mut src = format!("# written by anina audit\nSpawn {}\nIdle 5\n", argv.join(" "));
        if let Some(after) = self.exits_after {
            src.push_str(&format!("Sleep {:.2}\n", after + 0.5));
        }
        for k in &self.quit_keys {
            src.push_str(&format!("Key {k}\nIdle 3\n"));
        }
        let file = PathBuf::from("journey.tape");
        let lines =
            steps.into_iter().enumerate().map(|(i, step)| Line { file: file.clone(), n: i + 1, step }).collect();
        (src, Tape { path: file, lines, warnings: Vec::new() })
    }
}

/// Double-quote a word for the tape language.
fn quote(w: &str) -> String {
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c)) {
        return w.to_string();
    }
    let mut s = String::from("\"");
    for c in w.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

/// Keys the screen offers for quitting, most likely first. Reads the usual
/// hint forms: `q quit`, `ctrl+q quit`, `^X Exit`, `[k] Kill`, `<q> close`,
/// `Quit : q`, `exit: esc`.
pub fn quit_hints(text: &str) -> Vec<String> {
    static KEY_WORD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:^|[\s·|,;(\[])(ctrl\s*\+\s*[a-z]|\^[a-z]|c-[a-z]|esc(?:ape)?|f10|\[[a-z]\]|<[a-z]>|\([a-z]\)|[a-z])\s*[:·\-–=]?\s*(?:to\s+)?(quit|exit|close|leave|kill|stop)\b").unwrap()
    });
    static WORD_KEY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(quit|exit|close|leave|kill|stop)\b[^:=\n]{0,12}[:=]\s*(ctrl\s*\+\s*[a-z]|\^[a-z]|c-[a-z]|esc(?:ape)?|f10|[a-z])\b").unwrap()
    });
    let rank = |w: &str| {
        ["quit", "exit", "close", "leave", "kill", "stop"].iter().position(|x| x.eq_ignore_ascii_case(w)).unwrap_or(9)
    };
    let mut found: Vec<(usize, usize, String)> = Vec::new();
    for line in text.lines() {
        for c in KEY_WORD.captures_iter(line) {
            found.push((rank(&c[2]), found.len(), key_name(&c[1])));
        }
        for c in WORD_KEY.captures_iter(line) {
            found.push((rank(&c[1]), found.len(), key_name(&c[2])));
        }
    }
    found.sort();
    let mut keys: Vec<String> = Vec::new();
    for (_, _, k) in found {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

fn key_name(raw: &str) -> String {
    let k: String = raw.chars().filter(|c| !c.is_whitespace() && !"[]<>()".contains(*c)).collect();
    let lower = k.to_ascii_lowercase();
    if let Some(c) = lower.strip_prefix('^') {
        return format!("ctrl+{c}");
    }
    if lower == "escape" {
        return "esc".into();
    }
    engine::normalize_key(&if k.len() == 1 { k } else { lower })
}

/// Open the program once (baseline terminal), let it settle, and find out
/// how it ends: by itself, by a quit key it shows, or by a usual one.
pub fn probe(cmd: &[String], engine_name: &str) -> Result<Probe> {
    let mut e = engine::by_name(engine_name)?;
    let mut spec = Spec::new(cmd[0].clone(), cmd[1..].to_vec());
    spec.cols = cases::BASE.0;
    spec.rows = cases::BASE.1;
    spec.cwd = std::env::current_dir().ok();
    let t0 = Instant::now();
    e.spawn(&spec)?;
    let mut p = Probe::default();
    // settle, or exit on its own
    let end = Instant::now() + Duration::from_secs(10);
    let mut last = e.screen()?.hash(None);
    let mut quiet_since = Instant::now();
    while Instant::now() < end {
        if e.exit_code()?.is_some() {
            p.exits_after = Some(t0.elapsed().as_secs_f64());
            e.close()?;
            return Ok(p);
        }
        std::thread::sleep(Duration::from_millis(30));
        let h = e.screen()?.hash(None);
        if h != last {
            last = h;
            quiet_since = Instant::now();
        } else if quiet_since.elapsed() > Duration::from_millis(1500) {
            break;
        }
    }
    let mut tried: Vec<String> = Vec::new();
    for _ in 0..6 {
        let screen = e.screen()?;
        let mut candidates = quit_hints(&screen.text());
        for k in ["ctrl+c", "q", "esc", "ctrl+d", "ctrl+q"] {
            if !candidates.iter().any(|c| c == k) {
                candidates.push(k.into());
            }
        }
        let Some(k) = candidates.into_iter().find(|k| !tried.contains(k)) else {
            break;
        };
        e.key(&k)?;
        tried.push(k.clone());
        p.quit_keys.push(k);
        // give it a moment to exit or to show what comes next (a confirm popup)
        let (_, _) = engine::wait_idle(e.as_mut(), None, Duration::from_millis(400), Duration::from_secs(3))?;
        if e.exit_code()?.is_some() {
            e.close()?;
            return Ok(p);
        }
    }
    e.close()?;
    p.quit_keys.clear();
    Ok(p)
}
