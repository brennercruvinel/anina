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

/// The scores under `min`, as `category score`, in the report's order. A
/// score that could not be measured is not under any limit: there is no
/// number to hold.
pub fn under(scores: &[Score], min: u8) -> Vec<String> {
    scores.iter().filter_map(|s| s.score.filter(|v| *v < min).map(|v| format!("{} {v}", s.category))).collect()
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use super::super::meter::tests::{matrix, ran};
    use super::super::meter::{self, Check};
    use super::super::ratio::{Pair, Stats};
    use super::*;

    /// The seven terminals of the default matrix, the journey run in each.
    fn all_ran() -> Vec<Outcome> {
        matrix().iter().map(|v| ran(v, "ready")).collect()
    }

    fn broke(o: &mut Outcome) {
        o.status = Status::Broke { error: "Wait \"ready\" never came".into() };
    }

    fn crashed(o: &mut Outcome) {
        o.status = Status::Crashed { reason: "panicked at src/main.rs:3:5".into() };
    }

    fn has(s: &Score, line: &str) -> bool {
        s.explain.iter().any(|e| e == line)
    }

    #[test]
    fn stability_is_one_for_a_run_half_for_a_break_and_nothing_for_a_crash() {
        assert_eq!(stability(&[]).score, None);
        let mut all = all_ran();
        let s = stability(&all);
        assert_eq!((s.category, s.score), ("stability", Some(100)));
        assert_eq!(s.explain, ["the journey ran to the end in all 7 terminals"]);

        all.truncate(3);
        broke(&mut all[2]);
        assert_eq!(stability(&all).score, Some(83), "(1 + 1 + 0.5) / 3");
        crashed(&mut all[1]);
        let s = stability(&all);
        assert_eq!(s.score, Some(50), "(1 + 0 + 0.5) / 3");
        assert!(has(&s, "80x24-truecolor: crashed: panicked at src/main.rs:3:5"), "{:?}", s.explain);
        assert!(has(&s, "60x20-truecolor: the journey broke: Wait \"ready\" never came"), "{:?}", s.explain);

        all.iter_mut().for_each(broke);
        assert_eq!(stability(&all).score, Some(50));
        all.iter_mut().for_each(crashed);
        assert_eq!(stability(&all).score, Some(0));
    }

    #[test]
    fn responsiveness_ramps_from_200_ms_to_3_s_and_from_100_ms_to_1_s() {
        let base = &all_ran()[0];
        for (first, p95, want) in [
            (None, None, None),
            // the ends of the two ramps, and past them
            (Some(0.2), Some(100.0), Some(100)),
            (Some(0.05), Some(10.0), Some(100)),
            (Some(3.0), Some(1000.0), Some(0)),
            (Some(9.0), Some(5000.0), Some(0)),
            // each one alone, halfway and three quarters up
            (Some(1.6), None, Some(50)),
            (None, Some(550.0), Some(50)),
            (Some(0.9), None, Some(75)),
            (None, Some(325.0), Some(75)),
            // the two averaged
            (Some(0.2), Some(1000.0), Some(50)),
            (Some(3.0), Some(100.0), Some(50)),
            (Some(0.9), Some(550.0), Some(63)),
        ] {
            let mut o = base.clone();
            o.first_screen_s = first;
            o.latency.p95_ms = p95;
            let s = responsiveness(&o);
            assert_eq!((s.category, s.score), ("responsiveness", want), "first {first:?}, p95 {p95:?}");
        }
        let mut o = base.clone();
        o.first_screen_s = Some(0.18);
        o.latency.unanswered = vec!["q".into(), "esc".into()];
        let s = responsiveness(&o);
        assert!(has(&s, "first screen after 180 ms") && has(&s, "no visible change after: q, esc"), "{:?}", s.explain);
    }

    #[test]
    fn readability_is_the_share_of_text_at_aa_and_of_graphics_at_three_to_one() {
        let base = &all_ran()[0];
        for (text, text_below, graphic, graphic_below, want) in [
            (0, 0, 0, 0, None),
            (100, 0, 0, 0, Some(100)),
            (100, 100, 0, 0, Some(0)),
            (100, 25, 0, 0, Some(75)),
            // graphics alone carry the whole score
            (0, 0, 10, 0, Some(100)),
            (0, 0, 10, 5, Some(50)),
            // both: 80% text, 20% graphics
            (100, 25, 10, 5, Some(70)),
            (100, 0, 10, 10, Some(80)),
            (100, 100, 10, 0, Some(20)),
            (100, 100, 10, 10, Some(0)),
        ] {
            let mut o = base.clone();
            o.contrast = Stats {
                text_cells: text,
                text_below_aa: text_below,
                graphic_cells: graphic,
                graphic_below_3: graphic_below,
                ..Stats::default()
            };
            let s = readability(&o);
            assert_eq!((s.category, s.score), ("readability", want), "{text_below}/{text} {graphic_below}/{graphic}");
        }
    }

    #[test]
    fn readability_says_what_failed_where_and_when_the_sample_is_thin() {
        let mut o = all_ran()[0].clone();
        assert_eq!(readability(&ran(&o.variant, "")).explain, ["nothing visible to measure"]);
        let pair = Pair {
            fg: "#777777".into(),
            bg: "#ffffff".into(),
            ratio: 4.48,
            cells: 30,
            graphic: false,
            sample: "press q to quit".into(),
            at: [4, 2, 0],
        };
        o.contrast = Stats { text_cells: 5000, text_below_aa: 30, worst: vec![pair], ..Stats::default() };
        let s = readability(&o);
        assert!(has(&s, "text: 30 of 5000 cells below 4.5:1 (0 below 3:1)"), "{:?}", s.explain);
        // counted from 1, the way a person counts columns and rows
        let where_ = "4.48:1 #777777 on #ffffff in 30 cells, e.g. \"press q to quit\" at col 5 row 3";
        assert!(has(&s, where_), "{:?}", s.explain);
        assert!(!s.explain.iter().any(|e| e.contains("held still long enough")), "{:?}", s.explain);
        o.contrast.text_cells = 40;
        let thin = readability(&o);
        assert!(thin.explain.iter().any(|e| e.starts_with("only 40 text cells held still")), "{:?}", thin.explain);
    }

    #[test]
    fn compatibility_is_the_share_of_checks_the_other_terminals_pass() {
        // the baseline is not one of the others
        assert_eq!(compatibility(&all_ran()[..1]).score, None);
        // six journeys, and colors to keep to in three of them
        let s = compatibility(&all_ran());
        assert_eq!((s.category, s.score), ("compatibility", Some(100)));
        assert_eq!(s.explain, ["all 9 checks passed"]);

        // the baseline failing takes nothing from it
        let mut all = all_ran();
        crashed(&mut all[0]);
        assert_eq!(compatibility(&all).score, Some(100));

        let mut all = all_ran();
        broke(&mut all[1]);
        let s = compatibility(&all);
        assert_eq!(s.score, Some(89), "8 of 9");
        assert!(has(&s, "80x24-truecolor: broke: Wait \"ready\" never came"), "{:?}", s.explain);

        // every journey crashed and every color rule broken
        let mut all = all_ran();
        all.iter_mut().for_each(crashed);
        (all[3].truecolor_cells, all[4].beyond16_cells, all[5].colored_cells) = (1, 1, 1);
        assert_eq!(compatibility(&all).score, Some(0));
    }

    #[test]
    fn a_small_terminal_that_says_it_is_too_small_is_handled() {
        let said = |i: usize, fail: fn(&mut Outcome)| {
            let mut all = all_ran();
            fail(&mut all[i]);
            all[i].says_too_small = true;
            compatibility(&all)
        };
        // 60x20 stopped and said why: that is the program handling it
        let s = said(2, broke);
        assert_eq!(s.score, Some(100));
        assert!(has(&s, "60x20-truecolor: said the terminal is too small (handled)"), "{:?}", s.explain);
        // a crash is not handling it, whatever the screen says
        assert_eq!(said(2, crashed).score, Some(89));
        // and a terminal as big as the baseline is not too small
        assert_eq!(said(5, broke).score, Some(89));
    }

    #[test]
    fn colors_past_what_the_terminal_announced_fail_a_check_each() {
        let mut all = all_ran();
        all[5].colored_cells = 12;
        all[5].visible_cells = 17;
        let s = compatibility(&all);
        assert_eq!(s.score, Some(89));
        let line = "120x40-nocolor: ignored NO_COLOR: 12 of 17 visible cells colored with NO_COLOR=1";
        assert!(has(&s, line), "{:?}", s.explain);
        all[4].beyond16_cells = 7;
        all[3].truecolor_cells = 3;
        let s = compatibility(&all);
        assert_eq!(s.score, Some(67), "6 of 9");
        assert!(
            s.explain.iter().any(|e| e.starts_with("120x40-16: used colors the terminal did not announce")),
            "{:?}",
            s.explain
        );
        // a truecolor terminal has no color rule to break
        all[1].truecolor_cells = 100;
        assert_eq!(compatibility(&all).score, Some(67));
    }

    #[test]
    fn hygiene_is_the_share_of_checks_the_baseline_passes_at_exit() {
        let modes = |list: &[(&str, bool)]| list.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let with = |checks: Vec<Check>| {
            let mut all = all_ran();
            all[0].hygiene = checks;
            hygiene(&all[0], &all)
        };
        assert_eq!(with(Vec::new()).score, None);
        let s = with(meter::hygiene(Some(0), None));
        assert_eq!((s.category, s.score), ("hygiene", Some(100)));
        assert_eq!(s.explain, ["all 2 checks passed"]);
        // still running: the one check there is fails
        assert_eq!(with(meter::hygiene(None, None)).score, Some(0));
        assert_eq!(with(meter::hygiene(Some(1), None)).score, Some(50));
        // exited with 0, the alternate screen and the hidden cursor left behind, the mouse turned off
        let left = modes(&[("alternate_screen", true), ("cursor_visible", false), ("mouse", false)]);
        let s = with(meter::hygiene(Some(0), Some(&left)));
        assert_eq!(s.score, Some(60), "3 of 5");
        assert_eq!(
            s.explain,
            ["left the alternate screen: alternate_screen = on", "cursor shown again: cursor_visible = off"]
        );
    }

    #[test]
    fn what_another_terminal_left_behind_is_told_and_not_scored() {
        let mut all = all_ran();
        let mouse = [("mouse".to_string(), true)].into_iter().collect();
        all[2].hygiene = meter::hygiene(Some(0), Some(&mouse));
        let s = hygiene(&all[0], &all);
        assert_eq!(s.score, Some(100));
        assert_eq!(s.explain, ["60x20-truecolor: mouse reporting turned off"]);
    }

    #[test]
    fn the_five_scores_come_in_order_and_read_the_baseline() {
        let mut all = all_ran();
        let cats: Vec<&str> = all_scores(&all).iter().map(|s| s.category).collect();
        assert_eq!(cats, ["stability", "responsiveness", "readability", "compatibility", "hygiene"]);
        // the baseline is found by its mark, not by its place
        all[0].first_screen_s = Some(3.0);
        all.swap(0, 6);
        assert_eq!(all_scores(&all)[1].score, Some(0));
        assert_eq!(status_line(&all[0]), "ran");
    }

    #[test]
    fn a_score_is_under_the_limit_only_when_it_was_measured_and_is_less() {
        let score = |category, score| Score { category, score, rule: "", explain: Vec::new() };
        let all = [
            score("stability", Some(100)),
            score("responsiveness", None),
            score("readability", Some(71)),
            score("compatibility", Some(90)),
            score("hygiene", Some(89)),
        ];
        for (min, want) in [
            (0, vec![]),
            (71, vec![]),
            (72, vec!["readability 71"]),
            (90, vec!["readability 71", "hygiene 89"]),
            (100, vec!["readability 71", "compatibility 90", "hygiene 89"]),
        ] {
            assert_eq!(under(&all, min), want, "min {min}");
        }
    }
}
