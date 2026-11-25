//! `anina explore`: map a program by itself. It reads the keys each screen
//! advertises (plus the keys every TUI understands), presses them one by one,
//! and records which screen each key leads to. Screens are compared with
//! their volatile cells masked, so a spinner or a clock does not make a new
//! screen. The result is a graph, a map, screenshots, and on request a tape
//! that walks the shortest way to a screen.
//!
//! Navigation is breadth first. After a key leads away, the crawler tries to
//! come back with the inverse key (up for down, shift+tab for tab, esc
//! otherwise); when that fails it restarts the program and replays the
//! shortest known path. Keys that end the program are pressed last, only to
//! record the way out; keys whose label sounds destructive (install, delete,
//! save...) are skipped unless `--allow-risky`.

pub mod graph;
pub mod hints;

use anina_record::cordon::{Mode, Sandbox};
use anina_runner::engine::{self, Engine, Spec};
use anina_runner::masker;
use anina_runner::redact::{Redacted, scope::Scope};
use anina_screen::{Mask, Screen};
use anyhow::{Result, anyhow, bail};
use graph::{Edge, Graph, Launch, Skipped, State};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod entry;
mod moves;
pub use entry::*;

/// Keys every TUI is likely to understand, tried after the advertised ones.
pub const GENERIC_KEYS: &[&str] = &["tab", "shift+tab", "down", "up", "right", "left", "enter", "esc", "?", "space"];

#[derive(Clone, Debug)]
pub struct Settings {
    pub max_states: usize,
    pub max_depth: usize,
    pub max_time: Duration,
    pub extra_keys: Vec<String>,
    pub avoid: Vec<String>,
    pub type_text: Option<String>,
    pub allow_risky: bool,
    /// A screen counts as settled after this long without a change.
    pub quiet: Duration,
    /// How long a key gets to change anything before it counts as a no-op.
    pub first_change: Duration,
    /// More time for an advertised key that seemed to do nothing.
    pub late: Duration,
    /// Give up waiting for a key's effect after this long.
    pub settle_max: Duration,
    /// Volatile learning on a new screen: (min, max, quiet) seconds.
    pub observe: (f64, f64, f64),
    pub start_timeout: Duration,
    pub verbose: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            max_states: 60,
            max_depth: 6,
            max_time: Duration::from_secs(300),
            extra_keys: Vec::new(),
            avoid: Vec::new(),
            type_text: None,
            allow_risky: false,
            quiet: Duration::from_millis(400),
            first_change: Duration::from_millis(1200),
            late: Duration::from_secs(3),
            settle_max: Duration::from_secs(5),
            // at least 1.6 s: many programs redraw once a second or slower
            observe: (1.6, 4.0, 0.6),
            start_timeout: Duration::from_secs(15),
            verbose: true,
        }
    }
}

/// Starts the program and hands back the running engine.
pub type Factory<'a> = Box<dyn FnMut() -> Result<Box<dyn Engine>> + 'a>;

struct Action {
    key: String,
    label: String,
    quit: bool,
}

pub struct Explorer<'a> {
    make: Factory<'a>,
    eng: Option<Box<dyn Engine>>,
    /// The state on screen now, when known.
    at: Option<usize>,
    g: Graph,
    set: Settings,
    t0: Instant,
    tried: HashSet<(usize, String)>,
    unreachable: HashSet<usize>,
    /// The last key that seemed to do nothing, as (state, key, label).
    last_noop: Option<(usize, String, String)>,
}

/// A settled screen and what was learned while waiting for it.
struct Settled {
    screen: Screen,
    /// From the key press to the first change on screen.
    latency: Option<u64>,
    exit: Option<i32>,
    /// Cells that kept changing while waiting: volatile on this screen.
    churn: Mask,
}

impl<'a> Explorer<'a> {
    pub fn new(make: Factory<'a>, launch: Launch, set: Settings) -> Self {
        Explorer {
            make,
            eng: None,
            at: None,
            g: Graph { launch, ..Default::default() },
            set,
            t0: Instant::now(),
            tried: HashSet::new(),
            unreachable: HashSet::new(),
            last_noop: None,
        }
    }

    fn log(&self, msg: impl AsRef<str>) {
        if self.set.verbose {
            eprintln!("anina explore: [{:5.1}s] {}", self.t0.elapsed().as_secs_f64(), msg.as_ref());
        }
    }

    fn eng(&mut self) -> Result<&mut Box<dyn Engine>> {
        self.eng.as_mut().ok_or_else(|| anyhow!("no program running"))
    }

    fn out_of_time(&self) -> bool {
        self.t0.elapsed() >= self.set.max_time
    }

    fn drop_engine(&mut self) {
        if let Some(mut e) = self.eng.take() {
            let _ = e.close();
        }
        self.at = None;
    }

    /// Start (or restart) the program and wait for its first settled screen.
    fn launch(&mut self) -> Result<Screen> {
        self.drop_engine();
        let e = (self.make)()?;
        self.eng = Some(e);
        let end = Instant::now() + self.set.start_timeout;
        loop {
            let s = self.eng()?.screen()?;
            if !s.text().trim().is_empty() {
                break;
            }
            if let Some(code) = self.eng()?.exit_code()? {
                bail!("the program exited with code {code} before drawing anything");
            }
            if Instant::now() >= end {
                bail!("the program drew nothing in {:?}", self.set.start_timeout);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let mask = self.g.states.first().map(|s| s.mask.clone()).unwrap_or_else(|| Mask::new(0, 0));
        Ok(self.settle(&mask, None)?.screen)
    }

    /// Wait for the screen to stop changing. Cells in `mask`, and cells seen
    /// changing twice or more while waiting (an animation, not the key's
    /// effect), do not count as changes.
    fn settle(&mut self, mask: &Mask, before: Option<&Screen>) -> Result<Settled> {
        let start = Instant::now();
        let mut last = self.eng()?.screen()?;
        let (cols, rows) = (last.cols, last.rows);
        let mut counts = vec![0u8; last.cells.len()];
        let mut churn = Mask::new(cols, rows);
        let masked = |i: usize, churn: &Mask| (mask.cols == cols && mask.rows == rows && mask.bits[i]) || churn.bits[i];
        let mut latency = None;
        if before.is_some_and(|b| !masker::equal_under(b, &last, mask)) {
            latency = Some(start.elapsed().as_millis() as u64);
        }
        let mut since = Instant::now();
        loop {
            if let Some(code) = self.eng()?.exit_code()? {
                return Ok(Settled { screen: last, latency, exit: Some(code), churn });
            }
            // before anything changed, give the program `first_change` to
            // react (a handoff to another program is slow); after, `quiet`
            let waited = if latency.is_none() && before.is_some() {
                start.elapsed() >= self.set.first_change
            } else {
                since.elapsed() >= self.set.quiet
            };
            if waited || start.elapsed() >= self.set.settle_max {
                // a screen gone blank is often a program on its way out
                if last.text().trim().is_empty() {
                    for _ in 0..25 {
                        if let Some(code) = self.eng()?.exit_code()? {
                            return Ok(Settled { screen: last, latency, exit: Some(code), churn });
                        }
                        std::thread::sleep(Duration::from_millis(40));
                    }
                }
                let churn = masker::widen(&churn, &last);
                return Ok(Settled { screen: last, latency, exit: None, churn });
            }
            std::thread::sleep(Duration::from_millis(20));
            let s = self.eng()?.screen()?;
            if s.cols != cols || s.rows != rows {
                last = s;
                since = Instant::now();
                continue;
            }
            let diff = last.diff(&s);
            let mut changed = false;
            for (i, d) in diff.bits.iter().enumerate() {
                if !*d || masked(i, &churn) {
                    continue;
                }
                counts[i] = counts[i].saturating_add(1);
                if counts[i] >= 2 {
                    churn.bits[i] = true;
                } else {
                    changed = true;
                }
            }
            if changed {
                since = Instant::now();
                if latency.is_none() && before.is_some_and(|b| !masker::equal_under(b, &s, mask)) {
                    latency = Some(start.elapsed().as_millis() as u64);
                }
            }
            last = s;
        }
    }

    /// The closest state of the same kind and its differing cells: up to
    /// half the cells for screens with a key bar (the bar pins the view), a
    /// tenth for screens without one.
    fn similar(&self, screen: &Screen, mask: &Mask) -> Option<(usize, Mask)> {
        let kind = graph::kind(&hints::parse(screen));
        let total = screen.cells.len().max(1);
        let limit = if kind == "-" { total / 10 } else { total / 2 };
        self.g
            .states
            .iter()
            .filter(|st| st.kind == kind && st.screen.cols == screen.cols && st.screen.rows == screen.rows)
            .map(|st| {
                let m = masker::union_of(&st.mask, mask, screen.cols, screen.rows);
                let mut d = st.screen.diff(screen);
                for (b, x) in d.bits.iter_mut().zip(&m.bits) {
                    *b &= !*x;
                }
                (st.id, d)
            })
            .filter(|(_, d)| d.count() <= limit)
            .min_by_key(|(_, d)| d.count())
    }

    /// A key left the program on the same view, changed: was that the key,
    /// or live data moving anyway? Watch a second with no input; what moves
    /// then is volatile (learned), and the key gets credit only for change
    /// beyond it.
    fn live_noise(&mut self, id: usize, before: &Screen, after: &Screen) -> Result<bool> {
        let (min, _, _) = self.set.observe;
        let watch = min.clamp(0.1, 1.0);
        let (_, idle) = masker::observe_adaptive(self.eng()?.as_mut(), watch, watch, watch)?;
        self.learn(id, &idle);
        let mask = &self.g.states[id].mask;
        if before.cols != after.cols || before.rows != after.rows {
            return Ok(false);
        }
        let left = before.diff(after).bits.iter().zip(&mask.bits).filter(|(d, m)| **d && !**m).count();
        Ok(left <= 3)
    }

    /// At most half of the cells outside `id`'s mask differ.
    fn close_to(&self, screen: &Screen, id: usize) -> bool {
        let st = &self.g.states[id];
        if st.screen.cols != screen.cols || st.screen.rows != screen.rows {
            return false;
        }
        let d = st.screen.diff(screen);
        let n = d.bits.iter().zip(&st.mask.bits).filter(|(a, m)| **a && !**m).count();
        n * 2 <= screen.cells.len()
    }

    /// Fold `extra` into a state's volatile cells, never its key bar rows:
    /// they are what tells one view from another.
    fn learn(&mut self, id: usize, extra: &Mask) {
        let st = &mut self.g.states[id];
        let (c, r) = (st.screen.cols, st.screen.rows);
        st.mask = masker::widen(&masker::union_of(&st.mask, extra, c, r), &st.screen);
        keep_hint_rows(&mut st.mask, &st.hints);
        st.volatile_cells = st.mask.count();
    }

    /// Identify a settled screen; if it looks new, learn its volatile cells
    /// first (a busy screen can look new only because of its animation).
    /// Returns the state id, or None when --max-states is reached.
    fn place(&mut self, screen: Screen, hint_mask: &Mask, depth: usize, path: &[String]) -> Result<Option<usize>> {
        if let Some(id) = self.match_and_learn(&screen, hint_mask) {
            return Ok(Some(id));
        }
        let (min, max, quiet) = self.set.observe;
        let (screen, learned) = masker::observe_adaptive(self.eng()?.as_mut(), min, max, quiet)?;
        let mask = masker::union_of(&learned, hint_mask, screen.cols, screen.rows);
        let mask = masker::widen(&mask, &screen);
        if let Some(id) = self.match_and_learn(&screen, &mask) {
            return Ok(Some(id));
        }
        if self.g.states.len() >= self.set.max_states {
            return Ok(None);
        }
        let id = self.g.states.len();
        let st = new_state(id, depth, path, screen, mask);
        self.log(format!("+ s{id} depth {depth} via {}: {}", path.last().map_or("start", |k| k.as_str()), st.title));
        self.g.states.push(st);
        Ok(Some(id))
    }

    /// The known state `screen` is: equal outside both masks, or else the
    /// same view (same key bar) with at most half its cells changed, a busy
    /// screen (a process list, a live plot) showing new data.
    fn match_and_learn(&mut self, screen: &Screen, mask: &Mask) -> Option<usize> {
        let exact = self.g.states.iter().position(|st| {
            let m = masker::union_of(&st.mask, mask, screen.cols, screen.rows);
            masker::equal_under(screen, &st.screen, &m)
        });
        // a fuzzy match identifies, it does not teach the mask: folding its
        // differences in would let one view swallow the next
        let id = match exact {
            Some(id) => id,
            None => self.similar(screen, mask)?.0,
        };
        self.g.states[id].visits += 1;
        Some(id)
    }

    fn press(&mut self, key: &str) -> Result<()> {
        match key.strip_prefix("type:") {
            Some(text) => self.eng()?.write(text),
            None => self.eng()?.key(key),
        }
    }

    /// The keys to try on a state, advertised ones first, quitting last.
    fn actions(&mut self, id: usize) -> Vec<Action> {
        let st = &self.g.states[id];
        let mut seen: HashSet<String> = HashSet::new();
        let mut normal = Vec::new();
        let mut quits = Vec::new();
        let mut skipped = Vec::new();
        let advertised = st.hints.iter().map(|h| (h.key.clone(), h.label.clone()));
        let generic: Vec<(String, String)> = GENERIC_KEYS
            .iter()
            .map(|k| k.to_string())
            .chain(self.set.extra_keys.iter().cloned())
            .map(|k| (engine::normalize_key(&k), String::new()))
            .collect();
        let mut typed = Vec::new();
        if let Some(t) = &self.set.type_text
            && st.screen.cursor.visible
        {
            typed.push((format!("type:{t}"), "type".to_string()));
        }
        for (key, label) in advertised.chain(generic).chain(typed) {
            if !seen.insert(key.clone()) {
                continue;
            }
            if self.set.avoid.iter().any(|a| engine::normalize_key(a) == key) {
                skipped.push(Skipped { from: id, key, label, reason: "--avoid".into() });
                continue;
            }
            if !self.set.allow_risky && hints::is_risky(&label) {
                skipped.push(Skipped {
                    from: id,
                    key,
                    label,
                    reason: "sounds destructive (--allow-risky to press it)".into(),
                });
                continue;
            }
            let quit = hints::is_quit(&key, &label);
            let a = Action { key, label, quit };
            if a.quit { quits.push(a) } else { normal.push(a) }
        }
        self.g.skipped.extend(skipped);
        normal.extend(quits);
        normal
    }

    /// Run the crawl and hand back the graph. Only a failed first start is
    /// an error: anything later stops the crawl and is reported in the graph.
    pub fn run(mut self) -> Result<Graph> {
        self.t0 = Instant::now();
        self.launch()?;
        let (min, max, quiet) = self.set.observe;
        let (screen, mask) = masker::observe_adaptive(self.eng()?.as_mut(), min, max, quiet)?;
        let root = new_state(0, 0, &[], screen, mask);
        self.log(format!("+ s0 start: {}", root.title));
        self.g.states.push(root);
        self.at = Some(0);
        let mut queue = VecDeque::from([0usize]);
        let result = self.crawl(&mut queue);
        let mut complete = matches!(result, Ok(true));
        if let Err(e) = result {
            self.log(format!("stopped: {e:#}"));
            self.g.error = Some(format!("{e:#}"));
        }
        if !queue.is_empty() || self.g.skipped.iter().any(|s| s.reason.starts_with("--max-states")) {
            complete = false;
        }
        self.drop_engine();
        graph::retitle(&mut self.g);
        self.g.complete = complete;
        self.g.seconds = self.t0.elapsed().as_secs_f64();
        Ok(self.g)
    }

    /// Breadth first over `queue`. Ok(false) when a budget cut it short.
    fn crawl(&mut self, queue: &mut VecDeque<usize>) -> Result<bool> {
        let mut complete = true;
        'states: while let Some(id) = queue.pop_front() {
            if self.g.states[id].depth >= self.set.max_depth || self.unreachable.contains(&id) {
                continue;
            }
            for a in self.actions(id) {
                if self.out_of_time() {
                    queue.push_front(id);
                    return Ok(false);
                }
                if self.tried.contains(&(id, a.key.clone())) {
                    continue;
                }
                // get there, and make sure no slow effect is still landing
                let mut reached = false;
                for _ in 0..3 {
                    reached = match self.goto(id).and_then(|r| if r { self.check_drift(id) } else { Ok(false) }) {
                        Ok(r) => r,
                        Err(e) => {
                            self.log(format!("  s{id}: {e:#}"));
                            self.drop_engine();
                            false
                        }
                    };
                    if reached {
                        break;
                    }
                }
                if !reached {
                    self.log(format!("  s{id} can not be reached again; leaving it"));
                    self.unreachable.insert(id);
                    complete = false;
                    continue 'states;
                }
                let before_len = self.g.states.len();
                let to = self.try_key(id, &a.key, &a.label)?;
                if std::env::var_os("ANINA_TRACE").is_some() {
                    self.log(format!("  s{id} {} -> {to:?}", a.key));
                }
                if self.g.states.len() > before_len {
                    queue.push_back(before_len);
                }
                // come back cheaply: the inverse key, recorded as an edge too
                if let Some(to) = to.filter(|t| *t != id && !a.quit) {
                    let back = inverse(&a.key);
                    if !self.tried.contains(&(to, back.to_string()))
                        && !self.set.avoid.iter().any(|k| engine::normalize_key(k) == back)
                    {
                        let label = self.g.states[to]
                            .hints
                            .iter()
                            .find(|h| h.key == back)
                            .map(|h| h.label.clone())
                            .unwrap_or_default();
                        if self.allowed(&label) && !hints::is_quit(back, &label) {
                            let before_len = self.g.states.len();
                            self.try_key(to, back, &label)?;
                            if self.g.states.len() > before_len {
                                queue.push_back(before_len);
                            }
                        }
                    }
                }
            }
        }
        Ok(complete)
    }

    fn allowed(&self, label: &str) -> bool {
        self.set.allow_risky || !hints::is_risky(label)
    }
}

/// Unmask the rows that carry key hints.
fn keep_hint_rows(mask: &mut Mask, hints: &[hints::Hint]) {
    for h in hints {
        if h.row < mask.rows {
            for x in 0..mask.cols {
                mask.bits[h.row as usize * mask.cols as usize + x as usize] = false;
            }
        }
    }
}

fn new_state(id: usize, depth: usize, path: &[String], screen: Screen, mut mask: Mask) -> State {
    let hints = hints::parse(&screen);
    keep_hint_rows(&mut mask, &hints);
    State {
        id,
        depth,
        path: path.to_vec(),
        title: graph::title(&screen, &mask),
        text: screen.text(),
        kind: graph::kind(&hints),
        hints,
        volatile_cells: mask.count(),
        visits: 1,
        screen,
        mask,
    }
}

/// The key that most likely undoes `key`.
pub fn inverse(key: &str) -> &'static str {
    match key {
        "down" => "up",
        "up" => "down",
        "right" => "left",
        "left" => "right",
        "tab" => "shift+tab",
        "shift+tab" => "tab",
        "pagedown" => "pageup",
        "pageup" => "pagedown",
        "home" => "end",
        "end" => "home",
        _ => "esc",
    }
}
