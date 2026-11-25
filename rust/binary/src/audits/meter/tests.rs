//! Tests of `meter`, and the recordings the tests of `score` and `print`
//! measure: beside the module, which would pass 639 lines with them inside.

use super::*;
use anina_screen::escape::parse;
use anina_screen::frames::Cast;
use anyhow::anyhow;

fn frames(list: &[(f64, usize)]) -> Vec<Frame> {
    list.iter().map(|&(t, s)| Frame { t, s, cursor: None, caption: String::new(), speed: 1.0 }).collect()
}

fn inputs(list: &[(f64, &str)]) -> Vec<Input> {
    list.iter().map(|&(t, what)| Input { t, what: what.into() }).collect()
}

/// A recording of a program that exited with 0: `screens` as ANSI rows on
/// a `size` terminal, `shown` as (seconds, screen), `keys` as (seconds, what).
pub fn recording(size: (u16, u16), screens: &[&str], shown: &[(f64, usize)], keys: &[(f64, &str)]) -> Recording {
    let cast = Cast {
        version: 1,
        title: String::new(),
        theme: String::new(),
        cols: size.0,
        rows: size.1,
        fps: 10.0,
        end_hold: 0.0,
        poster: 0,
        tape: String::new(),
        recorded: String::new(),
        engine: String::new(),
        screens: screens.iter().map(|s| s.to_string()).collect(),
        frames: frames(shown),
    };
    Recording {
        cast,
        shots: Vec::new(),
        inputs: inputs(keys),
        first_screen: None,
        exit_code: Some(0),
        end_modes: None,
        warnings: Vec::new(),
    }
}

/// The default matrix: the baseline, 80x24, 60x20, 256 colors, 16 colors, NO_COLOR, LC_ALL=C.
pub fn matrix() -> Vec<Variant> {
    crate::audits::cases::variants(&[], false, false)
}

/// What a journey measures in `variant` when it showed `screen` for two
/// seconds and its program exited with 0.
pub fn ran(variant: &Variant, screen: &str) -> Outcome {
    let rec = recording((variant.cols, variant.rows), &[screen], &[(0.0, 0), (2.0, 0)], &[]);
    outcome(variant, Ok(rec), Path::new(""))
}

#[test]
fn latency_runs_from_a_key_to_the_first_screen_that_differs() {
    let shown = frames(&[(0.0, 0), (1.0, 0), (1.05, 1), (2.0, 1), (2.3, 2), (3.0, 2), (3.5, 2)]);
    let l = latency(&shown, &inputs(&[(1.0, "a"), (2.0, "b"), (3.0, "c")]), 10.0);
    assert_eq!((l.answered, l.p50_ms, l.p95_ms, l.max_ms), (2, Some(50.0), Some(300.0), Some(300.0)));
    assert_eq!(l.unanswered, ["c"]);
    // sampled ten times a second, nothing under 100 ms can be told apart
    assert_eq!(l.resolution_ms, 100.0);
    assert_eq!(latency(&shown, &[], 60.0).resolution_ms, 16.7);
}

#[test]
fn a_key_is_answered_before_the_next_one_and_within_five_seconds() {
    // the screen moved after `b`: `a` got no answer of its own
    let l = latency(&frames(&[(0.0, 0), (1.5, 1)]), &inputs(&[(1.0, "a"), (1.2, "b")]), 10.0);
    assert_eq!((l.answered, l.p50_ms, l.unanswered), (1, Some(300.0), vec!["a".to_string()]));
    let late = latency(&frames(&[(0.0, 0), (7.0, 1)]), &inputs(&[(1.0, "q")]), 10.0);
    assert_eq!((late.answered, late.unanswered), (0, vec!["q".to_string()]));
    // no key pressed: nothing to report, and no percentile made up
    let none = latency(&frames(&[(0.0, 0), (1.0, 1)]), &[], 10.0);
    assert_eq!((none.answered, none.p50_ms, none.p95_ms, none.max_ms), (0, None, None, None));
    assert!(none.unanswered.is_empty());
}

#[test]
fn percentiles_are_the_nearest_rank() {
    // twenty keys ten seconds apart, answered after 10, 20, ... 200 ms
    let shown: Vec<(f64, usize)> = (0..20).map(|i| (10.0 * i as f64 + 0.01 * (i + 1) as f64, (i + 1) % 2)).collect();
    let keys: Vec<(f64, &str)> = (0..20).map(|i| (10.0 * i as f64, "k")).collect();
    let l = latency(&frames(&shown), &inputs(&keys), 60.0);
    assert_eq!((l.answered, l.p50_ms, l.p95_ms, l.max_ms), (20, Some(100.0), Some(190.0), Some(200.0)));
}

#[test]
fn hygiene_checks_the_exit_and_every_mode_the_engine_reports() {
    let modes = |list: &[(&str, bool)]| list.iter().map(|(k, v)| (k.to_string(), *v)).collect::<Modes>();
    let checks =
        |code: Option<i32>, m: Option<&Modes>| hygiene(code, m).into_iter().map(|c| (c.name, c.ok)).collect::<Vec<_>>();
    let named = |list: &[(&str, bool)]| list.iter().map(|(n, ok)| (n.to_string(), *ok)).collect::<Vec<_>>();
    // still running: what it has turned on is not left behind yet
    let on = modes(&[("alternate_screen", true)]);
    assert_eq!(checks(None, None), named(&[("exited", false)]));
    assert_eq!(checks(None, Some(&on)), named(&[("exited", false)]));
    assert_eq!(checks(Some(0), None), named(&[("exited", true), ("exit code 0", true)]));
    assert_eq!(checks(Some(2), None), named(&[("exited", true), ("exit code 0", false)]));
    let left = modes(&[
        ("alternate_screen", true),
        ("cursor_visible", false),
        ("mouse", true),
        ("bracketed_paste", false),
        ("focus_events", false),
    ]);
    assert_eq!(
        checks(Some(0), Some(&left)),
        named(&[
            ("exited", true),
            ("exit code 0", true),
            ("left the alternate screen", false),
            ("cursor shown again", false),
            ("bracketed paste turned off", true),
            ("mouse reporting turned off", false),
            ("focus reporting turned off", true),
        ])
    );
    // tmux names the alternate screen its own way
    let tmux = checks(Some(0), Some(&modes(&[("alt_screen", true)])));
    assert!(tmux.contains(&("left the alternate screen".into(), false)), "{tmux:?}");
    let clean = modes(&[("alternate_screen", false), ("cursor_visible", true), ("mouse", false)]);
    assert!(hygiene(Some(0), Some(&clean)).iter().all(|c| c.ok));
}

/// The cells a person could read, as (column, what it showed), sorted.
fn read(screens: &[&str], shown: &[(f64, usize)], keys: &[f64]) -> Vec<(u16, String)> {
    let s: Vec<Screen> = screens.iter().map(|a| parse(a, 3, 1)).collect();
    let picks = stable_cells(&s, &frames(shown), keys);
    let mut out: Vec<_> = picks.into_iter().map(|(si, x, y)| (x, s[si].cell(x, y).ch.clone())).collect();
    out.sort();
    out
}

fn cells(list: &[(u16, &str)]) -> Vec<(u16, String)> {
    list.iter().map(|(x, ch)| (*x, ch.to_string())).collect()
}

#[test]
fn a_cell_is_read_when_it_held_a_second_or_until_a_key_moved_on() {
    // column 0 spins ten times a second, column 1 never changes, column 2 changes once at 0.4 s
    let spin = ["|bc", "/bc", "-bc", "/bd", "-bd"];
    let shown = [(0.0, 0), (0.1, 1), (0.2, 2), (0.3, 0), (0.4, 3), (0.5, 4)];
    // a key at 0.35 s moved `c` on after it held 0.4 s: it was glanced at. The spinner's
    // looks were not; the one it stopped on stays, like everything left at the end
    assert_eq!(read(&spin, &shown, &[0.35]), cells(&[(0, "-"), (1, "b"), (2, "c"), (2, "d")]));
    // changed by itself, `c` was gone before anyone read it
    assert_eq!(read(&spin, &shown, &[]), cells(&[(0, "-"), (1, "b"), (2, "d")]));

    let once = ["abc", "abd"];
    let gone = cells(&[(0, "a"), (1, "b"), (2, "d")]);
    let both = cells(&[(0, "a"), (1, "b"), (2, "c"), (2, "d")]);
    // 0.9 s is under a second: read only when a key moved it on within half a second
    assert_eq!(read(&once, &[(0.0, 0), (0.9, 1)], &[]), gone);
    assert_eq!(read(&once, &[(0.0, 0), (0.9, 1)], &[0.2]), gone, "the key was 0.7 s before");
    assert_eq!(read(&once, &[(0.0, 0), (0.9, 1)], &[0.95]), gone, "the key came after");
    assert_eq!(read(&once, &[(0.0, 0), (0.9, 1)], &[0.5]), both);
    assert_eq!(read(&once, &[(0.0, 0), (1.0, 1)], &[]), both, "a full second needs no key");
    // a look that comes back is the same cell: counted once
    assert_eq!(read(&once, &[(0.0, 0), (1.5, 1), (3.0, 0), (4.5, 1)], &[]), both);
}

#[test]
fn a_journey_that_ran_is_measured_from_its_recording() {
    let v = matrix();
    let screens = ["$ ls\nnotes", "$ ls\nnotes\n$ "];
    let mut rec = recording((120, 40), &screens, &[(0.0, 0), (0.5, 1), (2.25, 1)], &[(0.4, "enter")]);
    rec.first_screen = Some(0.04321);
    let o = outcome(&v[0], Ok(rec), Path::new(""));
    assert!(matches!(o.status, Status::Ok), "{:?}", o.status);
    assert_eq!((o.exit_code, o.exited, o.frames, o.screens), (Some(0), true, 3, 2));
    assert_eq!((o.duration_s, o.first_screen_s), (2.25, Some(0.043)));
    assert_eq!((o.latency.answered, o.latency.p50_ms), (1, Some(100.0)));
    assert_eq!((o.used_cols, o.used_rows), (5, 3));
    assert!(o.hygiene.iter().all(|c| c.ok) && o.crash_text.is_none() && !o.says_too_small);
    // the screenshot is the fullest screen
    assert_eq!(o.shot.map(|s| s.text()).as_deref(), Some("$ ls\nnotes\n$"));
}

#[test]
fn a_crash_on_screen_or_an_exit_mid_journey_is_a_crash_and_a_failed_wait_is_not() {
    let v = matrix();
    let o = ran(&v[0], "$ app\nthread 'main' panicked at src/main.rs:3:5:\nindex out of bounds");
    let line = "thread 'main' panicked at src/main.rs:3:5:";
    assert!(matches!(&o.status, Status::Crashed { reason } if reason == line), "{:?}", o.status);
    assert_eq!((o.crash_text.as_deref(), o.status.label()), (Some(line), "crashed"));

    let nowhere = Path::new("/nowhere");
    let died = outcome(&v[1], Err(anyhow!("app.tape:4: the program exited with code 101 before Wait")), nowhere);
    let why = "exited with code 101 mid-journey: app.tape:4: the program exited with code 101 before Wait";
    assert!(matches!(&died.status, Status::Crashed { reason } if reason == why), "{:?}", died.status);

    // the error keeps its hint and drops the screen dump after it
    let wait = "app.tape:9: Wait \"ready\" never came\n  is the program slow here?\nthe screen was:\n$ app";
    let broke = outcome(&v[1], Err(anyhow!(wait)), nowhere);
    let said = "app.tape:9: Wait \"ready\" never came is the program slow here?";
    assert!(matches!(&broke.status, Status::Broke { error } if error == said), "{:?}", broke.status);
    assert_eq!((broke.status.label(), broke.exit_code, broke.exited), ("broke", None, false));
    assert_eq!((broke.frames, broke.screens), (0, 0));
    assert_eq!(broke.hygiene.iter().map(|c| (c.name.as_str(), c.ok)).collect::<Vec<_>>(), [("exited", false)]);
    // leaving with 0 before the journey ended is a broken journey, not a crash
    let left = outcome(&v[1], Err(anyhow!("app.tape:4: the program exited with code 0 before Wait")), nowhere);
    assert!(matches!(left.status, Status::Broke { .. }), "{:?}", left.status);
}

#[test]
fn a_journey_that_stopped_is_read_from_the_screen_it_stopped_on() {
    let v = matrix();
    let dir = std::env::temp_dir().join(format!("anina-meter-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("failed")).unwrap();
    let ansi = "\x1b[31mTerminal too small\x1b[0m: needs at least 80x24";
    std::fs::write(dir.join("failed").join("failed.ansi"), ansi).unwrap();
    let o = outcome(&v[2], Err(anyhow!("app.tape:3: Wait \"Files\" never came")), &dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(matches!(o.status, Status::Broke { .. }), "{:?}", o.status);
    assert!(o.says_too_small);
    assert_eq!((o.screens, o.frames, o.used_rows), (1, 0, 1));
    assert_eq!(o.shot.map(|s| s.text()).as_deref(), Some("Terminal too small: needs at least 80x24"));
    // no frames to tell what held still: every cell counts. The 16 red ones are at 3.6:1
    assert_eq!((o.contrast.text_cells, o.contrast.text_below_aa, o.contrast.text_below_3), (34, 16, 0));
}

#[test]
fn a_program_that_says_the_terminal_is_too_small_is_heard() {
    let v = matrix();
    for said in [
        "Terminal too small",
        "Window is too small (80x24 needed)",
        "Please resize your terminal",
        "Enlarge the window to continue",
        "needs at least 80x24",
        "Minimum terminal size: 80 x 24",
    ] {
        assert!(ran(&v[2], said).says_too_small, "{said}");
    }
    for other in ["3 files, 2 too large", "small talk", "resized 4 images", ""] {
        assert!(!ran(&v[2], other).says_too_small, "{other:?}");
    }
}

#[test]
fn error_lines_on_screen_are_kept_once_each() {
    let o = ran(&matrix()[0], "error: no such file\nerror: no such file\n  Error[E12]: bad\nno error: here\nall fine");
    assert_eq!(o.error_lines, ["error: no such file", "Error[E12]: bad"]);
    assert!(matches!(o.status, Status::Ok), "an error line is a note, not a crash");
}

#[test]
fn colors_are_counted_and_held_to_what_the_terminal_announced() {
    let v = matrix();
    // 3 cells in one of the 16 colors, 5 plain, 4 in a 256 color, 3 in truecolor, 2 blanks on a red background
    let colored = "\x1b[31mred\x1b[0m plain \x1b[38;5;200mpink\x1b[0m \x1b[38;2;1;2;3mrgb\x1b[0m \x1b[41m  \x1b[0m";
    let o = ran(&v[0], colored);
    assert_eq!((o.visible_cells, o.colored_cells, o.truecolor_cells, o.beyond16_cells), (17, 12, 3, 7));
    // a truecolor terminal allows everything: nothing to check
    assert_eq!((o.color_respected(), o.colors_respected), (None, None));
    for (i, detail) in [
        (3, "3 of 17 visible cells in truecolor without COLORTERM"),
        (4, "7 of 17 visible cells past the 16 colors of TERM=xterm"),
        (5, "12 of 17 visible cells colored with NO_COLOR=1"),
    ] {
        let o = ran(&v[i], colored);
        assert_eq!(o.color_respected(), Some((false, detail.to_string())), "{}", v[i].name);
        assert_eq!(o.colors_respected, Some(false));
        assert_eq!(ran(&v[i], "plain").colors_respected, Some(true), "{}", v[i].name);
    }
    // the 16 colors are fine without COLORTERM and in TERM=xterm, and still colors under NO_COLOR
    let basic = "\x1b[31mred\x1b[0m \x1b[92mgreen\x1b[0m";
    let kept: Vec<Option<bool>> = [3, 4, 5].iter().map(|&i| ran(&v[i], basic).colors_respected).collect();
    assert_eq!(kept, [Some(true), Some(true), Some(false)]);
    // a cell is counted once, however many screens show it
    let twice = recording((120, 40), &[colored, colored], &[(0.0, 0), (1.0, 1)], &[]);
    assert_eq!(outcome(&v[0], Ok(twice), Path::new("")).colored_cells, 12);
}
