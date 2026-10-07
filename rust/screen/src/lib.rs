//! The screen model every part of anina reads: a grid of cells with colors and
//! attributes, plus the cursor. Engines produce it, the recorder stores it,
//! renderers draw it, explore and audit reason over it.

pub mod escape;
pub mod frames;
pub mod lesson;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// A cell color as the program set it: the terminal default, one of the 256
/// indexed colors, or truecolor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Color {
    #[default]
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

/// Cell attributes as bit flags.
pub mod attr {
    pub const BOLD: u8 = 1;
    pub const DIM: u8 = 2;
    pub const ITALIC: u8 = 4;
    pub const UNDERLINE: u8 = 8;
    pub const INVERSE: u8 = 16;
    pub const STRIKE: u8 = 32;
    pub const INVISIBLE: u8 = 64;
    pub const BLINK: u8 = 128;
}

/// One terminal cell. `ch` holds a grapheme; it is empty for the second
/// half of a wide character (the spacer).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub ch: String,
    pub fg: Color,
    pub bg: Color,
    pub attrs: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: " ".into(), fg: Color::Default, bg: Color::Default, attrs: 0 }
    }
}

impl Cell {
    pub fn is_spacer(&self) -> bool {
        self.ch.is_empty()
    }
    pub fn has(&self, flag: u8) -> bool {
        self.attrs & flag != 0
    }
    /// Blank and unstyled: what an untouched terminal cell looks like.
    pub fn is_blank(&self) -> bool {
        self.ch == " " && self.fg == Color::Default && self.bg == Color::Default && self.attrs & !attr::BOLD == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `cols * rows` cells.
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
}

/// Cells to ignore when comparing screens (spinners, clocks, animations).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub cols: u16,
    pub rows: u16,
    pub bits: Vec<bool>,
}

impl Mask {
    pub fn new(cols: u16, rows: u16) -> Self {
        Mask { cols, rows, bits: vec![false; cols as usize * rows as usize] }
    }
    pub fn get(&self, x: u16, y: u16) -> bool {
        x < self.cols && y < self.rows && self.bits[y as usize * self.cols as usize + x as usize]
    }
    pub fn set(&mut self, x: u16, y: u16) {
        if x < self.cols && y < self.rows {
            self.bits[y as usize * self.cols as usize + x as usize] = true;
        }
    }
    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }
    pub fn union(&mut self, other: &Mask) {
        for (a, b) in self.bits.iter_mut().zip(&other.bits) {
            *a |= *b;
        }
    }
}

/// What a wait looks for: a literal text or a regular expression.
#[derive(Clone, Debug)]
pub enum Pattern {
    Text(String),
    Regex(Regex),
}

impl Pattern {
    /// `/re/` is a regex, anything else a literal.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        if s.len() >= 2 && s.starts_with('/') && s.ends_with('/') {
            Ok(Pattern::Regex(Regex::new(&format!("(?m){}", &s[1..s.len() - 1]))?))
        } else {
            Ok(Pattern::Text(s.to_string()))
        }
    }
    pub fn matches(&self, screen: &Screen) -> bool {
        let text = screen.text();
        match self {
            Pattern::Text(t) => {
                text.contains(t.as_str())
                    || screen.text_flat().contains(t.as_str())
                    || screen.text_unwrapped().contains(t.as_str())
            }
            Pattern::Regex(r) => r.is_match(&text),
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Pattern::Text(t) => t,
            Pattern::Regex(r) => r.as_str().trim_start_matches("(?m)"),
        }
    }
}

impl Screen {
    pub fn blank(cols: u16, rows: u16) -> Self {
        Screen { cols, rows, cells: vec![Cell::default(); cols as usize * rows as usize], cursor: Cursor::default() }
    }

    pub fn idx(&self, x: u16, y: u16) -> usize {
        y as usize * self.cols as usize + x as usize
    }

    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.cells[self.idx(x, y)]
    }

    pub fn cell_mut(&mut self, x: u16, y: u16) -> &mut Cell {
        let i = self.idx(x, y);
        &mut self.cells[i]
    }

    pub fn row(&self, y: u16) -> &[Cell] {
        let start = self.idx(0, y);
        &self.cells[start..start + self.cols as usize]
    }

    /// The text of one row, spacers skipped, trailing spaces trimmed.
    pub fn row_text(&self, y: u16) -> String {
        let s: String = self.row(y).iter().filter(|c| !c.is_spacer()).map(|c| c.ch.as_str()).collect();
        s.trim_end().to_string()
    }

    /// All rows as text, trailing blank rows removed.
    pub fn text(&self) -> String {
        let mut rows: Vec<String> = (0..self.rows).map(|y| self.row_text(y)).collect();
        while rows.last().is_some_and(|r| r.is_empty()) {
            rows.pop();
        }
        rows.join("\n")
    }

    /// Rows joined with single spaces and runs of spaces collapsed, so a
    /// literal wait still matches text a program wrapped across rows.
    pub fn text_flat(&self) -> String {
        let joined = (0..self.rows).map(|y| self.row_text(y)).collect::<Vec<_>>().join(" ");
        joined.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Rows as text, a row filled to the last column joined to the next with
    /// nothing in between: a line the terminal wrapped at its edge reads as
    /// one, wherever the edge cut it (a word split at column 60).
    pub fn text_unwrapped(&self) -> String {
        let mut out = String::new();
        for y in 0..self.rows {
            out.push_str(&self.row_text(y));
            let full = self.cols > 0 && self.cell(self.cols - 1, y).ch != " ";
            if !full && y + 1 < self.rows {
                out.push('\n');
            }
        }
        out
    }

    /// The number of rows up to the last one that holds anything visible.
    pub fn used_rows(&self) -> u16 {
        (0..self.rows).rev().find(|&y| self.row(y).iter().any(|c| !c.is_blank() && !c.is_spacer())).map_or(0, |y| y + 1)
    }

    /// Where `needle` starts on screen, as (column, row), counting cells.
    pub fn find(&self, needle: &str) -> Option<(u16, u16)> {
        if needle.is_empty() {
            return None;
        }
        for y in 0..self.rows {
            let row = self.row(y);
            let mut text = String::new();
            let mut col_at = Vec::new(); // byte offset -> column
            for (x, c) in row.iter().enumerate() {
                if c.is_spacer() {
                    continue;
                }
                for _ in 0..c.ch.len() {
                    col_at.push(x as u16);
                }
                text.push_str(&c.ch);
            }
            if let Some(b) = text.find(needle) {
                return Some((col_at[b], y));
            }
        }
        None
    }

    /// A stable 64-bit fingerprint of what is drawn (FNV-1a), skipping masked
    /// cells. The cursor is not part of it.
    pub fn hash(&self, mask: Option<&Mask>) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        eat(&self.cols.to_le_bytes());
        eat(&self.rows.to_le_bytes());
        for y in 0..self.rows {
            for x in 0..self.cols {
                if mask.is_some_and(|m| m.get(x, y)) {
                    eat(&[0xff]);
                    continue;
                }
                let c = self.cell(x, y);
                eat(c.ch.as_bytes());
                eat(&[0]);
                eat(&color_bytes(c.fg));
                eat(&color_bytes(c.bg));
                eat(&[c.attrs]);
            }
        }
        h
    }

    /// The cells that differ between two screens of the same size.
    pub fn diff(&self, other: &Screen) -> Mask {
        let mut m = Mask::new(self.cols, self.rows);
        if self.cols != other.cols || self.rows != other.rows {
            m.bits.iter_mut().for_each(|b| *b = true);
            return m;
        }
        for (i, (a, b)) in self.cells.iter().zip(&other.cells).enumerate() {
            if a != b {
                m.bits[i] = true;
            }
        }
        m
    }

    /// The screen as rows of text with SGR escapes, one line per row, each
    /// row closed with a reset. `crate::escape::parse` reads it back exactly.
    pub fn to_ansi(&self) -> String {
        let mut out = String::new();
        for y in 0..self.rows {
            let row = self.row(y);
            let end = row.iter().rposition(|c| *c != Cell::default()).map_or(0, |i| i + 1);
            let mut last: Option<(Color, Color, u8)> = None;
            for c in &row[..end] {
                if c.is_spacer() {
                    continue;
                }
                let style = (c.fg, c.bg, c.attrs);
                if last != Some(style) {
                    out.push_str(&sgr(c.fg, c.bg, c.attrs));
                    last = Some(style);
                }
                out.push_str(&c.ch);
            }
            out.push_str("\x1b[0m");
            if y + 1 < self.rows {
                out.push('\n');
            }
        }
        out
    }
}

fn color_bytes(c: Color) -> [u8; 4] {
    match c {
        Color::Default => [0, 0, 0, 0],
        Color::Idx(n) => [1, n, 0, 0],
        Color::Rgb(r, g, b) => [2, r, g, b],
    }
}

/// A full SGR sequence (starting from a reset) for a style.
pub fn sgr(fg: Color, bg: Color, attrs: u8) -> String {
    let mut s = String::from("\x1b[0");
    for (flag, code) in [
        (attr::BOLD, "1"),
        (attr::DIM, "2"),
        (attr::ITALIC, "3"),
        (attr::UNDERLINE, "4"),
        (attr::BLINK, "5"),
        (attr::INVERSE, "7"),
        (attr::INVISIBLE, "8"),
        (attr::STRIKE, "9"),
    ] {
        if attrs & flag != 0 {
            s.push(';');
            s.push_str(code);
        }
    }
    for (c, base) in [(fg, 30u16), (bg, 40u16)] {
        match c {
            Color::Default => {}
            Color::Idx(n) if n < 8 => s.push_str(&format!(";{}", base + n as u16)),
            Color::Idx(n) if n < 16 => s.push_str(&format!(";{}", base + 60 + (n - 8) as u16)),
            Color::Idx(n) => s.push_str(&format!(";{};5;{}", base + 8, n)),
            Color::Rgb(r, g, b) => s.push_str(&format!(";{};2;{};{};{}", base + 8, r, g, b)),
        }
    }
    s.push('m');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_matches_text_the_terminal_wrapped_at_its_edge() {
        let s = escape::parse("$ printf 'build %s' comp\r\nlete\r\n$ ", 24, 3);
        assert_eq!(s.row_text(0), "$ printf 'build %s' comp");
        assert!(Pattern::parse("complete").unwrap().matches(&s));
        // a short row is not continued by the next
        let s = escape::parse("ready\r\nnow", 24, 2);
        assert!(!Pattern::parse("readynow").unwrap().matches(&s));
    }
}
