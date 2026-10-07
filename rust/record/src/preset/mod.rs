//! Presets: ready journeys for a program (requirements, environment, parts as
//! tapes, theme). `anina run NAME [--part P]` runs the whole journey or some
//! of its parts in a sandbox and leaves screenshots, casts, animated SVGs, an
//! index page, `run.json` and what changed on disk.
//!
//! A preset is a directory with `preset.toml` (see `specs`) and its tapes.
//! Lookup order: a path; `./demo/NAME`; each dir in `$ANINA_PRESETS`
//! (`:`-separated); `~/.config/anina/presets/NAME`; the presets built into
//! the binary.

pub mod print;
pub mod specs;

use crate::cordon::{Mode, Sandbox, diffs::Snapshot};
use crate::{Config, Options};
use anina_runner::redact::Redactor;
use anina_runner::script::{Tape, expand};
use anyhow::{Context, Result, anyhow, bail};
use include_dir::{Dir, include_dir};
use specs::Manifest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

static BUILTIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/demo");

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Preset name or directory
    pub preset: String,
    /// Run only these parts (repeatable; default: all, in order)
    #[arg(long)]
    pub part: Vec<String>,
    /// Output directory (default: anina-out/<preset>)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Terminal engine: tui-test (default) or tmux
    #[arg(long, default_value = "tui-test")]
    pub engine: String,
    /// Sandbox mode, over the preset's: off, env, strict
    #[arg(long)]
    pub sandbox: Option<String>,
    /// Set a preset variable, K=V (the environment works too)
    #[arg(long = "var", value_name = "K=V")]
    pub vars: Vec<String>,
    /// Extra redaction, REGEX=REPLACEMENT
    #[arg(long = "redact", value_name = "RE=WITH")]
    pub redact: Vec<String>,
    /// Keep the sandbox of the last run instead of starting empty
    #[arg(long)]
    pub keep: bool,
}

/// Where anina keeps clones, builds and extracted presets.
pub fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("ANINA_CACHE") {
        return PathBuf::from(d);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cache"));
    base.join("anina")
}

/// The directories presets are searched in, in order.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("demo")];
    if let Ok(v) = std::env::var("ANINA_PRESETS") {
        dirs.extend(std::env::split_paths(&v));
    }
    if let Ok(h) = std::env::var("HOME") {
        dirs.push(PathBuf::from(h).join(".config/anina/presets"));
    }
    dirs
}

/// A preset directory on disk (built-in presets are extracted to the cache).
pub fn resolve(name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.join("preset.toml").is_file() {
        return Ok(p.to_path_buf());
    }
    if name.contains('/') {
        bail!("{name} has no preset.toml");
    }
    for d in search_dirs() {
        if d.join(name).join("preset.toml").is_file() {
            return Ok(d.join(name));
        }
    }
    if let Some(dir) = BUILTIN.get_dir(name) {
        let dest = cache_dir().join("presets");
        std::fs::create_dir_all(&dest)?;
        let _ = std::fs::remove_dir_all(dest.join(name));
        // extract() writes entries by their path from the embed root
        // (`shell/x.tape`) without creating the preset's own folder
        std::fs::create_dir_all(dest.join(name))?;
        dir.extract(&dest).with_context(|| format!("extracting the {name} preset"))?;
        return Ok(dest.join(name));
    }
    let known: Vec<String> = list_all().into_iter().map(|(n, _)| n).collect();
    bail!("no preset {name:?}; known: {}", known.join(", "))
}

/// Every preset anina can find: (name, where).
fn list_all() -> Vec<(String, String)> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for d in search_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut names: Vec<_> = rd.flatten().filter(|e| e.path().join("preset.toml").is_file()).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let n = e.file_name().to_string_lossy().into_owned();
            if !seen.iter().any(|(s, _)| *s == n) {
                seen.push((n, d.display().to_string()));
            }
        }
    }
    for d in BUILTIN.dirs() {
        let n = d.path().display().to_string();
        if !seen.iter().any(|(s, _)| *s == n) {
            seen.push((n, "built in".into()));
        }
    }
    seen
}

pub fn list() -> Result<()> {
    for (name, from) in list_all() {
        let dir = match resolve(&name) {
            Ok(d) => d,
            Err(e) => {
                println!("{name:10} ({from}) unreadable: {e:#}");
                continue;
            }
        };
        let m = match Manifest::load(&dir) {
            Ok(m) => m,
            Err(e) => {
                println!("{name:10} ({from}) invalid: {e:#}");
                continue;
            }
        };
        let missing: Vec<&str> =
            m.requires.iter().filter(|r| crate::which(&r.program).is_none()).map(|r| r.program.as_str()).collect();
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

/// The variables of a run: the built-in ones, then the preset's (the
/// environment and --var win over their defaults).
fn resolve_vars(m: &Manifest, base: HashMap<String, String>, cli: &[String]) -> Result<HashMap<String, String>> {
    let mut vars = base;
    let mut over: HashMap<String, String> = HashMap::new();
    for kv in cli {
        let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("--var takes K=V, got {kv:?}"))?;
        over.insert(k.into(), v.into());
    }
    for v in &m.vars {
        let val = over
            .get(&v.name)
            .cloned()
            .or_else(|| std::env::var(&v.name).ok())
            .unwrap_or_else(|| expand(&v.default, &vars));
        vars.insert(v.name.clone(), val);
    }
    for (k, v) in over {
        vars.entry(k).or_insert(v);
    }
    Ok(vars)
}

fn missing_needs<'a>(needs: &'a [String], vars: &HashMap<String, String>) -> Vec<&'a str> {
    needs.iter().filter(|n| vars.get(*n).is_none_or(|v| v.is_empty())).map(|s| s.as_str()).collect()
}

pub fn run(a: Args) -> Result<()> {
    let dir = std::fs::canonicalize(resolve(&a.preset)?)?;
    let m = Manifest::load(&dir)?;
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("anina-out").join(&m.name));
    std::fs::create_dir_all(&out)?;
    let out = std::fs::canonicalize(&out)?;
    let started = Instant::now();
    let say = |msg: String| eprintln!("anina [{}] {msg}", m.name);

    // which parts, before any work
    let parts: Vec<&specs::Part> = if a.part.is_empty() {
        m.parts.iter().collect()
    } else {
        let mut v = Vec::new();
        for p in &a.part {
            v.push(m.parts.iter().find(|x| &x.name == p).ok_or_else(|| {
                anyhow!(
                    "{} has no part {p:?}; parts: {}",
                    m.name,
                    m.parts.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            })?);
        }
        v
    };

    // what the machine must have
    let missing: Vec<String> = m
        .requires
        .iter()
        .filter(|r| crate::which(&r.program).is_none())
        .map(|r| format!("  {}: {}", r.program, if r.hint.is_empty() { "not on PATH" } else { &r.hint }))
        .collect();
    if !missing.is_empty() {
        bail!("{} needs:\n{}", m.name, missing.join("\n"));
    }

    // the sandbox, empty unless --keep
    let mode: Mode = a.sandbox.as_deref().or(m.sandbox.as_deref()).unwrap_or("env").parse()?;
    // in the cache, not in the output: installs weigh hundreds of MB
    let root = cache_dir().join("sandbox").join(&m.name);
    if !a.keep && root.exists() {
        std::fs::remove_dir_all(&root).with_context(|| format!("emptying {}", root.display()))?;
    }
    let sb = Sandbox::new(&root, mode)?;

    let mut base = HashMap::new();
    for (k, v) in [
        ("PRESET_DIR", dir.display().to_string()),
        ("ANINA_CACHE", cache_dir().display().to_string()),
        ("OUT", out.display().to_string()),
        ("SANDBOX", sb.root.display().to_string()),
        ("HOME_DIR", sb.home.display().to_string()),
        ("WORK", sb.work.display().to_string()),
        ("TMP_DIR", sb.tmp.display().to_string()),
        ("REAL_HOME", std::env::var("HOME").unwrap_or_default()),
    ] {
        base.insert(k.to_string(), v);
    }
    let vars = resolve_vars(&m, base, &a.vars)?;

    // redaction: the sandbox paths first (they may sit under the real home),
    // then the preset's and the command line's rules, then the defaults
    let mut red = Redactor::default();
    for (p, w) in sb.redactions() {
        red.literal(&p, &w);
    }
    for r in &m.redact {
        red.push(&expand(&r.pattern, &vars), &r.with)?;
    }
    for kv in &a.redact {
        let (p, w) = kv.rsplit_once('=').ok_or_else(|| anyhow!("--redact takes REGEX=REPLACEMENT"))?;
        red.push(p, w)?;
    }
    red.rules.extend(Redactor::defaults().rules);

    // prepare: clones, builds, installs; cached by `creates`
    for (i, p) in m.prepare.iter().enumerate() {
        let label = if p.about.is_empty() { format!("prepare {}", i + 1) } else { p.about.clone() };
        let gaps = missing_needs(&p.needs, &vars);
        if !gaps.is_empty() {
            say(format!("skip {label}: set {}", gaps.join(", ")));
            continue;
        }
        if let Some(c) = &p.creates {
            let c = expand(c, &vars);
            if Path::new(&c).exists() {
                say(format!("{label}: already there ({c})"));
                continue;
            }
        }
        say(format!("{label}…"));
        let st = std::process::Command::new("sh")
            .arg("-c")
            .arg(&p.run)
            .current_dir(&out)
            .envs(&vars)
            .status()
            .context("running sh")?;
        if !st.success() {
            bail!("{label} failed ({st}): {}", p.run);
        }
    }

    let theme = m.theme.as_ref().map(|t| dir.join(t).display().to_string()).unwrap_or_default();
    let before = Snapshot::take(&sb.root);
    let mut results = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let title = if part.title.is_empty() { part.name.clone() } else { part.title.clone() };
        let gaps = missing_needs(&part.needs, &vars);
        if !gaps.is_empty() {
            say(format!(
                "part {} skipped: set {} (anina run {} --var {}=...)",
                part.name,
                gaps.join(", "),
                m.name,
                gaps[0]
            ));
            results.push(print::PartResult::skipped(part, &title, format!("needs {}", gaps.join(", "))));
            continue;
        }
        say(format!("part {}/{} {} — {title}", i + 1, parts.len(), part.name));
        let pdir = out.join(&part.name);
        let _ = std::fs::remove_dir_all(&pdir);
        std::fs::create_dir_all(&pdir)?;
        let t0 = Instant::now();
        let res = (|| -> Result<crate::Recording> {
            let mut tvars = vars.clone();
            tvars.insert("ANINA_OUT".into(), pdir.display().to_string());
            let tape = Tape::load(&dir.join(&part.tape), &tvars)?;
            let cfg = Config {
                cwd: Some(sb.work.clone()),
                env: sb.env(),
                clean_env: sb.clean_env(),
                wrap: sb.wrap(),
                theme: theme.clone(),
                title: title.clone(),
                ..Config::default()
            };
            let opts = Options {
                engine: a.engine.clone(),
                out_dir: pdir.clone(),
                redactor: Some(red.clone()),
                png: true,
                ..Default::default()
            };
            crate::record(&tape, cfg, opts)
        })();
        let secs = t0.elapsed().as_secs_f64();
        match res {
            Ok(rec) => {
                for w in &rec.warnings {
                    say(format!("  warning: {w}"));
                }
                let cast_path = pdir.join(format!("{}.cast.json.gz", part.name));
                rec.cast.save(&cast_path)?;
                let svg = pdir.join(format!("{}.svg", part.name));
                let svg_ok = match anina_render::shape::write_cast_svg(&rec.cast, None, &svg) {
                    Ok(()) => true,
                    Err(e) => {
                        say(format!("  no svg: {e:#}"));
                        false
                    }
                };
                say(format!(
                    "  ok in {secs:.1}s: {} frames, {} screens, {} shots",
                    rec.cast.frames.len(),
                    rec.cast.screens.len(),
                    rec.shots.len()
                ));
                results.push(print::PartResult::ok(part, &title, &rec, secs, svg_ok));
            }
            Err(e) => {
                say(format!("  FAILED after {secs:.1}s: {e:#}"));
                results.push(print::PartResult::failed(part, &title, red.text(&format!("{e:#}")), secs));
            }
        }
    }
    let changes = before.diff(&Snapshot::take(&sb.root));
    std::fs::write(out.join("changes.json"), serde_json::to_string(&changes)?)?;
    say(format!("disk: {}", changes.summary().replace('\n', "\n           ")));

    let run = print::Run {
        preset: m.name.clone(),
        description: m.description.clone(),
        homepage: m.homepage.clone(),
        recorded: anina_screen::frames::now_iso(),
        seconds: (started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
        engine: a.engine.clone(),
        sandbox: format!("{mode:?}").to_lowercase(),
        parts: results,
        changes: changes.summary(),
    };
    std::fs::write(out.join("run.json"), red.to_json(&run)?)?;
    print::write_index(&out, &run)?;
    if let Some(r) = anina_runner::redact::summary() {
        say(format!("redacted {r}"));
    }
    say(format!("-> {}", out.join("index.html").display()));
    let failed: Vec<&str> = run.parts.iter().filter(|p| p.status == "failed").map(|p| p.name.as_str()).collect();
    if !failed.is_empty() {
        bail!("{} part(s) failed: {}", failed.len(), failed.join(", "));
    }
    Ok(())
}
