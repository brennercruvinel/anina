//! Redaction before anything is written: home paths, user and host names,
//! tokens. Applied to every screen the recorder reads, so secrets never reach
//! a cast, a PNG or an SVG.
//!
//! A replacement keeps the grid: the text after a match, up to the next gap
//! (two blank cells, a box-drawing border or the row end), slides left or
//! right so columns and borders beyond it stay where the program drew them.

use crate::screen::{Cell, Screen};
use regex::Regex;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug)]
pub struct Rule {
    pub re: Regex,
    /// Replacement; `$1`, `${name}` expand from the match's groups.
    pub with: String,
}

#[derive(Clone, Debug, Default)]
pub struct Redactor {
    pub rules: Vec<Rule>,
}

/// Secret shapes worth hiding wherever they show up.
const SECRETS: &[(&str, &str)] = &[
    (r"\bgh[pousr]_[A-Za-z0-9]{20,}", "gh*_••••••••"),
    (r"\bgithub_pat_[A-Za-z0-9_]{20,}", "github_pat_••••••••"),
    (r"\bsk-[A-Za-z0-9_\-]{16,}", "sk-••••••••"),
    (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "AKIA••••••••"),
    (r"\bxox[abposr]-[A-Za-z0-9\-]{10,}", "xox•-••••••••"),
    (r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}", "eyJ••••••••"),
    (
        r#"(?i)\b((?:api[_-]?key|access[_-]?key|secret|token|password|passwd|auth)["']?\s*[=:]\s*["']?)[A-Za-z0-9_\-/+=.]{12,}"#,
        "${1}••••••••",
    ),
];

impl Redactor {
    /// The rules every run gets: this machine's identity and secret shapes.
    pub fn defaults() -> Self {
        let mut r = Redactor::default();
        if let Some(home) = std::env::var("HOME").ok().filter(|h| h.len() > 1) {
            r.literal(home.trim_end_matches('/'), "~");
        }
        for (k, v) in identity() {
            match k {
                Who::User => r.word(&v, "user"),
                Who::Host => r.word(&v, "host"),
                Who::Name => r.word(&v, "Your Name"),
                Who::Email => r.literal(&v, "you@example.com"),
            }
        }
        for (pat, with) in SECRETS {
            r.push(pat, with).expect("built-in secret pattern");
        }
        r
    }

    /// Add a regex rule.
    pub fn push(&mut self, pattern: &str, with: &str) -> anyhow::Result<()> {
        let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("redact pattern {pattern:?}: {e}"))?;
        // a replacement its own pattern matches would loop forever
        if re.is_match(with) && !with.is_empty() {
            anyhow::bail!("redact: the replacement {with:?} matches its own pattern {pattern:?}");
        }
        self.rules.push(Rule { re, with: with.into() });
        Ok(())
    }

    /// Replace an exact text anywhere.
    pub fn literal(&mut self, text: &str, with: &str) {
        if text.is_empty() || text == with {
            return;
        }
        let _ = self.push(&regex::escape(text), &with.replace('$', "$$"));
    }

    /// Replace an exact text only as a whole word, so a short user name does
    /// not eat the middle of other words.
    pub fn word(&mut self, text: &str, with: &str) {
        if text.trim().is_empty() || text == with {
            return;
        }
        let _ = self.push(&format!(r"(?:^|\b){}(?:\b|$)", regex::escape(text)), &with.replace('$', "$$"));
    }

    /// Rewrite matches in place, keeping the cell grid. Rows a long line
    /// soft-wrapped over are treated as the one line they are.
    pub fn apply(&self, screen: &mut Screen) {
        let mut y = 0;
        while y < screen.rows {
            let mut end = y;
            while end + 1 < screen.rows && continues(screen, end) {
                end += 1;
            }
            if end == y {
                self.apply_row(screen, y);
            } else {
                self.apply_wrapped(screen, y, end);
            }
            y = end + 1;
        }
    }

    fn apply_row(&self, screen: &mut Screen, y: u16) {
        for rule in &self.rules {
            // a row has few matches; cap the passes in case one keeps sliding into view
            for _ in 0..16 {
                let (text, col_at) = row_map(screen.row(y));
                let Some((start, end, with)) = rule.find(&text, &col_at, screen.row(y)) else { break };
                splice(screen, y, start, end, &with);
            }
        }
    }

    /// One logical line over rows `y0..=y1`: splice it as a sequence of
    /// cells and lay it back out, as the terminal would have wrapped it.
    fn apply_wrapped(&self, screen: &mut Screen, y0: u16, y1: u16) {
        for rule in &self.rules {
            for _ in 0..16 {
                let cells: Vec<Cell> = (y0..=y1).flat_map(|y| screen.row(y).to_vec()).collect();
                let (text, at) = row_map(&cells);
                let Some((start, end, with)) = rule.find(&text, &at, &cells) else { break };
                let spare = cells.iter().rev().take_while(|c| c.ch == " ").count();
                let room = end - start + spare;
                let mut seq = cells[..start].to_vec();
                seq.extend(styled(&with, &cells[start], room));
                seq.extend_from_slice(&cells[end..]);
                relay(screen, y0, y1, &seq);
            }
        }
    }
}

impl Rule {
    /// The first match in `text` as a cell range and its replacement.
    fn find(&self, text: &str, at: &[u16], cells: &[Cell]) -> Option<(usize, usize, String)> {
        let caps = self.re.captures(text)?;
        let m = caps.get(0)?;
        if m.as_str().is_empty() {
            return None;
        }
        let mut with = String::new();
        caps.expand(&self.with, &mut with);
        if with == m.as_str() {
            return None;
        }
        let start = at[m.start()] as usize;
        let last = at[m.end() - 1] as usize;
        Some((start, last + cell_width(&cells[last]), with))
    }
}

/// Whether row `y` runs on into the next one (a soft wrap): its last cell
/// holds text that is not a border, and the next row starts with text.
fn continues(s: &Screen, y: u16) -> bool {
    let last = s.cell(s.cols - 1, y);
    let next = s.cell(0, y + 1);
    last.ch != " " && !is_border(last) && next.ch != " " && !is_border(next)
}

/// `text` as cells in `style`, wide characters with their spacer, at most
/// `room` cells wide (at least one character).
fn styled(text: &str, style: &Cell, room: usize) -> Vec<Cell> {
    let mut out = Vec::new();
    for ch in text.chars() {
        let cw = ch.width().unwrap_or(0);
        if cw == 0 {
            continue;
        }
        if out.len() + cw > room && !out.is_empty() {
            break;
        }
        out.push(Cell { ch: ch.to_string(), ..style.clone() });
        if cw == 2 {
            out.push(Cell { ch: String::new(), ..style.clone() });
        }
    }
    out
}

/// Lay `seq` over rows `y0..=y1`, never splitting a wide character across a
/// row end, blanks after.
fn relay(s: &mut Screen, y0: u16, y1: u16, seq: &[Cell]) {
    let cols = s.cols as usize;
    let (mut x, mut y) = (0usize, y0);
    let mut i = 0;
    while y <= y1 {
        let cell = match seq.get(i) {
            Some(c) if c.is_spacer() => {
                i += 1;
                continue; // placed with its wide character
            }
            Some(c) => c.clone(),
            None => Cell::default(),
        };
        let wide = seq.get(i + 1).is_some_and(|n| n.is_spacer());
        if wide && x + 1 >= cols {
            *s.cell_mut(x as u16, y) = Cell::default();
        } else {
            *s.cell_mut(x as u16, y) = cell.clone();
            if wide {
                *s.cell_mut(x as u16 + 1, y) = Cell { ch: String::new(), ..cell };
                x += 1;
            }
            i += 1;
        }
        x += 1;
        if x >= cols {
            x = 0;
            y += 1;
        }
    }
}

#[derive(Clone, Copy)]
enum Who {
    User,
    Host,
    Name,
    Email,
}

/// Who and where this is: user names, host names, git identity.
fn identity() -> Vec<(Who, String)> {
    let mut out = Vec::new();
    let mut add = |w: Who, v: String| {
        let v = v.trim().to_string();
        if !v.is_empty() && !out.iter().any(|(_, x): &(Who, String)| *x == v) {
            out.push((w, v));
        }
    };
    let cmd = |prog: &str, args: &[&str]| {
        std::process::Command::new(prog)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    // longest first, so `Brenner Cruvinel` goes before a user name inside it
    add(Who::Name, cmd("git", &["config", "--global", "user.name"]));
    add(Who::Email, cmd("git", &["config", "--global", "user.email"]));
    let host = hostname();
    if let Some(short) = host.split('.').next().filter(|s| *s != host) {
        let short = short.to_string();
        add(Who::Host, host);
        add(Who::Host, short);
    } else {
        add(Who::Host, host);
    }
    for k in ["USER", "LOGNAME"] {
        add(Who::User, std::env::var(k).unwrap_or_default());
    }
    add(Who::User, cmd("id", &["-un"]));
    out
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length and gethostname NUL-terminates within it
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } == 0;
    if !ok {
        return String::new();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// The cells' text and, for each byte of it, the index of its cell.
fn row_map(cells: &[Cell]) -> (String, Vec<u16>) {
    let mut text = String::new();
    let mut col_at = Vec::new();
    for (x, c) in cells.iter().enumerate() {
        if c.is_spacer() {
            continue;
        }
        for _ in 0..c.ch.len() {
            col_at.push(x as u16);
        }
        text.push_str(&c.ch);
    }
    (text, col_at)
}

fn cell_width(c: &Cell) -> usize {
    c.ch.chars().next().and_then(|ch| ch.width()).unwrap_or(1).max(1)
}

fn is_border(c: &Cell) -> bool {
    c.ch.chars().next().is_some_and(|ch| ('\u{2500}'..='\u{259f}').contains(&ch))
}

/// Put `with` over columns `start..end` of row `y`, sliding the text that
/// follows up to the next gap so nothing beyond it moves.
#[allow(clippy::needless_range_loop)] // `i` addresses both the new cells and the old row
fn splice(s: &mut Screen, y: u16, start: usize, end: usize, with: &str) {
    let cols = s.cols as usize;
    let row: Vec<Cell> = s.row(y).to_vec();
    let blank = |i: usize| row[i].ch == " ";
    // the text after the match runs until two blanks, a border or the row end
    let mut text_end = end;
    while text_end < cols
        && !is_border(&row[text_end])
        && !(blank(text_end) && (text_end + 1 >= cols || blank(text_end + 1)))
    {
        text_end += 1;
    }
    // the room we may use: through the blanks that follow, keeping one blank
    // before whatever comes next
    let mut room_end = text_end;
    while room_end < cols && blank(room_end) && !is_border(&row[room_end]) {
        room_end += 1;
    }
    if room_end < cols && room_end > text_end {
        room_end -= 1;
    }
    let tail: Vec<Cell> = row[end..text_end].to_vec();
    let tail_w = tail.len();
    let room = room_end - start;
    let style = row[start].clone();
    let mut repl: Vec<Cell> = Vec::new();
    let mut w = 0;
    for ch in with.chars() {
        let cw = ch.width().unwrap_or(0);
        if cw == 0 {
            continue;
        }
        if w + cw + tail_w > room && !repl.is_empty() {
            break;
        }
        let mut c = style.clone();
        c.ch = ch.to_string();
        repl.push(c);
        if cw == 2 {
            let mut sp = style.clone();
            sp.ch = String::new();
            repl.push(sp);
        }
        w += cw;
    }
    let mut out = repl;
    out.extend(tail);
    // never cut a wide character in half at the edge
    while out.len() > room {
        out.pop();
        if out.last().is_some_and(|c| !c.is_spacer() && cell_width(c) == 2) {
            out.pop();
        }
    }
    for i in start..room_end.max(start + out.len()).min(cols) {
        let cell = match out.get(i - start) {
            Some(c) => c.clone(),
            None => {
                let mut b = row[i].clone();
                if b.is_spacer() || !blank(i) {
                    b = Cell { bg: row[i].bg, ..Cell::default() };
                }
                b.ch = " ".into();
                b
            }
        };
        *s.cell_mut(i as u16, y) = cell;
    }
}
