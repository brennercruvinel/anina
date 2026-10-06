//! WCAG 2.x contrast of what a program actually drew: every visible cell's
//! foreground against its background, after resolving the terminal default
//! and indexed colors through the xterm palette (dark background, light text,
//! the way most terminals ship), with INVERSE and DIM applied.

use crate::screen::{Cell, Color, Screen, attr};
use serde::Serialize;
use std::collections::HashMap;

/// The default text and background of the reference terminal.
pub const DEFAULT_FG: [u8; 3] = [0xe5, 0xe5, 0xe5];
pub const DEFAULT_BG: [u8; 3] = [0x00, 0x00, 0x00];

/// WCAG AA for text, and for graphical objects (box drawing, blocks, braille).
pub const AA_TEXT: f64 = 4.5;
pub const AA_GRAPHIC: f64 = 3.0;

/// The xterm 256-color palette.
pub fn xterm(n: u8) -> [u8; 3] {
    const BASE: [[u8; 3]; 16] = [
        [0x00, 0x00, 0x00],
        [0xcd, 0x00, 0x00],
        [0x00, 0xcd, 0x00],
        [0xcd, 0xcd, 0x00],
        [0x00, 0x00, 0xee],
        [0xcd, 0x00, 0xcd],
        [0x00, 0xcd, 0xcd],
        [0xe5, 0xe5, 0xe5],
        [0x7f, 0x7f, 0x7f],
        [0xff, 0x00, 0x00],
        [0x00, 0xff, 0x00],
        [0xff, 0xff, 0x00],
        [0x5c, 0x5c, 0xff],
        [0xff, 0x00, 0xff],
        [0x00, 0xff, 0xff],
        [0xff, 0xff, 0xff],
    ];
    match n {
        0..=15 => BASE[n as usize],
        16..=231 => {
            let i = n - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [level(i / 36), level(i / 6 % 6), level(i % 6)]
        }
        _ => {
            let g = 8 + (n - 232) * 10;
            [g, g, g]
        }
    }
}

pub fn resolve(c: Color, default: [u8; 3]) -> [u8; 3] {
    match c {
        Color::Default => default,
        Color::Idx(n) => xterm(n),
        Color::Rgb(r, g, b) => [r, g, b],
    }
}

/// The colors a cell is seen in: (foreground, background).
pub fn seen(cell: &Cell) -> ([u8; 3], [u8; 3]) {
    let mut fg = resolve(cell.fg, DEFAULT_FG);
    let mut bg = resolve(cell.bg, DEFAULT_BG);
    if cell.has(attr::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.has(attr::DIM) {
        // terminals draw dim text at about two thirds of its intensity
        for i in 0..3 {
            fg[i] = ((fg[i] as f64 * 2.0 + bg[i] as f64) / 3.0).round() as u8;
        }
    }
    (fg, bg)
}

/// WCAG relative luminance.
pub fn luminance(c: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG contrast ratio, 1 to 21.
pub fn ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Box drawing, block elements and braille are graphics (3:1), the rest text (4.5:1).
pub fn is_graphic(ch: &str) -> bool {
    ch.chars().next().is_some_and(|c| matches!(c as u32, 0x2500..=0x259f | 0x2800..=0x28ff | 0x25a0..=0x25ff))
}

/// A color pair that fails, with where it shows.
#[derive(Clone, Debug, Serialize)]
pub struct Pair {
    pub fg: String,
    pub bg: String,
    pub ratio: f64,
    pub cells: usize,
    pub graphic: bool,
    /// Some text drawn in this pair, and its first position (col, row, screen).
    pub sample: String,
    pub at: [usize; 3],
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    pub text_cells: usize,
    pub text_below_aa: usize,
    pub text_below_3: usize,
    pub graphic_cells: usize,
    pub graphic_below_3: usize,
    /// Failing pairs, worst first.
    pub worst: Vec<Pair>,
}

impl Stats {
    pub fn text_pass_ratio(&self) -> f64 {
        if self.text_cells == 0 { 1.0 } else { 1.0 - self.text_below_aa as f64 / self.text_cells as f64 }
    }
    pub fn graphic_pass_ratio(&self) -> f64 {
        if self.graphic_cells == 0 { 1.0 } else { 1.0 - self.graphic_below_3 as f64 / self.graphic_cells as f64 }
    }
}

/// (foreground, background, graphic)
type FailKey = ([u8; 3], [u8; 3], bool);

/// Contrast over every visible, non-blank cell of `screens`.
pub fn measure(screens: &[Screen]) -> Stats {
    let all: Vec<(usize, u16, u16)> = screens
        .iter()
        .enumerate()
        .flat_map(|(si, s)| (0..s.rows).flat_map(move |y| (0..s.cols).map(move |x| (si, x, y))))
        .collect();
    measure_at(screens, &all)
}

/// Contrast over the cells at `picks` (screen index, col, row). A cell drawn
/// in its own background color is hidden on purpose and not measured.
pub fn measure_at(screens: &[Screen], picks: &[(usize, u16, u16)]) -> Stats {
    let mut st = Stats::default();
    // (fg, bg, graphic) -> (cells, sample text, position)
    let mut fails: HashMap<FailKey, (usize, String, [usize; 3])> = HashMap::new();
    for &(si, x, y) in picks {
        let s = &screens[si];
        {
            {
                let c = s.cell(x, y);
                if c.is_spacer() || c.ch.trim().is_empty() || c.has(attr::INVISIBLE) {
                    continue;
                }
                let (fg, bg) = seen(c);
                if fg == bg {
                    continue;
                }
                let r = ratio(fg, bg);
                let graphic = is_graphic(&c.ch);
                let failed = if graphic {
                    st.graphic_cells += 1;
                    if r < AA_GRAPHIC {
                        st.graphic_below_3 += 1;
                    }
                    r < AA_GRAPHIC
                } else {
                    st.text_cells += 1;
                    if r < AA_TEXT {
                        st.text_below_aa += 1;
                    }
                    if r < 3.0 {
                        st.text_below_3 += 1;
                    }
                    r < AA_TEXT
                };
                if failed {
                    let e = fails
                        .entry((fg, bg, graphic))
                        .or_insert_with(|| (0, run_text(s, x, y, fg, bg), [x as usize, y as usize, si]));
                    e.0 += 1;
                }
            }
        }
    }
    let mut worst: Vec<Pair> = fails
        .into_iter()
        .map(|((fg, bg, graphic), (cells, sample, at))| Pair {
            fg: hex(fg),
            bg: hex(bg),
            ratio: round2(ratio(fg, bg)),
            cells,
            graphic,
            sample,
            at,
        })
        .collect();
    // worst ratio first, then the most cells; ties broken by colors so the order is stable
    worst.sort_by(|a, b| {
        a.ratio.total_cmp(&b.ratio).then(b.cells.cmp(&a.cells)).then(a.fg.cmp(&b.fg)).then(a.bg.cmp(&b.bg))
    });
    worst.truncate(12);
    st.worst = worst;
    st
}

/// The run of text around (x, y) drawn in the same colors, for a sample.
fn run_text(s: &Screen, x: u16, y: u16, fg: [u8; 3], bg: [u8; 3]) -> String {
    let same = |cx: u16| {
        let c = s.cell(cx, y);
        c.is_spacer() || seen(c) == (fg, bg) || c.ch == " "
    };
    let mut a = x;
    while a > 0 && same(a - 1) {
        a -= 1;
    }
    let mut b = x;
    while b + 1 < s.cols && same(b + 1) {
        b += 1;
    }
    let text: String = (a..=b).map(|cx| s.cell(cx, y)).filter(|c| !c.is_spacer()).map(|c| c.ch.as_str()).collect();
    let t = text.trim();
    t.chars().take(40).collect()
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
