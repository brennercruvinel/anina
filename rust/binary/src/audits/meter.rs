//! What one run of the journey in one terminal tells: did it start, crash or
//! break, how fast it answered, how readable it was, whether it respected
//! the terminal's capabilities, and what state it left the terminal in.

use super::cases::{Colors, Variant};
use super::ratio::{self, Stats};
use anina_record::{Input, Recording};
use anina_runner::engine::Modes;
use anina_screen::frames::Frame;
use anina_screen::{Cell, Color, Screen};
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    /// The whole journey ran.
    Ok,
    /// A step of the journey could not happen (a wait timed out): the
    /// screen did not show what the tape expected in this terminal.
    Broke { error: String },
    /// The program died with an error or printed a crash.
    Crashed { reason: String },
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Broke { .. } => "broke",
            Status::Crashed { .. } => "crashed",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Latency {
    /// Inputs that changed the screen, and how long that took (ms).
    pub answered: usize,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Inputs after which the screen did not change before the next one.
    pub unanswered: Vec<String>,
    /// The sampling interval: no latency below it can be told apart.
    pub resolution_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub variant: Variant,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub first_screen_s: Option<f64>,
    pub duration_s: f64,
    pub frames: usize,
    pub screens: usize,
    pub latency: Latency,
    pub contrast: Stats,
    /// Distinct visible cells (position and look) with a color other than the default.
    pub colored_cells: usize,
    pub truecolor_cells: usize,
    /// Cells using a color past the first 16.
    pub beyond16_cells: usize,
    pub visible_cells: usize,
    pub says_too_small: bool,
    pub used_rows: u16,
    pub used_cols: u16,
    pub crash_text: Option<String>,
    pub error_lines: Vec<String>,
    pub exited: bool,
    pub hygiene: Vec<Check>,
    /// The most telling screen of the run (the fullest one, or the failure).
    #[serde(skip)]
    pub shot: Option<Screen>,
    /// Its PNG, relative to the report, when one could be drawn.
    pub shot_png: Option<String>,
}

static CRASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(panicked at|Traceback \(most recent call last\)|Segmentation fault|core dumped|stack backtrace:|fatal error:|Unhandled exception|uncaught exception|RUST_BACKTRACE=|SIGSEGV|SIGABRT)").unwrap()
});
static ERROR_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*(?:error|Error|ERROR)(?:\[[A-Z0-9]+\])?:.*$").unwrap());
static TOO_SMALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(too small|terminal (?:size|window|is) (?:is )?too|minimum (?:terminal |window )?size|(?:resize|enlarge|widen) (?:your |the )?(?:terminal|window)|needs? at least \d+ ?(?:x|×|by) ?\d+)").unwrap()
});

/// Latency from a recording's inputs to the first frame that differs.
pub fn latency(frames: &[Frame], inputs: &[Input], fps: f64) -> Latency {
    let mut ms = Vec::new();
    let mut unanswered = Vec::new();
    for (i, inp) in inputs.iter().enumerate() {
        let before = frames.iter().rev().find(|f| f.t < inp.t).map(|f| f.s);
        let until = inputs.get(i + 1).map_or(inp.t + 5.0, |n| n.t.min(inp.t + 5.0));
        let answer = frames.iter().find(|f| f.t >= inp.t && f.t <= until && Some(f.s) != before);
        match answer {
            Some(f) => ms.push(((f.t - inp.t).max(0.0) * 1000.0 * 10.0).round() / 10.0),
            None => unanswered.push(inp.what.clone()),
        }
    }
    ms.sort_by(f64::total_cmp);
    let pct = |p: f64| -> Option<f64> {
        if ms.is_empty() {
            return None;
        }
        // nearest rank
        let rank = ((p * ms.len() as f64).ceil() as usize).clamp(1, ms.len());
        Some(ms[rank - 1])
    };
    Latency {
        answered: ms.len(),
        p50_ms: pct(0.50),
        p95_ms: pct(0.95),
        max_ms: ms.last().copied(),
        unanswered,
        resolution_ms: if fps > 0.0 { (1000.0 / fps * 10.0).round() / 10.0 } else { 0.0 },
    }
}

/// The state a well-behaved program leaves the terminal in when it exits.
const HYGIENE: &[(&[&str], bool, &str)] = &[
    (&["alternate_screen", "alt_screen"], false, "left the alternate screen"),
    (&["cursor_visible"], true, "cursor shown again"),
    (&["bracketed_paste"], false, "bracketed paste turned off"),
    (&["mouse"], false, "mouse reporting turned off"),
    (&["focus_events"], false, "focus reporting turned off"),
    (&["application_cursor_keys"], false, "cursor keys back to normal mode"),
    (&["application_keypad"], false, "keypad back to normal mode"),
    (&["insert"], false, "insert mode off"),
];

pub fn hygiene(exit_code: Option<i32>, modes: Option<&Modes>) -> Vec<Check> {
    let mut out = vec![Check {
        name: "exited".into(),
        ok: exit_code.is_some(),
        detail: match exit_code {
            Some(c) => format!("exited with code {c}"),
            None => "still running when the journey ended (no quit step, or quitting failed)".into(),
        },
    }];
    if let Some(code) = exit_code {
        out.push(Check { name: "exit code 0".into(), ok: code == 0, detail: format!("exit code {code}") });
        if let Some(m) = modes {
            for (names, want, what) in HYGIENE {
                if let Some((name, on)) = names.iter().find_map(|n| m.get(*n).map(|v| (*n, *v))) {
                    out.push(Check {
                        name: what.to_string(),
                        ok: on == *want,
                        detail: format!("{name} = {}", if on { "on" } else { "off" }),
                    });
                }
            }
        }
    }
    out
}

/// A cell that stays unchanged this long was there to be read (seconds).
pub const READ_SECS: f64 = 1.0;
/// A shorter look still counts when the user moved on: the cell held at
/// least `GLANCE_SECS` and changed within `REACT_SECS` after an input.
pub const GLANCE_SECS: f64 = 0.25;
pub const REACT_SECS: f64 = 0.5;

/// The cells a person had time to read, each distinct one (position and
/// look) once: unchanged for `READ_SECS`, or for `GLANCE_SECS` until an
/// input changed them. Animations, fades and spinners change on their own
/// many times a second, so they drop out.
pub fn stable_cells(screens: &[Screen], frames: &[Frame], inputs: &[f64]) -> Vec<(usize, u16, u16)> {
    let mut picks = Vec::new();
    let mut seen_cells: HashSet<(u16, u16, Cell)> = HashSet::new();
    // per position: (screen the current look started on, when)
    let mut runs: Vec<Option<(usize, f64)>> = Vec::new();
    let mut dims = (0u16, 0u16);
    let by_input = |end: f64| inputs.iter().any(|&i| i <= end && end - i <= REACT_SECS);
    let mut close = |s: usize, x: u16, y: u16, held: f64, end: f64, picks: &mut Vec<(usize, u16, u16)>| {
        let read = held >= READ_SECS || (held >= GLANCE_SECS && by_input(end));
        if read && seen_cells.insert((x, y, screens[s].cell(x, y).clone())) {
            picks.push((s, x, y));
        }
    };
    for f in frames {
        let sc = &screens[f.s];
        if (sc.cols, sc.rows) != dims {
            dims = (sc.cols, sc.rows);
            runs = vec![None; sc.cols as usize * sc.rows as usize];
        }
        for y in 0..sc.rows {
            for x in 0..sc.cols {
                let i = sc.idx(x, y);
                match runs[i] {
                    Some((s0, _)) if screens[s0].cell(x, y) == sc.cell(x, y) => {}
                    prev => {
                        if let Some((s0, t0)) = prev {
                            close(s0, x, y, f.t - t0, f.t, &mut picks);
                        }
                        runs[i] = Some((f.s, f.t));
                    }
                }
            }
        }
    }
    // what is left on screen at the end stays there
    for (i, r) in runs.iter().enumerate() {
        if let Some((s0, _)) = r {
            let cols = dims.0.max(1) as usize;
            close(*s0, (i % cols) as u16, (i / cols) as u16, f64::INFINITY, f64::INFINITY, &mut picks);
        }
    }
    picks
}

/// Each distinct visible cell (position and look) once, across `screens`.
fn unique_cells(screens: &[Screen]) -> Vec<&Cell> {
    let mut set: HashSet<(u16, u16, &Cell)> = HashSet::new();
    for s in screens {
        for y in 0..s.rows {
            for x in 0..s.cols {
                set.insert((x, y, s.cell(x, y)));
            }
        }
    }
    set.into_iter().map(|(_, _, c)| c).collect()
}

fn visible(s: &Screen) -> usize {
    s.cells.iter().filter(|c| !c.is_spacer() && !c.ch.trim().is_empty()).count()
}

fn used_cols(s: &Screen) -> u16 {
    (0..s.rows)
        .map(|y| s.row(y).iter().rposition(|c| !c.is_blank() && !c.is_spacer()).map_or(0, |x| x as u16 + 1))
        .max()
        .unwrap_or(0)
}

/// Turn a recording (or the error the journey stopped on) into measures.
pub fn outcome(variant: &Variant, rec: anyhow::Result<Recording>, dir: &Path) -> Outcome {
    let (screens, frames, inputs, first, exit_code, modes, fps, duration, broke) = match rec {
        Ok(r) => {
            let screens: Vec<Screen> = (0..r.cast.screens.len()).map(|i| r.cast.screen(i)).collect();
            let fps = r.cast.fps;
            let dur = r.cast.frames.last().map_or(0.0, |f| f.t);
            (screens, r.cast.frames, r.inputs, r.first_screen, r.exit_code, r.end_modes, fps, dur, None)
        }
        Err(e) => {
            // the recorder leaves the screen it gave up on in failed/
            let failed = std::fs::read_to_string(dir.join("failed").join("failed.ansi"))
                .ok()
                .map(|a| anina_screen::escape::parse(&a, variant.cols, variant.rows));
            let msg = format!("{e:#}");
            // the error and its hint, without the screen dump teach appends
            let first_line = msg
                .lines()
                .take_while(|l| !l.trim_start().starts_with("the screen was"))
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" ");
            (failed.into_iter().collect(), Vec::new(), Vec::new(), None, None, None, 0.0, 0.0, Some(first_line))
        }
    };
    let all_text: String = screens.iter().map(|s| s.text()).collect::<Vec<_>>().join("\n");
    let crash_text = CRASH.find(&all_text).map(|m| {
        let line = all_text[..m.start()].rfind('\n').map_or(0, |i| i + 1);
        let end = all_text[m.end()..].find('\n').map_or(all_text.len(), |i| m.end() + i);
        all_text[line..end].trim().chars().take(160).collect::<String>()
    });
    let mut error_lines: Vec<String> =
        ERROR_LINE.find_iter(&all_text).map(|m| m.as_str().trim().chars().take(160).collect()).collect();
    error_lines.dedup();
    error_lines.truncate(5);

    // a journey that stopped because the program exited with an error crashed;
    // one that stopped on a wait broke
    static DIED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"exited with code (-?\d+)").unwrap());
    let died =
        broke.as_deref().and_then(|e| DIED.captures(e)).and_then(|c| c[1].parse::<i32>().ok()).filter(|c| *c != 0);
    let status = match (&crash_text, died, broke) {
        (Some(t), _, _) => Status::Crashed { reason: t.clone() },
        (None, Some(code), Some(err)) => {
            Status::Crashed { reason: format!("exited with code {code} mid-journey: {err}") }
        }
        (None, _, Some(err)) => Status::Broke { error: err },
        _ => Status::Ok,
    };

    let mut colored = 0;
    let mut truecolor = 0;
    let mut beyond16 = 0;
    let mut vis = 0;
    {
        for c in unique_cells(&screens) {
            if c.is_spacer() || c.ch.trim().is_empty() && c.bg == Color::Default {
                continue;
            }
            vis += 1;
            let colors = [c.fg, c.bg];
            if colors.iter().any(|k| *k != Color::Default) {
                colored += 1;
            }
            if colors.iter().any(|k| matches!(k, Color::Rgb(..))) {
                truecolor += 1;
            }
            if colors.iter().any(|k| matches!(k, Color::Rgb(..)) || matches!(k, Color::Idx(n) if *n >= 16)) {
                beyond16 += 1;
            }
        }
    }
    let shot = screens.iter().max_by_key(|s| visible(s)).cloned();
    Outcome {
        variant: variant.clone(),
        status,
        exit_code,
        first_screen_s: first.map(|t| (t * 1000.0).round() / 1000.0),
        duration_s: duration,
        frames: frames.len(),
        screens: screens.len(),
        latency: latency(&frames, &inputs, fps),
        contrast: if frames.is_empty() {
            ratio::measure(&screens)
        } else {
            let at: Vec<f64> = inputs.iter().map(|i| i.t).collect();
            ratio::measure_at(&screens, &stable_cells(&screens, &frames, &at))
        },
        colored_cells: colored,
        truecolor_cells: truecolor,
        beyond16_cells: beyond16,
        visible_cells: vis,
        says_too_small: TOO_SMALL.is_match(&all_text),
        used_rows: screens.iter().map(|s| s.used_rows()).max().unwrap_or(0),
        used_cols: screens.iter().map(used_cols).max().unwrap_or(0),
        crash_text,
        error_lines,
        exited: exit_code.is_some(),
        hygiene: hygiene(exit_code, modes.as_ref()),
        shot,
        shot_png: None,
    }
}

impl Outcome {
    /// Did the program honor what this terminal can show? (None: nothing to check.)
    pub fn color_respected(&self) -> Option<(bool, String)> {
        match self.variant.colors {
            Colors::Truecolor => None,
            Colors::NoColor => Some((
                self.colored_cells == 0,
                format!("{} of {} visible cells colored with NO_COLOR=1", self.colored_cells, self.visible_cells),
            )),
            Colors::C256 => Some((
                self.truecolor_cells == 0,
                format!(
                    "{} of {} visible cells in truecolor without COLORTERM",
                    self.truecolor_cells, self.visible_cells
                ),
            )),
            Colors::C16 => Some((
                self.beyond16_cells == 0,
                format!(
                    "{} of {} visible cells past the 16 colors of TERM=xterm",
                    self.beyond16_cells, self.visible_cells
                ),
            )),
        }
    }
}
