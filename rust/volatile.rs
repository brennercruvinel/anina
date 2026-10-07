//! Cells that change on their own (spinners, clocks, animations, random
//! names): found by watching a screen nobody is typing into, masked when
//! comparing screens, so "the same screen" means the same to a human.

use crate::engine::Engine;
use crate::screen::{Mask, Screen};
use anyhow::Result;
use std::time::{Duration, Instant};

/// Cells that differ anywhere across `screens` (all the same size).
pub fn from_screens(screens: &[Screen]) -> Mask {
    let Some(first) = screens.first() else {
        return Mask::new(0, 0);
    };
    let mut m = Mask::new(first.cols, first.rows);
    for s in &screens[1..] {
        m.union(&first.diff(s));
    }
    m
}

/// Watch the screen for `secs` without input. Returns the last screen and
/// the cells that changed meanwhile.
pub fn observe(engine: &mut dyn Engine, secs: f64) -> Result<(Screen, Mask)> {
    observe_adaptive(engine, secs, secs, secs)
}

/// Watch at least `min` seconds; keep watching while new cells keep turning
/// volatile, until `quiet` seconds pass without a new one or `max` is
/// reached. A still screen costs `min`; a busy one is learned fully. The mask
/// comes back widened to whole tokens (see `widen`).
pub fn observe_adaptive(engine: &mut dyn Engine, min: f64, max: f64, quiet: f64) -> Result<(Screen, Mask)> {
    let start = Instant::now();
    let first = engine.screen()?;
    let mut last = first.clone();
    let mut mask = Mask::new(first.cols, first.rows);
    let mut grew = Instant::now();
    loop {
        let t = start.elapsed().as_secs_f64();
        // once anything moved, wait longer: many programs redraw once a
        // second or slower (htop, top, dashboards)
        let need = if mask.count() > 0 { quiet.max(1.5) } else { quiet };
        if t >= max || (t >= min && grew.elapsed().as_secs_f64() >= need) {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
        let s = engine.screen()?;
        if s.cols != first.cols || s.rows != first.rows {
            // resized under us: start over at the new size
            mask = Mask::new(s.cols, s.rows);
            last = s;
            grew = Instant::now();
            continue;
        }
        if s.cells != last.cells {
            let before = mask.count();
            mask.union(&last.diff(&s));
            if mask.count() > before {
                grew = Instant::now();
            }
            last = s;
        }
    }
    let wide = widen(&mask, &last);
    Ok((last, wide))
}

/// Grow each masked cell to its whole token on the row (the run of non-blank
/// cells around it) when that token is a value: mostly digits and
/// punctuation (a clock, a counter, a percentage) or drawing (braille plots,
/// bars). A ticking `12:00:01` masks the whole clock; a random character
/// flickering next to a word does not swallow the word.
pub fn widen(mask: &Mask, screen: &Screen) -> Mask {
    let mut out = mask.clone();
    if mask.cols != screen.cols || mask.rows != screen.rows {
        return out;
    }
    let blank = |x: u16, y: u16| {
        let c = screen.cell(x, y);
        !c.is_spacer() && c.ch.trim().is_empty()
    };
    for y in 0..screen.rows {
        let mut x = 0;
        while x < screen.cols {
            if !mask.get(x, y) || blank(x, y) {
                x += 1;
                continue;
            }
            let mut a = x;
            while a > 0 && !blank(a - 1, y) {
                a -= 1;
            }
            let mut b = x;
            while b + 1 < screen.cols && !blank(b + 1, y) {
                b += 1;
            }
            if value_like(screen, y, a, b) {
                for i in a..=b {
                    out.set(i, y);
                }
            }
            x = b + 1;
        }
    }
    out
}

/// Cells `a..=b` of row `y` read as a value or a drawing, not a word.
fn value_like(screen: &Screen, y: u16, a: u16, b: u16) -> bool {
    let chars: Vec<char> = (a..=b).filter_map(|x| screen.cell(x, y).ch.chars().next()).collect();
    if chars.is_empty() || chars.len() > 32 {
        return false;
    }
    let drawing = |c: char| ('\u{2500}'..='\u{28ff}').contains(&c);
    let valueish = chars.iter().filter(|c| c.is_ascii_digit() || c.is_ascii_punctuation() || drawing(**c)).count();
    valueish * 2 > chars.len()
}

/// Two screens are the same state when they match outside `mask`.
pub fn equal_under(a: &Screen, b: &Screen, mask: &Mask) -> bool {
    if a.cols != b.cols || a.rows != b.rows {
        return false;
    }
    let use_mask = mask.cols == a.cols && mask.rows == a.rows;
    a.cells.iter().zip(&b.cells).enumerate().all(|(i, (x, y))| (use_mask && mask.bits[i]) || x == y)
}

/// The union of two masks of possibly different sizes (empty if sizes differ
/// from `cols` x `rows`).
pub fn union_of(a: &Mask, b: &Mask, cols: u16, rows: u16) -> Mask {
    let mut m = Mask::new(cols, rows);
    for src in [a, b] {
        if src.cols == cols && src.rows == rows {
            m.union(src);
        }
    }
    m
}
