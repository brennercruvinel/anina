//! Play a tape against a program and record it: every screen that changed
//! (sampled FPS times a second on one thread, no races), the cursor, the
//! caption and the playback speed, plus what audit needs (when each key went
//! in, when the first screen came up, the terminal modes at the end).

pub mod cordon;
pub mod preset;

use anina_runner::engine::{self, Engine, Modes, Spec};
use anina_runner::redact::Redactor;
use anina_runner::script::{Step, Tape};
use anina_screen::frames::{Cast, Frame, now_iso};
use anina_screen::{Pattern, Screen};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How a tape runs, before its own `Set` lines.
#[derive(Clone, Debug)]
pub struct Config {
    pub cols: u16,
    pub rows: u16,
    pub fps: f64,
    pub typing: f64,
    pub key_delay: f64,
    pub wait_timeout: f64,
    pub end_hold: f64,
    pub title: String,
    pub theme: String,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub clean_env: bool,
    pub shell: Vec<String>,
    /// An argv prefix the program runs under (an OS sandbox, `nice`...).
    pub wrap: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            cols: 100,
            rows: 30,
            fps: 10.0,
            typing: 0.06,
            key_delay: 0.12,
            wait_timeout: 30.0,
            end_hold: 3.0,
            title: String::new(),
            theme: String::new(),
            cwd: None,
            env: Vec::new(),
            clean_env: false,
            shell: vec!["sh".into()],
            wrap: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub engine: String,
    /// Where screenshots (`Snap NAME`) and failure dumps go.
    pub out_dir: PathBuf,
    /// Settings that win over the tape's own `Set` (audit uses this to run
    /// one tape in many terminals): `cols`, `rows`, `env K=V`, `fps`...
    pub force: Vec<(String, Vec<String>)>,
    pub redactor: Option<Redactor>,
    /// Write PNGs for `Snap NAME` (off for audit and explore speed).
    pub png: bool,
    pub quiet: bool,
}

#[derive(Clone, Debug)]
pub struct Shot {
    pub name: String,
    pub screen: Screen,
}

#[derive(Clone, Debug)]
pub struct Input {
    /// Seconds on the recording timeline.
    pub t: f64,
    pub what: String,
}

#[derive(Clone, Debug)]
pub struct Recording {
    pub cast: Cast,
    pub shots: Vec<Shot>,
    pub inputs: Vec<Input>,
    /// When something first showed on screen.
    pub first_screen: Option<f64>,
    pub exit_code: Option<i32>,
    /// Terminal modes at the end (after the program exited, if it did).
    pub end_modes: Option<Modes>,
    pub warnings: Vec<String>,
}

struct Rec {
    eng: Box<dyn Engine>,
    cfg: Config,
    opts: Options,
    started: bool,
    t0: Instant,
    offset: f64,
    hidden_at: Option<Instant>,
    recording: bool,
    caption: String,
    speed: f64,
    poster: Option<usize>,
    next_sample: Instant,
    last_key: Option<(u64, Option<[u16; 2]>, String, u64)>,
    screens: Vec<String>,
    index: HashMap<u64, usize>,
    frames: Vec<Frame>,
    shots: Vec<Shot>,
    inputs: Vec<Input>,
    first_screen: Option<f64>,
    warnings: Vec<String>,
    last_screen: Option<Screen>,
}

impl Rec {
    fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64() + self.offset
    }

    fn read(&mut self) -> Result<Screen> {
        let mut s = self.eng.screen()?;
        if let Some(r) = &self.opts.redactor {
            r.apply(&mut s);
        }
        Ok(s)
    }

    /// Sample when due (or now with `force`); returns the screen read, if any.
    fn tick(&mut self, force: bool) -> Result<Option<Screen>> {
        if !self.started || (!force && Instant::now() < self.next_sample) {
            return Ok(None);
        }
        self.next_sample = Instant::now() + Duration::from_secs_f64(1.0 / self.cfg.fps.max(0.5));
        let s = self.read()?;
        if self.first_screen.is_none() && !s.text().trim().is_empty() {
            self.first_screen = Some(self.now());
        }
        if self.recording {
            self.push(&s, force);
        }
        self.last_screen = Some(s.clone());
        Ok(Some(s))
    }

    fn push(&mut self, s: &Screen, force: bool) {
        let h = s.hash(None);
        let cursor = s.cursor.visible.then_some([s.cursor.x, s.cursor.y]);
        let key = (h, cursor, self.caption.clone(), self.speed.to_bits());
        if !force && self.last_key.as_ref() == Some(&key) {
            return;
        }
        self.last_key = Some(key);
        let id = *self.index.entry(h).or_insert_with(|| {
            self.screens.push(s.to_ansi());
            self.screens.len() - 1
        });
        let t = (self.now() * 1000.0).round() / 1000.0;
        self.frames.push(Frame { t, s: id, cursor, caption: self.caption.clone(), speed: self.speed });
    }

    /// Keep sampling for `secs`.
    fn pump(&mut self, secs: f64) -> Result<()> {
        let end = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
        loop {
            self.tick(false)?;
            let now = Instant::now();
            if now >= end {
                return Ok(());
            }
            let next = self.next_sample.max(now);
            std::thread::sleep((end.min(next) - now).max(Duration::from_millis(1)).min(Duration::from_millis(20)));
        }
    }

    fn need(&mut self) -> Result<()> {
        if !self.started {
            // vhs tapes never spawn: they type into a shell
            let shell = self.cfg.shell.clone();
            self.spawn(&shell)?;
            // let the shell draw its prompt before anything is typed
            let w = self.warnings.len();
            self.idle(Some(3.0))?;
            self.warnings.truncate(w);
        }
        Ok(())
    }

    fn spawn(&mut self, argv: &[String]) -> Result<()> {
        if self.started {
            bail!("Spawn runs once per tape");
        }
        let argv: Vec<String> = self.cfg.wrap.iter().chain(argv).cloned().collect();
        let mut spec = Spec::new(argv[0].clone(), argv[1..].to_vec());
        spec.cols = self.cfg.cols;
        spec.rows = self.cfg.rows;
        spec.cwd = self.cfg.cwd.clone();
        spec.env = self.cfg.env.clone();
        spec.clean_env = self.cfg.clean_env;
        self.eng.spawn(&spec)?;
        self.started = true;
        self.t0 = Instant::now();
        self.next_sample = Instant::now();
        Ok(())
    }

    fn input(&mut self, what: String) {
        let t = self.now();
        self.inputs.push(Input { t, what });
    }

    fn wait(&mut self, pat: &Pattern, gone: bool, timeout: Option<f64>) -> Result<()> {
        self.need()?;
        let secs = timeout.unwrap_or(self.cfg.wait_timeout);
        let end = Instant::now() + Duration::from_secs_f64(secs);
        loop {
            let s = match self.tick(false)? {
                Some(s) => s,
                None => self.read()?,
            };
            if pat.matches(&s) != gone {
                return Ok(());
            }
            if let Some(code) = self.eng.exit_code()? {
                let dump = self.opts.out_dir.join("failed");
                return Err(anina_screen::lesson::wait_failed(&s, pat, gone, secs, Some(&dump))
                    .context(format!("the program exited with code {code} while waiting")));
            }
            if Instant::now() >= end {
                let dump = self.opts.out_dir.join("failed");
                return Err(anina_screen::lesson::wait_failed(&s, pat, gone, secs, Some(&dump)));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn idle(&mut self, timeout: Option<f64>) -> Result<()> {
        self.need()?;
        let end = Instant::now() + Duration::from_secs_f64(timeout.unwrap_or(self.cfg.wait_timeout));
        let quiet = Duration::from_millis(600);
        let mut last = self.read()?.hash(None);
        let mut since = Instant::now();
        while Instant::now() < end {
            self.pump(0.03)?;
            let h = self.read()?.hash(None);
            if h != last {
                last = h;
                since = Instant::now();
            } else if since.elapsed() >= quiet {
                return Ok(());
            }
        }
        self.warnings.push("Idle: the screen never settled (an animation?); went on".into());
        Ok(())
    }

    fn set(&mut self, name: &str, vals: &[String], forced: bool) -> Result<()> {
        if !forced && self.opts.force.iter().any(|(n, _)| n == name && n != "env") {
            return Ok(());
        }
        let v0 = || vals.first().cloned().ok_or_else(|| anyhow!("Set {name} needs a value"));
        let secs = |s: String| anina_runner::script::duration(&s);
        match name {
            "cols" | "columns" => self.cfg.cols = v0()?.parse()?,
            "rows" => self.cfg.rows = v0()?.parse()?,
            "fps" | "framerate" => self.cfg.fps = v0()?.parse()?,
            "typingspeed" => self.cfg.typing = secs(v0()?)?,
            "keydelay" => self.cfg.key_delay = secs(v0()?)?,
            "waittimeout" => self.cfg.wait_timeout = secs(v0()?)?,
            "endhold" => self.cfg.end_hold = secs(v0()?)?,
            "title" => self.cfg.title = vals.join(" "),
            "theme" => self.cfg.theme = vals.join(" "),
            "cwd" => self.cfg.cwd = Some(PathBuf::from(v0()?)),
            "env" => {
                for kv in vals {
                    let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("Set Env takes K=V, got {kv:?}"))?;
                    // a forced variable (audit's TERM, COLORTERM, NO_COLOR...) wins over the tape's
                    let is_forced = |(n, vs): &(String, Vec<String>)| {
                        n == "env" && vs.iter().any(|f| f.split_once('=').is_some_and(|(fk, _)| fk == k))
                    };
                    if !forced && self.opts.force.iter().any(is_forced) {
                        continue;
                    }
                    self.cfg.env.retain(|(n, _)| n != k);
                    self.cfg.env.push((k.into(), v.into()));
                }
            }
            "cleanenv" => {
                self.cfg.clean_env = matches!(v0()?.to_ascii_lowercase().as_str(), "on" | "true" | "1" | "yes")
            }
            "shell" => self.cfg.shell = anina_runner::script::split(&vals.join(" "))?,
            "playbackspeed" => self.speed = v0()?.parse()?,
            "engine" => {} // chosen before the run
            other => self.warnings.push(format!("Set {other} is not a setting anina knows; ignored")),
        }
        Ok(())
    }

    fn step(&mut self, step: &Step) -> Result<()> {
        match step {
            Step::Set(name, vals) => self.set(name, vals, false)?,
            Step::Exec(cmd) => {
                // -u: a variable nobody set fails by name instead of being ""
                let st =
                    std::process::Command::new("sh").arg("-u").arg("-c").arg(cmd).status().context("running sh")?;
                if !st.success() {
                    bail!("Exec failed ({st}): {cmd}");
                }
            }
            Step::Spawn(argv) => self.spawn(argv)?,
            Step::Type { text, delay } => {
                self.need()?;
                let d = delay.unwrap_or(self.cfg.typing);
                self.input(format!("type {text:?}"));
                for ch in text.chars() {
                    self.eng.write(&ch.to_string())?;
                    self.pump(d)?;
                }
            }
            Step::Key { keys, delay } => {
                self.need()?;
                let d = delay.unwrap_or(self.cfg.key_delay);
                for (k, n) in keys {
                    for _ in 0..*n {
                        self.input(format!("key {k}"));
                        self.eng.key(k)?;
                        self.pump(d)?;
                    }
                }
            }
            Step::Wait { pat, gone, timeout } => self.wait(pat, *gone, *timeout)?,
            Step::Idle { timeout } => self.idle(*timeout)?,
            Step::Sleep(s) => {
                self.need()?;
                self.pump(*s)?
            }
            Step::Hold(s) => {
                self.tick(true)?;
                self.offset += s;
            }
            Step::Caption(c) => {
                self.caption = c.clone();
                self.tick(true)?;
            }
            Step::Speed(x) => {
                self.speed = *x;
                self.tick(true)?;
            }
            Step::Hide => {
                self.tick(true)?;
                self.recording = false;
                self.hidden_at = Some(Instant::now());
            }
            Step::Show => {
                if let Some(at) = self.hidden_at.take() {
                    self.offset -= at.elapsed().as_secs_f64();
                }
                self.recording = true;
                self.tick(true)?;
            }
            Step::Snap(name) => {
                self.need()?;
                let s = self.tick(true)?.unwrap_or(self.read()?);
                if let Some(name) = name {
                    self.save_shot(name, &s)?;
                    self.shots.push(Shot { name: name.clone(), screen: s });
                }
            }
            Step::Poster => {
                self.tick(true)?;
                self.poster = Some(self.frames.len().saturating_sub(1));
            }
            Step::When { pat, then } => {
                self.need()?;
                let s = self.read()?;
                if pat.matches(&s) {
                    self.step(then)?;
                }
            }
            Step::Resize(c, r) => {
                self.need()?;
                self.eng.resize(*c, *r)?;
                self.cfg.cols = *c;
                self.cfg.rows = *r;
                self.tick(true)?;
            }
            Step::Require(p) => {
                if which(p).is_none() {
                    bail!("this tape needs {p:?} on PATH");
                }
            }
            Step::Output(_) => {}
        }
        Ok(())
    }

    fn save_shot(&mut self, name: &str, s: &Screen) -> Result<()> {
        let dir = self.opts.out_dir.join("shots");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{name}.txt")), s.text())?;
        if self.opts.png {
            let title =
                if self.cfg.title.is_empty() { name.to_string() } else { format!("{} · {name}", self.cfg.title) };
            if let Err(e) =
                anina_render::pixel::screen_png(s, &title, &self.cfg.theme, &dir.join(format!("{name}.png")))
            {
                self.warnings.push(format!("Snap {name}: no png ({e})"));
            }
        }
        Ok(())
    }
}

/// Find a program on PATH.
pub fn which(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Path::new(program).exists().then(|| PathBuf::from(program));
    }
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(program)).find(|f| f.is_file()))
}

/// Run `tape` and return what was recorded. The program is closed at the end.
pub fn record(tape: &Tape, base: Config, opts: Options) -> Result<Recording> {
    let eng = engine::by_name(&opts.engine)?;
    let mut r = Rec {
        eng,
        cfg: base,
        opts,
        started: false,
        t0: Instant::now(),
        offset: 0.0,
        hidden_at: None,
        recording: true,
        caption: String::new(),
        speed: 1.0,
        poster: None,
        next_sample: Instant::now(),
        last_key: None,
        screens: Vec::new(),
        index: HashMap::new(),
        frames: Vec::new(),
        shots: Vec::new(),
        inputs: Vec::new(),
        first_screen: None,
        warnings: tape.warnings.clone(),
        last_screen: None,
    };
    for (name, vals) in r.opts.force.clone() {
        r.set(&name, &vals, true)?;
    }
    let mut result = Ok(());
    for line in &tape.lines {
        if let Err(e) = r.step(&line.step) {
            result = Err(e.context(format!("{}:{}", line.file.display(), line.n)));
            break;
        }
    }
    let mut exit_code = None;
    let mut end_modes = None;
    if r.started {
        let _ = r.tick(true);
        // give an exiting program a moment to restore the terminal
        for _ in 0..20 {
            exit_code = r.eng.exit_code().unwrap_or(None);
            if exit_code.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        end_modes = r.eng.modes().ok();
    }
    let _ = r.eng.close();
    result?;
    let cast = Cast {
        version: 2,
        title: if r.cfg.title.is_empty() {
            tape.path.file_stem().map_or("anina".into(), |s| s.to_string_lossy().into_owned())
        } else {
            r.cfg.title.clone()
        },
        theme: r.cfg.theme.clone(),
        cols: r.cfg.cols,
        rows: r.cfg.rows,
        fps: r.cfg.fps,
        end_hold: r.cfg.end_hold,
        poster: r.poster.unwrap_or(0),
        tape: {
            let p = tape.path.display().to_string();
            r.opts.redactor.as_ref().map_or(p.clone(), |red| red.text(&p))
        },
        recorded: now_iso(),
        engine: r.eng.name().into(),
        screens: r.screens,
        frames: r.frames,
    };
    Ok(Recording {
        cast,
        shots: r.shots,
        inputs: r.inputs,
        first_screen: r.first_screen,
        exit_code,
        end_modes,
        warnings: r.warnings,
    })
}

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The tape to play (anina or vhs dialect)
    pub tape: PathBuf,
    /// Output directory (default: anina-out/<tape name>)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Terminal engine: tui-test (default) or tmux
    #[arg(long, default_value = "tui-test")]
    pub engine: String,
    /// Theme for the SVG (default: the tape's, else derived from the app)
    #[arg(long)]
    pub theme: Option<String>,
    /// Only record the cast, render nothing
    #[arg(long)]
    pub no_render: bool,
    /// Variables for the tape, K=V (also read from the environment)
    #[arg(long = "var", value_name = "K=V")]
    pub vars: Vec<String>,
}

pub fn run(a: Args) -> Result<()> {
    let name = a.tape.file_stem().map_or("tape".into(), |s| s.to_string_lossy().into_owned());
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("anina-out").join(&name));
    std::fs::create_dir_all(&out)?;
    let mut vars = anina_runner::script::base_vars(&a.tape, &out);
    for kv in &a.vars {
        let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("--var takes K=V"))?;
        vars.insert(k.into(), v.into());
    }
    let tape = Tape::load(&a.tape, &vars)?;
    for w in &tape.warnings {
        eprintln!("anina: warning: {w}");
    }
    let opts = Options {
        engine: a.engine.clone(),
        out_dir: out.clone(),
        redactor: Some(Redactor::defaults()),
        png: true,
        ..Default::default()
    };
    let rec = record(&tape, Config::default(), opts)?;
    for w in rec.warnings.iter().skip(tape.warnings.len()) {
        eprintln!("anina: warning: {w}");
    }
    let cast_path = out.join(format!("{name}.cast.json.gz"));
    rec.cast.save(&cast_path)?;
    eprintln!("anina: {} frames, {} screens -> {}", rec.cast.frames.len(), rec.cast.screens.len(), cast_path.display());
    if let Some(r) = anina_runner::redact::summary() {
        eprintln!("anina: redacted {r}");
    }
    for o in tape.outputs() {
        let p = out.join(Path::new(&o).file_name().unwrap_or_default());
        if o.ends_with(".cast") {
            std::fs::write(&p, rec.cast.to_asciicast())?;
            eprintln!("anina: asciicast -> {}", p.display());
        }
    }
    if !a.no_render {
        let svg = out.join(format!("{name}.svg"));
        anina_render::shape::write_cast_svg(&rec.cast, a.theme.as_deref(), &svg)?;
        eprintln!("anina: svg -> {}", svg.display());
    }
    Ok(())
}
