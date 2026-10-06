//! Text with ANSI escapes -> `Screen`. Reads what `Screen::to_ansi` writes
//! (the cast format), tmux `capture-pane -e` output and simple raw streams:
//! SGR in every form (16/256/truecolor, `;` and `:` separators), cursor
//! moves, erase, wide characters, OSC ignored.

use crate::screen::{Cell, Color, Screen, attr};
use unicode_width::UnicodeWidthChar;
use vte::{Params, Parser, Perform};

/// Parse `text` into a `cols` x `rows` screen. A newline is a CR+LF, the way
/// captures and casts store rows; one final newline ends the last row
/// (tmux captures end that way) instead of scrolling the screen.
pub fn parse(text: &str, cols: u16, rows: u16) -> Screen {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let mut grid =
        Grid { s: Screen::blank(cols, rows), x: 0, y: 0, pen: Cell::default(), crlf: true, pending_wrap: false };
    let mut p = Parser::new();
    p.advance(&mut grid, text.as_bytes());
    grid.s.cursor.x = grid.x.min(cols.saturating_sub(1));
    grid.s.cursor.y = grid.y.min(rows.saturating_sub(1));
    grid.s
}

struct Grid {
    s: Screen,
    x: u16,
    y: u16,
    pen: Cell,
    crlf: bool,
    pending_wrap: bool,
}

impl Grid {
    fn newline(&mut self) {
        if self.y + 1 >= self.s.rows {
            // scroll up one row
            let cols = self.s.cols as usize;
            self.s.cells.drain(0..cols);
            self.s.cells.extend(std::iter::repeat_n(Cell::default(), cols));
        } else {
            self.y += 1;
        }
    }

    fn erase(&mut self, from: usize, to: usize) {
        let blank = Cell { ch: " ".into(), fg: Color::Default, bg: self.pen.bg, attrs: 0 };
        let to = to.min(self.s.cells.len());
        for c in &mut self.s.cells[from.min(to)..to] {
            *c = blank.clone();
        }
    }

    fn sgr(&mut self, params: &Params) {
        let ps: Vec<Vec<u16>> = params.iter().map(|p| p.to_vec()).collect();
        if ps.is_empty() {
            self.reset_pen();
            return;
        }
        let mut i = 0;
        while i < ps.len() {
            let p = &ps[i];
            let code = p[0];
            match code {
                0 => self.reset_pen(),
                1 => self.pen.attrs |= attr::BOLD,
                2 => self.pen.attrs |= attr::DIM,
                3 => self.pen.attrs |= attr::ITALIC,
                4 => {
                    // 4:0 turns underline off
                    if p.get(1) == Some(&0) {
                        self.pen.attrs &= !attr::UNDERLINE
                    } else {
                        self.pen.attrs |= attr::UNDERLINE
                    }
                }
                5 | 6 => self.pen.attrs |= attr::BLINK,
                7 => self.pen.attrs |= attr::INVERSE,
                8 => self.pen.attrs |= attr::INVISIBLE,
                9 => self.pen.attrs |= attr::STRIKE,
                21 => self.pen.attrs |= attr::UNDERLINE,
                22 => self.pen.attrs &= !(attr::BOLD | attr::DIM),
                23 => self.pen.attrs &= !attr::ITALIC,
                24 => self.pen.attrs &= !attr::UNDERLINE,
                25 => self.pen.attrs &= !attr::BLINK,
                27 => self.pen.attrs &= !attr::INVERSE,
                28 => self.pen.attrs &= !attr::INVISIBLE,
                29 => self.pen.attrs &= !attr::STRIKE,
                30..=37 => self.pen.fg = Color::Idx((code - 30) as u8),
                39 => self.pen.fg = Color::Default,
                40..=47 => self.pen.bg = Color::Idx((code - 40) as u8),
                49 => self.pen.bg = Color::Default,
                90..=97 => self.pen.fg = Color::Idx((code - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Idx((code - 100 + 8) as u8),
                38 | 48 | 58 => {
                    let (color, used) = if p.len() > 1 {
                        (extended(&p[1..]), 0) // colon form: 38:2:r:g:b or 38:2::r:g:b
                    } else {
                        let rest: Vec<u16> = ps[i + 1..].iter().map(|q| q[0]).collect();
                        match rest.first() {
                            Some(5) => (extended(&rest[..2.min(rest.len())]), 2),
                            Some(2) => (extended(&rest[..4.min(rest.len())]), 4),
                            _ => (None, 0),
                        }
                    };
                    if let Some(c) = color {
                        match code {
                            38 => self.pen.fg = c,
                            48 => self.pen.bg = c,
                            _ => {} // underline color: not modeled
                        }
                    }
                    i += used;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn reset_pen(&mut self) {
        self.pen = Cell::default();
    }
}

/// `5;n` or `2;r;g;b` (also `2;colorspace;r;g;b` from the colon form).
fn extended(p: &[u16]) -> Option<Color> {
    match p.first()? {
        5 => Some(Color::Idx(*p.get(1)? as u8)),
        2 => {
            let v = if p.len() >= 5 { &p[2..5] } else { p.get(1..4)? };
            Some(Color::Rgb(v[0] as u8, v[1] as u8, v[2] as u8))
        }
        _ => None,
    }
}

impl Perform for Grid {
    fn print(&mut self, c: char) {
        let w = c.width().unwrap_or(0) as u16;
        if w == 0 {
            // combining mark: attach to the previous cell
            if self.x > 0 {
                let x = self.x - 1;
                let y = self.y;
                self.s.cell_mut(x, y).ch.push(c);
            }
            return;
        }
        if self.pending_wrap || self.x + w > self.s.cols {
            self.x = 0;
            self.newline();
            self.pending_wrap = false;
        }
        let (x, y) = (self.x, self.y);
        let mut cell = self.pen.clone();
        cell.ch = c.to_string();
        *self.s.cell_mut(x, y) = cell;
        if w == 2 && x + 1 < self.s.cols {
            let mut spacer = self.pen.clone();
            spacer.ch = String::new();
            *self.s.cell_mut(x + 1, y) = spacer;
        }
        self.x += w;
        if self.x >= self.s.cols {
            self.x = self.s.cols - 1;
            self.pending_wrap = true;
        }
    }

    fn execute(&mut self, byte: u8) {
        self.pending_wrap = false;
        match byte {
            b'\n' => {
                if self.crlf {
                    self.x = 0;
                }
                self.newline();
            }
            b'\r' => self.x = 0,
            b'\t' => self.x = ((self.x / 8 + 1) * 8).min(self.s.cols - 1),
            0x08 => self.x = self.x.saturating_sub(1),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        if !intermediates.is_empty() && action != 'm' {
            return;
        }
        let first = |d: u16| params.iter().next().map(|p| p[0]).filter(|v| *v != 0).unwrap_or(d);
        let (cols, rows) = (self.s.cols, self.s.rows);
        self.pending_wrap = false;
        match action {
            'm' => self.sgr(params),
            'H' | 'f' => {
                let mut it = params.iter();
                let r = it.next().map(|p| p[0]).filter(|v| *v != 0).unwrap_or(1);
                let c = it.next().map(|p| p[0]).filter(|v| *v != 0).unwrap_or(1);
                self.y = (r - 1).min(rows - 1);
                self.x = (c - 1).min(cols - 1);
            }
            'A' => self.y = self.y.saturating_sub(first(1)),
            'B' => self.y = (self.y + first(1)).min(rows - 1),
            'C' => self.x = (self.x + first(1)).min(cols - 1),
            'D' => self.x = self.x.saturating_sub(first(1)),
            'G' => self.x = (first(1) - 1).min(cols - 1),
            'd' => self.y = (first(1) - 1).min(rows - 1),
            'K' => {
                let row = self.y as usize * cols as usize;
                let here = row + self.x as usize;
                match params.iter().next().map_or(0, |p| p[0]) {
                    0 => self.erase(here, row + cols as usize),
                    1 => self.erase(row, here + 1),
                    _ => self.erase(row, row + cols as usize),
                }
            }
            'J' => {
                let here = self.y as usize * cols as usize + self.x as usize;
                let all = cols as usize * rows as usize;
                match params.iter().next().map_or(0, |p| p[0]) {
                    0 => self.erase(here, all),
                    1 => self.erase(0, here + 1),
                    _ => self.erase(0, all),
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
}
