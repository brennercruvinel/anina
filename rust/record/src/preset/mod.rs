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

use crate::cordon::diffs::{Differ, Snapshot};
use crate::cordon::{Mode, Sandbox};
use crate::{Config, Options};
use anina_runner::redact::{Redactor, scope::Scope};
use anina_runner::script::{Tape, expand};
use anina_screen::notice::Notice;
use anyhow::{Context, Result, anyhow, bail};
use include_dir::{Dir, include_dir};
use specs::Manifest;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

static BUILTIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/demo");

/// What `run` is asked of the preset `load` returned.
#[derive(Clone, Debug)]
pub struct Request {
    /// Run only these parts; all of them, in order, when empty.
    pub part: Vec<String>,
    /// Output directory; `anina-out/<preset>` when none.
    pub out: Option<PathBuf>,
    /// Terminal engine: `tui-test` or `tmux`.
    pub engine: String,
    /// Sandbox mode, over the preset's: `off`, `env`, `strict`.
    pub sandbox: Option<String>,
    /// Preset variables, `K=V` (the environment works too).
    pub vars: Vec<String>,
    /// Extra redaction, `REGEX=REPLACEMENT`.
    pub redact: Vec<String>,
    /// Keep the sandbox of the last run instead of starting empty.
    pub keep: bool,
    /// Put the images inside index.html: one file to share.
    pub embed: bool,
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
    if let Some(dir) = BUILTIN.get_dir(name).filter(|d| is_builtin(d)) {
        let cache = cache_dir();
        // a `prepare` writes the cache: a link it left on the way to this
        // folder would lead the emptying and the extraction elsewhere
        crate::cordon::kept(&cache, "presets", name)?;
        let dest = cache.join("presets");
        std::fs::create_dir_all(&dest)?;
        let _ = std::fs::remove_dir_all(dest.join(name));
        // extract() writes entries by their path from the embed root
        // (`shell/x.tape`) without creating the preset's own folder
        std::fs::create_dir_all(dest.join(name))?;
        dir.extract(&dest).with_context(|| format!("extracting the {name} preset"))?;
        return Ok(dest.join(name));
    }
    let known: Vec<String> = list().into_iter().map(|(n, _)| n).collect();
    bail!("no preset {name:?}; known: {}", known.join(", "))
}

/// A preset found and read, once per run: its folder (absolute) and manifest.
/// A built-in preset is extracted to the cache on every lookup, so a run
/// looks it up here and hands the result on.
pub fn load(name: &str) -> Result<(PathBuf, Manifest)> {
    let dir = std::fs::canonicalize(resolve(name)?)?;
    let m = Manifest::load(&dir)?;
    Ok((dir, m))
}

/// A built-in folder is a preset only with its manifest, the same rule a
/// folder on disk follows.
fn is_builtin(d: &Dir) -> bool {
    d.get_file(d.path().join("preset.toml")).is_some()
}

/// Every preset anina can find, in lookup order: its name and where it was
/// found (a folder, or `built in`). `resolve` and `Manifest::load` read one.
pub fn list() -> Vec<(String, String)> {
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
    for d in BUILTIN.dirs().filter(|d| is_builtin(d)) {
        let n = d.path().display().to_string();
        if !seen.iter().any(|(s, _)| *s == n) {
            seen.push((n, "built in".into()));
        }
    }
    seen
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

/// A preset made ready to play: requirements checked, sandbox created,
/// variables resolved, redaction built, prepare steps done. `run` plays its
/// parts once; `anina audit --preset` plays them in many terminals.
pub struct Stage {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub mode: Mode,
    pub sandbox: Sandbox,
    pub vars: HashMap<String, String>,
    pub red: Redactor,
    pub theme: String,
    /// The sandbox as `freeze` or the last `restore` left it, and whether no
    /// play has touched it since.
    base: RefCell<Snapshot>,
    fresh: Cell<bool>,
}

/// What `stage` needs from the command line.
pub struct StageArgs<'a> {
    pub out: &'a Path,
    pub sandbox: Option<&'a str>,
    pub vars: &'a [String],
    pub redact: &'a [String],
    /// Keep the sandbox of the last run instead of starting empty.
    pub keep: bool,
    /// The sandbox folder under the cache: the preset's name by default.
    pub slot: Option<&'a str>,
}

impl Stage {
    /// Stage the preset `load` returned; its redaction counts in `scope`, and
    /// what prepare does is said to `notice`.
    pub fn new(dir: PathBuf, m: Manifest, a: &StageArgs, scope: &Scope, notice: &dyn Notice) -> Result<Stage> {
        let say = |msg: String| notice.status(&msg);

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

        // the sandbox, empty unless keep
        let mode: Mode = a.sandbox.or(m.sandbox.as_deref()).unwrap_or("env").parse()?;
        // in the cache, not in the output: installs weigh hundreds of MB
        let root = crate::cordon::place(&cache_dir(), a.slot.unwrap_or(&m.name))?;
        if !a.keep && root.exists() {
            remove_tree(&root).with_context(|| format!("emptying {}", root.display()))?;
        }
        let sb = Sandbox { network: m.network, ..Sandbox::new(&root, mode)? };

        let mut base = HashMap::new();
        for (k, v) in [
            ("PRESET_DIR", dir.display().to_string()),
            ("ANINA_CACHE", cache_dir().display().to_string()),
            ("OUT", a.out.display().to_string()),
            ("SANDBOX", sb.root.display().to_string()),
            ("HOME_DIR", sb.home.display().to_string()),
            ("WORK", sb.work.display().to_string()),
            ("TMP_DIR", sb.tmp.display().to_string()),
            ("REAL_HOME", std::env::var("HOME").unwrap_or_default()),
        ] {
            base.insert(k.to_string(), v);
        }
        let vars = resolve_vars(&m, base, a.vars)?;

        // redaction: the sandbox paths first (they may sit under the real home),
        // then the preset's and the command line's rules, then the defaults
        let mut red = scope.redactor().blank();
        for (p, w) in sb.redactions() {
            red.literal(&p, &w);
        }
        for r in &m.redact {
            red.push(&expand(&r.pattern, &vars), &r.with)?;
        }
        for kv in a.redact {
            let (p, w) = kv.rsplit_once('=').ok_or_else(|| anyhow!("--redact takes REGEX=REPLACEMENT"))?;
            red.push(p, w)?;
        }
        red.rules.extend(scope.redactor().rules.iter().cloned());

        // prepare: clones, builds, installs; cached by `creates`. Under strict
        // it writes the cache too, where clones and builds are kept (ADR-0006)
        let prepares = sb.wrap_with(&[std::fs::canonicalize(cache_dir())?]);
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
            prepares.sh(&[], &p.run).cwd(a.out).envs(&vars).run(&label)?;
        }
        // before anina goes back to the root or to its saved copy
        sb.still_in_place("prepare", &[pristine_of(&sb.root)])?;

        let theme = m.theme.as_ref().map(|t| dir.join(t).display().to_string()).unwrap_or_default();
        Ok(Stage {
            dir,
            manifest: m,
            mode,
            sandbox: sb,
            vars,
            red,
            theme,
            base: RefCell::default(),
            fresh: Cell::new(false),
        })
    }

    /// The variables `part` still misses, empty when it can run.
    pub fn gaps<'a>(&self, part: &'a specs::Part) -> Vec<&'a str> {
        missing_needs(&part.needs, &self.vars)
    }

    pub fn title(part: &specs::Part) -> String {
        if part.title.is_empty() { part.name.clone() } else { part.title.clone() }
    }

    /// `part`'s tape, with `$ANINA_OUT` set to `out`.
    pub fn tape(&self, part: &specs::Part, out: &Path) -> Result<Tape> {
        let mut tvars = self.vars.clone();
        tvars.insert("ANINA_OUT".into(), out.display().to_string());
        Tape::load(&self.dir.join(&part.tape), &tvars)
    }

    /// How a part runs: in the sandbox, with the preset's theme.
    pub fn config(&self, title: &str) -> Config {
        Config {
            cwd: Some(self.sandbox.work.clone()),
            env: self.sandbox.env(),
            clean_env: self.sandbox.clean_env(),
            wrap: self.sandbox.wrap(),
            theme: self.theme.clone(),
            title: title.to_string(),
            ..Config::default()
        }
    }

    fn pristine(&self) -> PathBuf {
        pristine_of(&self.sandbox.root)
    }

    /// Keep the sandbox as it is now (after prepare), to start every later
    /// play from the same state with `restore`.
    pub fn freeze(&self) -> Result<()> {
        let to = self.pristine();
        remove_tree(&to)?;
        copy_tree(&self.sandbox.root, &to).with_context(|| format!("saving {}", to.display()))?;
        self.base.replace(Snapshot::take(&self.sandbox.root));
        self.fresh.set(true);
        Ok(())
    }

    /// Put the sandbox back as `freeze` left it. Nothing to do before the
    /// first play; after one, only what it changed is put back (an install of
    /// hundreds of MB stays where it is) and read again, and the whole tree
    /// is copied again only when that fails, which is said to `notice`.
    pub fn restore(&self, notice: &dyn Notice) -> Result<()> {
        if self.fresh.replace(false) {
            return Ok(());
        }
        let root = &self.sandbox.root;
        let mut now = Snapshot::take(root);
        let changed = self.base.borrow().differs(&now);
        match put_back(root, &self.pristine(), &changed) {
            Ok(()) => now.refresh(root, &changed),
            Err(e) => {
                let n = changed.len();
                notice.status(&format!("putting back {n} path(s) failed ({e}); copying the whole sandbox"));
                remove_tree(root).with_context(|| format!("emptying {}", root.display()))?;
                copy_tree(&self.pristine(), root).with_context(|| format!("restoring {}", root.display()))?;
                now = Snapshot::take(root);
            }
        }
        self.base.replace(now);
        Ok(())
    }
}

/// Where `freeze` keeps a sandbox: `pristine/<name>` next to
/// `sandbox/<name>`, the whole name kept (`with_extension` would give
/// `audit-tool.a` and `audit-tool.b` the same copy) and out of the sandboxes'
/// folder (a preset named `x.pristine` can't land on it).
fn pristine_of(root: &Path) -> PathBuf {
    let name = root.file_name().unwrap_or_default();
    root.parent().and_then(Path::parent).unwrap_or(Path::new(".")).join("pristine").join(name)
}

/// Each path that differs, as `saved` holds it: gone when it isn't there,
/// folders with their permissions. Fails on anything but a missing path, so
/// the caller can fall back to a whole copy.
fn put_back(root: &Path, saved: &Path, changed: &[Differ]) -> std::io::Result<()> {
    use std::io::ErrorKind::NotFound;
    use std::os::unix::fs::PermissionsExt;
    // folders that end with saved's permissions: those that changed, and the
    // parents of what is replaced (opened for writing meanwhile)
    let mut folders: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    for d in changed {
        match d {
            Differ::Mode(p) => folders.insert(p.clone()),
            Differ::Path(p) => folders.insert(p.parent().map(Path::to_path_buf).unwrap_or_default()),
        };
    }
    for f in &folders {
        let mut perm = std::fs::symlink_metadata(root.join(f))?.permissions();
        perm.set_mode(perm.mode() | 0o700);
        std::fs::set_permissions(root.join(f), perm)?;
    }
    for d in changed {
        let Differ::Path(p) = d else { continue };
        let (now, then) = (root.join(p), saved.join(p));
        match std::fs::symlink_metadata(&now) {
            Ok(md) if md.is_dir() => remove_tree(&now)?,
            Ok(_) => std::fs::remove_file(&now)?,
            Err(e) if e.kind() == NotFound => {}
            Err(e) => return Err(e),
        }
        match std::fs::symlink_metadata(&then) {
            Ok(md) => copy_entry(&then, &now, md.file_type())?,
            Err(e) if e.kind() == NotFound => {}
            Err(e) => return Err(e),
        }
    }
    // deepest first: a parent closed again would lock its children out;
    // putting its entries back moved its mtime
    for f in folders.iter().rev() {
        let md = std::fs::symlink_metadata(saved.join(f))?;
        keep_times(&md, &root.join(f));
        std::fs::set_permissions(root.join(f), md.permissions())?;
    }
    Ok(())
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = remove_tree(&self.pristine());
    }
}

/// What `run` leaves: where it wrote, and the parts whose journey failed.
#[derive(Debug)]
pub struct Ran {
    pub out: PathBuf,
    pub failed: Vec<String>,
}

/// Run the preset `load` returned (`dir`, `m`). Each step of the way is said
/// to `notice`; a part whose journey failed is in what it returns, not an
/// error.
pub fn run(dir: PathBuf, m: Manifest, a: &Request, scope: &Scope, notice: &dyn Notice) -> Result<Ran> {
    // picked before any work: a wrong --part fails here
    let parts: Vec<specs::Part> = m.pick(&a.part)?.into_iter().cloned().collect();
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("anina-out").join(&m.name));
    std::fs::create_dir_all(&out)?;
    let out = std::fs::canonicalize(&out)?;
    scope.writes_to(&out);
    let started = Instant::now();
    let args = StageArgs {
        out: &out,
        sandbox: a.sandbox.as_deref(),
        vars: &a.vars,
        redact: &a.redact,
        keep: a.keep,
        slot: None,
    };
    let stage = Stage::new(dir, m, &args, scope, notice)?;
    let (m, sb, red) = (&stage.manifest, &stage.sandbox, &stage.red);
    let say = |msg: String| notice.status(&msg);

    let before = Snapshot::take(&sb.root);
    let mut results = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let title = Stage::title(part);
        let gaps = stage.gaps(part);
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
        say(format!("part {}/{} {} · {title}", i + 1, parts.len(), part.name));
        let pdir = out.join(&part.name);
        let _ = std::fs::remove_dir_all(&pdir);
        std::fs::create_dir_all(&pdir)?;
        let t0 = Instant::now();
        let res = (|| -> Result<crate::Recording> {
            let tape = stage.tape(part, &pdir)?;
            let cfg = stage.config(&title);
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
                let svg_ok = match anina_render::svg::write_cast_svg(&rec.cast, None, &svg, notice) {
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
                say(format!("  FAILED after {secs:.1}s: {}", red.quiet(&format!("{e:#}"))));
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
        sandbox: format!("{:?}", stage.mode).to_lowercase(),
        parts: results,
        changes: changes.summary(),
    };
    std::fs::write(out.join("run.json"), red.to_json(&run)?)?;
    print::write_index(&out, &run, a.embed, scope.redactor())?;
    if let Some(r) = scope.redactor().summary() {
        say(format!("redacted {r}"));
    }
    say(format!("-> {}", anina_screen::lesson::shown(&out.join("index.html"))));
    let failed = run.parts.iter().filter(|p| p.status == "failed").map(|p| p.name.clone()).collect();
    Ok(Ran { out, failed })
}

#[cfg(test)]
mod tests;

mod trees;
use trees::*;
