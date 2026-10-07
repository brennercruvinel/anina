//! What explore learns about a program: its screens (states) and the keys
//! between them (edges), and the ways to write it down: graph.json, a
//! Mermaid map, screenshots, and a tape that replays the way to a screen.

use super::hints::{self, Hint};
use anina_screen::{Mask, Screen};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct State {
    pub id: usize,
    pub depth: usize,
    /// Keys from the start screen, as first found.
    pub path: Vec<String>,
    pub title: String,
    pub text: String,
    pub hints: Vec<Hint>,
    /// Cells that change on their own on this screen.
    pub volatile_cells: usize,
    /// Screens with the same key bar share a kind (same view, other data).
    pub kind: String,
    pub visits: usize,
    #[serde(skip)]
    pub screen: Screen,
    #[serde(skip)]
    pub mask: Mask,
}

#[derive(Clone, Debug, Serialize)]
pub struct Edge {
    pub from: usize,
    /// `None` when the key ended the program.
    pub to: Option<usize>,
    pub key: String,
    pub label: String,
    /// From the key press to the first change on screen.
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Skipped {
    pub from: usize,
    pub key: String,
    pub label: String,
    pub reason: String,
}

/// How the program was started, kept so a tape can start it the same way.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub clean_env: bool,
    pub cols: u16,
    pub rows: u16,
    /// Run before every start (`--before`).
    pub before: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Graph {
    pub launch: Launch,
    pub seconds: f64,
    pub restarts: usize,
    /// False when a budget stopped the crawl before every key was tried.
    pub complete: bool,
    pub states: Vec<State>,
    pub edges: Vec<Edge>,
    /// Keys pressed that changed nothing, per state.
    pub noops: Vec<(usize, String)>,
    pub skipped: Vec<Skipped>,
    /// Why the crawl stopped early, if it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A short name for a screen: its first line of real text, volatile and
/// drawing characters left out.
pub fn title(screen: &Screen, mask: &Mask) -> String {
    let lines = text_lines(screen, mask);
    let pick = lines.iter().find(|(_, dirty)| !dirty).or(lines.first());
    pick.map_or_else(|| "(blank)".into(), |(l, _)| truncate(l, 48))
}

/// Name each screen by its first line of real text that most other screens
/// do not share (a header every screen repeats names nothing), lines clear
/// of volatile cells first.
pub fn retitle(g: &mut Graph) {
    if g.states.len() < 3 {
        return;
    }
    let lines: Vec<Vec<(String, bool)>> = g.states.iter().map(|s| text_lines(&s.screen, &s.mask)).collect();
    let n = g.states.len();
    let common = |l: &str| lines.iter().filter(|other| other.iter().any(|(o, _)| o == l)).count() * 2 > n;
    for (i, st) in g.states.iter_mut().enumerate() {
        let mine = &lines[i];
        let pick = mine
            .iter()
            .find(|(l, dirty)| !dirty && !common(l))
            .or_else(|| mine.iter().find(|(_, dirty)| !dirty))
            .or_else(|| mine.iter().find(|(l, _)| !common(l)));
        if let Some((l, _)) = pick {
            st.title = truncate(l, 48);
        }
    }
    // screens that still share a title get the first clean line that the
    // others with the same title lack ("anina no corpus open — pick a file")
    let titles: Vec<String> = g.states.iter().map(|s| s.title.clone()).collect();
    for i in 0..n {
        let twins: Vec<usize> = (0..n).filter(|&j| j != i && titles[j] == titles[i]).collect();
        if twins.is_empty() {
            continue;
        }
        // the stable text of each line (volatile cells blanked): on screens
        // an animation runs through, every whole line is touched by it
        let stable = |k: usize| stable_lines(&g.states[k].screen, &g.states[k].mask);
        let theirs: Vec<Vec<String>> = twins.iter().map(|&j| stable(j)).collect();
        // the most telling one: most letters, a whole word at least (an
        // animation leaves fragments like "singl -" behind)
        let letters = |l: &String| l.chars().filter(|c| c.is_alphabetic()).count();
        let has_word = |l: &String| l.split_whitespace().any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 4);
        let telling = |l: &String| letters(l) >= 6 && has_word(l) && !titles[i].contains(l.as_str());
        let distinct = stable(i)
            .into_iter()
            .filter(|l| telling(l) && theirs.iter().all(|t| !t.contains(l)))
            .max_by_key(letters)
            .or_else(|| {
                // a mask grown over most of the screen leaves no stable text:
                // fall back to raw segments that no twin shows anywhere
                let flat: Vec<String> = twins.iter().map(|&j| g.states[j].screen.text_flat()).collect();
                let none = Mask::new(0, 0);
                stable_lines(&g.states[i].screen, &none)
                    .into_iter()
                    .filter(|l| telling(l) && flat.iter().all(|t| !t.contains(l.as_str())))
                    .max_by_key(letters)
            });
        if let Some(l) = distinct {
            g.states[i].title = truncate(&format!("{} · {}", titles[i], l), 64);
        }
    }
}

/// Like `text_lines`, with volatile cells blanked out first: what of each
/// line stays put, as its longest run of words.
fn stable_lines(screen: &Screen, mask: &Mask) -> Vec<String> {
    let mut out = Vec::new();
    for y in 0..screen.rows {
        let mut row = String::new();
        for x in 0..screen.cols {
            let c = screen.cell(x, y);
            if c.is_spacer() {
                continue;
            }
            let ch = c.ch.chars().next().unwrap_or(' ');
            if mask.get(x, y) || is_drawing(ch) {
                row.push(' ');
            } else {
                row.push_str(&c.ch);
            }
        }
        let best = row
            .split("   ")
            .map(|seg| seg.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|seg| wordy(seg) && hints::parse_row(seg).is_empty())
            .max_by_key(|seg| seg.chars().count());
        if let Some(seg) = best {
            out.push(seg);
        }
    }
    out
}

/// The text lines of a screen worth naming it by: mostly letters (meters
/// and rulers name nothing), no key bars, drawing removed. Each comes with
/// whether volatile cells touch it.
fn text_lines(screen: &Screen, mask: &Mask) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for y in 0..screen.rows {
        let mut row = String::new();
        let mut touched = false;
        for x in 0..screen.cols {
            let c = screen.cell(x, y);
            if c.is_spacer() {
                continue;
            }
            let ch = c.ch.chars().next().unwrap_or(' ');
            if mask.get(x, y) && !ch.is_whitespace() && !is_drawing(ch) {
                touched = true;
            }
            if is_drawing(ch) {
                row.push(' ');
            } else {
                row.push_str(&c.ch);
            }
        }
        let line = row.split_whitespace().collect::<Vec<_>>().join(" ");
        if wordy(&line) && hints::parse_row(&line).is_empty() {
            out.push((line, touched));
        }
    }
    out
}

/// At least 3 letters and 40% of the visible characters.
fn wordy(s: &str) -> bool {
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    let visible = s.chars().filter(|c| !c.is_whitespace()).count();
    letters >= 3 && letters * 5 >= visible * 2
}

fn is_drawing(c: char) -> bool {
    ('\u{2500}'..='\u{259f}').contains(&c) || ('\u{2800}'..='\u{28ff}').contains(&c)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end())
    }
}

/// The kind of a screen: its sorted hint keys, so the corpus tab with two
/// different files open is one kind.
pub fn kind(hints: &[Hint]) -> String {
    let mut keys: Vec<&str> = hints.iter().map(|h| h.key.as_str()).collect();
    keys.sort_unstable();
    if keys.is_empty() {
        return "-".into();
    }
    let mut h: u32 = 0x811c9dc5;
    for b in keys.join(" ").bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("k{:06x}", h & 0xff_ffff)
}

impl Graph {
    /// The shortest key sequence between two states over known edges.
    pub fn path(&self, from: usize, to: usize) -> Option<Vec<usize>> {
        if from == to {
            return Some(vec![]);
        }
        let mut prev: HashMap<usize, usize> = HashMap::new(); // state -> edge that reached it
        let mut seen = HashSet::from([from]);
        let mut q = VecDeque::from([from]);
        while let Some(s) = q.pop_front() {
            for (i, e) in self.edges.iter().enumerate().filter(|(_, e)| e.from == s) {
                let Some(t) = e.to else { continue };
                if seen.insert(t) {
                    prev.insert(t, i);
                    if t == to {
                        let mut out = vec![i];
                        let mut cur = s;
                        while cur != from {
                            let ei = prev[&cur];
                            out.push(ei);
                            cur = self.edges[ei].from;
                        }
                        out.reverse();
                        return Some(out);
                    }
                    q.push_back(t);
                }
            }
        }
        None
    }

    /// The first state (nearest the start) whose screen shows `text`,
    /// preferring screens where it is content, not a key bar label.
    pub fn find(&self, text: &str) -> Option<usize> {
        let shows = |s: &State, body_only: bool| {
            let lines: Vec<String> = s
                .text
                .lines()
                .filter(|l| !body_only || hints::parse_row(l).is_empty())
                .map(|l| l.to_string())
                .collect();
            let joined = lines.join("\n");
            joined.contains(text) || flat(&joined).contains(text)
        };
        for body_only in [true, false] {
            let mut by_depth: Vec<&State> = self.states.iter().filter(|s| shows(s, body_only)).collect();
            by_depth.sort_by_key(|s| (s.depth, s.id));
            if let Some(s) = by_depth.first() {
                return Some(s.id);
            }
        }
        None
    }

    /// Text on `id`'s screen worth waiting for: not volatile, not on the
    /// previous screen, unique to this state when possible.
    pub fn distinctive(&self, id: usize, prev: Option<usize>) -> Option<String> {
        let st = &self.states[id];
        let prev_text = prev.map(|p| flat(&self.states[p].text));
        // (unique to this state, not a key bar, length, text)
        let mut cands: Vec<(bool, bool, usize, String)> = Vec::new();
        for y in 0..st.screen.rows {
            // segments: runs separated by 2+ blank cells or drawing; a
            // segment touching a volatile cell is no good to wait for
            let mut segs: Vec<(String, bool)> = vec![(String::new(), false)];
            let mut blanks = 0;
            for x in 0..st.screen.cols {
                let c = st.screen.cell(x, y);
                if c.is_spacer() {
                    continue;
                }
                let ch = c.ch.chars().next().unwrap_or(' ');
                let boundary = is_drawing(ch) || matches!(ch, '│' | '┃' | '║');
                if ch == ' ' && !st.mask.get(x, y) {
                    blanks += 1;
                    if blanks >= 2 && !segs.last().is_some_and(|s| s.0.trim().is_empty()) {
                        segs.push((String::new(), false));
                    } else {
                        segs.last_mut().unwrap().0.push(' ');
                    }
                    continue;
                }
                blanks = 0;
                if boundary {
                    segs.push((String::new(), false));
                    continue;
                }
                let last = segs.last_mut().unwrap();
                last.1 |= st.mask.get(x, y);
                last.0.push_str(&c.ch);
            }
            for (seg, dirty) in segs {
                let seg = seg.trim();
                // `$` would expand in a tape, a leading `~` too
                if dirty
                    || seg.chars().count() < 4
                    || !wordy(seg)
                    || seg.contains('$')
                    || seg.starts_with('~')
                    || seg.contains('`')
                {
                    continue;
                }
                let seg = truncate_plain(seg, 48);
                if prev_text.as_ref().is_some_and(|p| p.contains(&seg)) {
                    continue;
                }
                let unique = self.states.iter().filter(|o| o.id != id).all(|o| !flat(&o.text).contains(&seg));
                let prose = hints::parse_row(&seg).is_empty();
                cands.push((unique, prose, seg.chars().count(), seg));
            }
        }
        cands.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(b.2.cmp(&a.2)));
        cands.into_iter().next().map(|c| c.3)
    }

    /// A tape in anina's dialect that starts the program and walks the
    /// shortest known way to `target`, waiting on each screen.
    pub fn tape_to(&self, target: usize, want: &str) -> Option<String> {
        let edges = self.path(0, target)?;
        let l = &self.launch;
        let mut t = String::new();
        let _ = writeln!(t, "# written by `anina explore --to {}`: the shortest way from the start", quote(want));
        let _ = writeln!(
            t,
            "# to the first screen showing it ({} key{}).",
            edges.len(),
            if edges.len() == 1 { "" } else { "s" }
        );
        let _ = writeln!(t, "Set Cols {}\nSet Rows {}", l.cols, l.rows);
        if let Some(cwd) = &l.cwd {
            let _ = writeln!(t, "Set Cwd {}", quote(&cwd.display().to_string()));
        }
        if l.clean_env {
            let _ = writeln!(t, "Set CleanEnv on");
        }
        for (k, v) in &l.env {
            let _ = writeln!(t, "Set Env {}", quote(&format!("{k}={v}")));
        }
        if let Some(cmd) = &l.before {
            let _ = writeln!(t, "Exec {cmd}");
        }
        let mut spawn = vec![quote(&l.program)];
        spawn.extend(l.args.iter().map(|a| quote(a)));
        let _ = writeln!(t, "Spawn {}", spawn.join(" "));
        match self.distinctive(0, None) {
            Some(w) => {
                let _ = writeln!(t, "Wait {}", quote(&w));
            }
            None => {
                let _ = writeln!(t, "Idle 10");
            }
        }
        for ei in edges {
            let e = &self.edges[ei];
            if let Some(text) = e.key.strip_prefix("type:") {
                let _ = writeln!(t, "Type {}", quote(text));
            } else {
                let _ = writeln!(t, "Key {}", e.key);
            }
            let to = e.to.unwrap_or(0);
            match self.distinctive(to, Some(e.from)) {
                Some(w) => {
                    let _ = writeln!(t, "Wait {}", quote(&w));
                }
                None => {
                    let _ = writeln!(t, "Idle 5 # the change is not text (a highlight moved)");
                }
            }
        }
        // the screen is reached when what was asked for shows (it may come
        // later than the screen itself: a check finishing, a list loading)
        if !want.contains('$') {
            let _ = writeln!(t, "Wait {}", quote(want));
        }
        let _ = writeln!(t, "Snap target");
        Some(t)
    }

    /// A Mermaid flowchart plus a table of the screens.
    pub fn map_md(&self) -> String {
        let mut m = String::new();
        let l = &self.launch;
        let cmd =
            std::iter::once(l.program.as_str()).chain(l.args.iter().map(|s| s.as_str())).collect::<Vec<_>>().join(" ");
        // the program's path may sit under the user's home
        let cmd = anina_runner::redact::Redactor::shared().text(&cmd);
        let _ = writeln!(m, "# Map of `{cmd}`\n");
        let exits = self.edges.iter().filter(|e| e.to.is_none()).count();
        let _ = writeln!(
            m,
            "{} screens, {} transitions ({} change the screen they are on), {} ways out, {} keys skipped, {} that did nothing; {:.0} s, {} restarts{}.\n",
            self.states.len(),
            self.edges.iter().filter(|e| e.to.is_some()).count(),
            self.edges.iter().filter(|e| e.to == Some(e.from)).count(),
            exits,
            self.skipped.len(),
            self.noops.len(),
            self.seconds,
            self.restarts,
            if self.complete { "" } else { " (stopped by a budget: not every key was tried)" }
        );
        let _ = writeln!(m, "```mermaid\nflowchart LR");
        for s in &self.states {
            let _ = writeln!(m, "  s{}[\"{}\"]", s.id, mermaid(&s.title));
        }
        if exits > 0 {
            let _ = writeln!(m, "  exit((exit))");
        }
        // one arrow per (from, to) with the keys joined
        type Arrow = ((usize, Option<usize>), Vec<String>);
        let mut arrows: Vec<Arrow> = Vec::new();
        for e in &self.edges {
            let k = (e.from, e.to);
            match arrows.iter_mut().find(|(a, _)| *a == k) {
                Some((_, keys)) => {
                    if !keys.contains(&e.key) {
                        keys.push(e.key.clone())
                    }
                }
                None => arrows.push((k, vec![e.key.clone()])),
            }
        }
        for ((from, to), keys) in arrows {
            let dest = to.map_or("exit".to_string(), |t| format!("s{t}"));
            let shown =
                if keys.len() > 4 { format!("{} +{}", keys[..3].join(", "), keys.len() - 3) } else { keys.join(", ") };
            let _ = writeln!(m, "  s{from} -->|\"{}\"| {dest}", mermaid(&shown));
        }
        let _ = writeln!(m, "```\n");
        let _ = writeln!(m, "| screen | depth | title | keys on screen | way there | volatile cells |");
        let _ = writeln!(m, "| --- | --- | --- | --- | --- | --- |");
        for s in &self.states {
            let keys = s.hints.iter().map(|h| format!("`{}` {}", h.key, h.label)).collect::<Vec<_>>().join(", ");
            let path = if s.path.is_empty() {
                "start".into()
            } else {
                s.path.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(" ")
            };
            let _ = writeln!(
                m,
                "| [s{0}](screens/s{0}.txt) | {1} | {2} | {3} | {4} | {5} |",
                s.id,
                s.depth,
                table(&s.title),
                table(&keys),
                path,
                s.volatile_cells
            );
        }
        if !self.skipped.is_empty() {
            let _ = writeln!(m, "\nSkipped:\n");
            for s in &self.skipped {
                let _ = writeln!(m, "- s{} `{}` {}: {}", s.from, s.key, s.label, s.reason);
            }
        }
        m
    }

    /// graph.json, map.md and screens/ in `dir`. Returns how many PNGs were
    /// written (the renderer may be missing).
    pub fn write(&self, dir: &Path) -> anyhow::Result<usize> {
        let screens = dir.join("screens");
        std::fs::create_dir_all(&screens)?;
        // the screens were redacted as read; this catches the launch (cwd, env, args)
        std::fs::write(dir.join("graph.json"), anina_runner::redact::Redactor::shared().to_json(self)?)?;
        std::fs::write(dir.join("map.md"), self.map_md())?;
        let mut pngs = 0;
        let mut png_ok = true;
        for s in &self.states {
            std::fs::write(screens.join(format!("s{}.txt", s.id)), &s.text)?;
            std::fs::write(screens.join(format!("s{}.ansi", s.id)), s.screen.to_ansi())?;
            if png_ok {
                let title = format!("s{} · {}", s.id, s.title);
                match anina_render::pixel::screen_png(&s.screen, &title, "", &screens.join(format!("s{}.png", s.id))) {
                    Ok(()) => pngs += 1,
                    Err(_) => png_ok = false, // no renderer: stop trying
                }
            }
        }
        Ok(pngs)
    }
}

fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_plain(s: &str, n: usize) -> String {
    let s: String = s.chars().take(n).collect();
    s.trim_end().to_string()
}

/// A tape word: double-quoted, with `"` and `\` escaped.
pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn mermaid(s: &str) -> String {
    s.replace('"', "#quot;").replace('|', "#124;").replace(['[', ']', '{', '}', '<', '>'], " ")
}

fn table(s: &str) -> String {
    s.replace('|', "\\|")
}
