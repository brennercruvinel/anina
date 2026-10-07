//! Scores from 0 to 100 per category. Every number comes from a stated rule
//! over the measures (no weights tuned to any program), and every score
//! carries the lines that explain it.

use super::cases::Colors;
use super::meter::{Outcome, Status};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub category: &'static str,
    /// None when nothing could be measured.
    pub score: Option<u8>,
    pub rule: &'static str,
    pub explain: Vec<String>,
}

fn pct(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// 1 at or below `good`, 0 at or above `bad`, linear in between.
fn ramp(v: f64, good: f64, bad: f64) -> f64 {
    ((bad - v) / (bad - good)).clamp(0.0, 1.0)
}

pub fn stability(all: &[Outcome]) -> Score {
    let mut sum = 0.0;
    let mut explain = Vec::new();
    for o in all {
        let v = match &o.status {
            Status::Ok => 1.0,
            Status::Broke { error } => {
                explain.push(format!("{}: the journey broke: {error}", o.variant.name));
                0.5
            }
            Status::Crashed { reason } => {
                explain.push(format!("{}: crashed: {reason}", o.variant.name));
                0.0
            }
        };
        sum += v;
    }
    if explain.is_empty() {
        explain.push(format!("the journey ran to the end in all {} terminals", all.len()));
    }
    Score {
        category: "stability",
        score: (!all.is_empty()).then(|| pct(sum / all.len() as f64)),
        rule: "per terminal: ran = 1, broke (an expected screen never came) = 0.5, crashed = 0; averaged",
        explain,
    }
}

pub fn responsiveness(base: &Outcome) -> Score {
    let mut parts = Vec::new();
    let mut explain = Vec::new();
    if let Some(t) = base.first_screen_s {
        parts.push(ramp(t, 0.2, 3.0));
        explain.push(format!("first screen after {:.0} ms", t * 1000.0));
    }
    let l = &base.latency;
    if let Some(p95) = l.p95_ms {
        parts.push(ramp(p95, 100.0, 1000.0));
        explain.push(format!(
            "{} inputs answered: p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms (sampled every {} ms)",
            l.answered,
            l.p50_ms.unwrap_or_default(),
            p95,
            l.max_ms.unwrap_or_default(),
            l.resolution_ms
        ));
    }
    if !l.unanswered.is_empty() {
        explain.push(format!("no visible change after: {}", l.unanswered.join(", ")));
    }
    Score {
        category: "responsiveness",
        score: (!parts.is_empty()).then(|| pct(parts.iter().sum::<f64>() / parts.len() as f64)),
        rule: "baseline terminal: first screen 100 at <= 200 ms, 0 at >= 3 s; key latency p95 100 at <= 100 ms, 0 at >= 1 s; averaged",
        explain,
    }
}

pub fn readability(base: &Outcome) -> Score {
    let c = &base.contrast;
    let mut explain = Vec::new();
    if c.text_cells + c.graphic_cells == 0 {
        return Score {
            category: "readability",
            score: None,
            rule: RULE_READ,
            explain: vec!["nothing visible to measure".into()],
        };
    }
    explain.push(format!(
        "text: {} of {} cells below 4.5:1 ({} below 3:1)",
        c.text_below_aa, c.text_cells, c.text_below_3
    ));
    explain.push(format!(
        "graphics (lines, blocks, braille): {} of {} cells below 3:1",
        c.graphic_below_3, c.graphic_cells
    ));
    if c.text_cells < FEW_CELLS {
        explain.push(format!(
            "only {} text cells held still long enough to read; give the journey pauses (Sleep, Idle) where a person would read",
            c.text_cells
        ));
    }
    for p in c.worst.iter().take(4) {
        explain.push(format!(
            "{:.2}:1 {} on {} in {} cells, e.g. {:?} at col {} row {}",
            p.ratio,
            p.fg,
            p.bg,
            p.cells,
            clip(&p.sample, 24),
            p.at[0] + 1,
            p.at[1] + 1
        ));
    }
    let w = if c.graphic_cells == 0 {
        1.0
    } else if c.text_cells == 0 {
        0.0
    } else {
        0.8
    };
    Score {
        category: "readability",
        score: Some(pct(w * c.text_pass_ratio() + (1.0 - w) * c.graphic_pass_ratio())),
        rule: RULE_READ,
        explain,
    }
}

const RULE_READ: &str = "baseline terminal, WCAG 2.x on the cells a person could read (unchanged 1 s, or 0.25 s until a key moved on; drawn-in-background cells skipped; default colors as a dark xterm): share of text at >= 4.5:1 (80%) and of graphics at >= 3:1 (20%)";

/// Below this many readable text cells the contrast sample is thin.
const FEW_CELLS: usize = 200;

pub fn compatibility(all: &[Outcome]) -> Score {
    let mut pass = 0;
    let mut total = 0;
    let mut explain = Vec::new();
    for o in all.iter().filter(|o| !o.variant.baseline) {
        total += 1;
        let small = o.variant.cols < super::cases::BASE.0 || o.variant.rows < super::cases::BASE.1;
        let ran = matches!(o.status, Status::Ok);
        let graceful = small && !matches!(o.status, Status::Crashed { .. }) && o.says_too_small;
        if ran || graceful {
            pass += 1;
            if graceful && !ran {
                explain.push(format!("{}: said the terminal is too small (handled)", o.variant.name));
            }
        } else {
            explain.push(format!("{}: {}", o.variant.name, status_line(o)));
        }
        if let Some((ok, detail)) = o.color_respected() {
            total += 1;
            if ok {
                pass += 1;
            } else {
                let what = match o.variant.colors {
                    Colors::NoColor => "ignored NO_COLOR",
                    _ => "used colors the terminal did not announce",
                };
                explain.push(format!("{}: {what}: {detail}", o.variant.name));
            }
        }
    }
    if total > 0 && pass == total {
        explain.push(format!("all {total} checks passed"));
    }
    Score {
        category: "compatibility",
        score: (total > 0).then(|| pct(pass as f64 / total as f64)),
        rule: "per other terminal: the journey runs (or a small terminal is reported as too small), and colors stay within what TERM/COLORTERM/NO_COLOR allow; share passed",
        explain,
    }
}

pub fn hygiene(base: &Outcome, all: &[Outcome]) -> Score {
    let checks = &base.hygiene;
    let pass = checks.iter().filter(|c| c.ok).count();
    let mut explain: Vec<String> =
        checks.iter().filter(|c| !c.ok).map(|c| format!("{}: {}", c.name, c.detail)).collect();
    // the same exit elsewhere should leave the same state
    for o in all.iter().filter(|o| !o.variant.baseline) {
        let bad: Vec<&str> =
            o.hygiene.iter().filter(|c| !c.ok && c.name != "exited").map(|c| c.name.as_str()).collect();
        if !bad.is_empty() && o.exited {
            explain.push(format!("{}: {}", o.variant.name, bad.join(", ")));
        }
    }
    if explain.is_empty() {
        explain.push(format!("all {} checks passed", checks.len()));
    }
    Score {
        category: "hygiene",
        score: (!checks.is_empty()).then(|| pct(pass as f64 / checks.len() as f64)),
        rule: "baseline terminal after the journey: the program exited, with code 0, and turned off every mode it turned on (alternate screen, hidden cursor, bracketed paste, mouse, focus, keypad); share passed",
        explain,
    }
}

pub fn status_line(o: &Outcome) -> String {
    match &o.status {
        Status::Ok => "ran".into(),
        Status::Broke { error } => format!("broke: {error}"),
        Status::Crashed { reason } => format!("crashed: {reason}"),
    }
}

pub fn all_scores(all: &[Outcome]) -> Vec<Score> {
    let base = all.iter().find(|o| o.variant.baseline).unwrap_or(&all[0]);
    vec![stability(all), responsiveness(base), readability(base), compatibility(all), hygiene(base, all)]
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}
