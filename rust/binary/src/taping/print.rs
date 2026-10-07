//! A session (what the user did, and the screen right before each input)
//! -> a tape that replays it. Typing merges into `Type`, keys become `Key`,
//! and after each step comes a `Wait` for text that newly appeared and stays
//! put (volatile cells excluded), or a short `Sleep` when nothing did.

use super::input::Ev;
use anina_screen::{Mask, Screen};

/// One input with the screen as it was just before it.
#[derive(Clone, Debug)]
pub struct Input {
    /// Seconds since the program started.
    pub t: f64,
    pub ev: Ev,
    pub before: Screen,
    /// Cells seen changing over and over while nobody typed, in the stretch
    /// that ended with this input (an animation on the screen it acts on).
    pub quiet: Mask,
}

/// Everything needed to write the tape.
#[derive(Clone, Debug)]
pub struct Session {
    pub cols: u16,
    pub rows: u16,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub clean_env: bool,
    pub inputs: Vec<Input>,
    /// Terminal resizes: (time, cols, rows).
    pub resizes: Vec<(f64, u16, u16)>,
    /// The screen when the session ended, and when.
    pub last: Screen,
    pub end: f64,
    /// `Input::quiet` of the stretch after the last input.
    pub last_quiet: Mask,
    /// Cells that keep changing on every screen (a logo animation): volatile
    /// in at least two stretches.
    pub volatile: Mask,
}

/// Quote a word for the tape parser so it reads back exactly: bare when
/// safe, '...' (literal, never expanded) otherwise, "..." for text with a
/// single quote.
pub fn quote(w: &str) -> String {
    let bare = !w.is_empty()
        && !w.starts_with('~')
        && !w.starts_with('#')
        && w.chars().all(|c| c.is_ascii_alphanumeric() || "_./:=@%+,-".contains(c));
    if bare {
        return w.to_string();
    }
    if !w.contains('\'') {
        return format!("'{w}'");
    }
    // pieces in '...' joined by "'" keep everything literal
    w.split('\'').map(|p| format!("'{p}'")).collect::<Vec<_>>().join("\"'\"")
}

/// `Type` arguments for `text`: the tape joins them, so a text with single
/// quotes becomes several literal words. Never exactly two words whose second
/// reads as a duration (that would be `Type TEXT SECS`).
fn type_args(text: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for (i, part) in text.split('\'').enumerate() {
        if i > 0 {
            words.push("\"'\"".into());
        }
        if !part.is_empty() {
            words.push(format!("'{part}'"));
        }
    }
    if words.is_empty() {
        words.push("''".into());
    }
    if words.len() == 2 {
        words.push("''".into());
    }
    words.join(" ")
}

/// A step the tape will say.
#[derive(Clone, Debug, PartialEq)]
enum Out {
    Type(String),
    Key(String, u32),
    Resize(u16, u16),
    Comment(String),
}

/// Group inputs into steps: text typed in a row (screen changing only on
/// the cursor row) becomes one Type; control characters inside text become
/// keys. Returns (step, time it started, index of its first input).
fn group(s: &Session) -> Vec<(Out, f64, usize)> {
    let mut out: Vec<(Out, f64, usize)> = Vec::new();
    let mut last_text_end: Option<(f64, usize)> = None; // (time, input index) of the last text input
    for (i, inp) in s.inputs.iter().enumerate() {
        match &inp.ev {
            Ev::Text(t) => {
                let mut first = true;
                for piece in split_controls(t) {
                    match piece {
                        Piece::Text(p) => {
                            let continues = first
                                && matches!(out.last(), Some((Out::Type(_), _, _)))
                                && last_text_end.is_some_and(|(lt, li)| {
                                    inp.t - lt < 1.5
                                        && typing_only(
                                            &s.inputs[li].before,
                                            &inp.before,
                                            &union(&s.volatile, &inp.quiet),
                                        )
                                });
                            if continues {
                                if let Some((Out::Type(prev), _, _)) = out.last_mut() {
                                    prev.push_str(&p);
                                }
                            } else {
                                out.push((Out::Type(p), inp.t, i));
                            }
                        }
                        Piece::Key(k) => out.push((Out::Key(k, 1), inp.t, i)),
                    }
                    first = false;
                }
                last_text_end = Some((inp.t, i));
            }
            Ev::Key(k) => {
                out.push((Out::Key(k.clone(), 1), inp.t, i));
                last_text_end = None;
            }
            Ev::Unknown(b) => out.push((Out::Comment(format!("input not expressible as a step: {b}")), inp.t, i)),
        }
    }
    out
}

enum Piece {
    Text(String),
    Key(String),
}

/// Pasted text may hold newlines and tabs: those go as keys.
fn split_controls(t: &str) -> Vec<Piece> {
    let mut v = Vec::new();
    let mut cur = String::new();
    for c in t.chars() {
        let k = match c {
            '\r' => Some("enter"),
            '\n' => Some("ctrl+j"),
            '\t' => Some("tab"),
            _ => None,
        };
        match k {
            Some(k) => {
                if !cur.is_empty() {
                    v.push(Piece::Text(std::mem::take(&mut cur)));
                }
                v.push(Piece::Key(k.into()));
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        v.push(Piece::Text(cur));
    }
    v
}

/// Between two screens, did little more than the cursor rows change (the
/// echo of typing, maybe a hint or a counter), volatile cells aside? A new
/// screen (a key that opened something) changes far more.
fn typing_only(a: &Screen, b: &Screen, volatile: &Mask) -> bool {
    if a.cols != b.cols || a.rows != b.rows {
        return false;
    }
    let d = a.diff(b);
    let elsewhere = (0..a.rows)
        .filter(|y| *y != a.cursor.y && *y != b.cursor.y)
        .flat_map(|y| (0..a.cols).map(move |x| (x, y)))
        .filter(|(x, y)| d.get(*x, *y) && !volatile.get(*x, *y))
        .count();
    elsewhere * 10 <= a.cols as usize * a.rows as usize
}

/// A text segment of one row, with the columns it covers.
struct Seg {
    text: String,
    y: u16,
    /// Column of each char of `text`.
    cols: Vec<u16>,
}

fn is_separator(c: &str) -> bool {
    let Some(ch) = c.chars().next() else { return true };
    matches!(ch as u32, 0x2500..=0x259f | 0x2800..=0x28ff) || ch == '|'
}

/// Segments of a row: runs split by two or more spaces or by box drawing,
/// blocks and braille.
fn segments(s: &Screen, y: u16) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut cur = Seg { text: String::new(), y, cols: Vec::new() };
    let mut spaces = 0;
    let flush = |cur: &mut Seg, out: &mut Vec<Seg>| {
        if !cur.text.trim().is_empty() {
            out.push(std::mem::replace(cur, Seg { text: String::new(), y, cols: Vec::new() }));
        } else {
            *cur = Seg { text: String::new(), y, cols: Vec::new() };
        }
    };
    for x in 0..s.cols {
        let c = s.cell(x, y);
        if c.is_spacer() {
            continue;
        }
        if is_separator(&c.ch) {
            flush(&mut cur, &mut out);
            spaces = 0;
            continue;
        }
        if c.ch == " " {
            spaces += 1;
            if spaces >= 2 {
                flush(&mut cur, &mut out);
                continue;
            }
        } else {
            spaces = 0;
        }
        for ch in c.ch.chars() {
            cur.text.push(ch);
            cur.cols.push(x);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Trim decoration at both ends (spaces, bullets, separators, a prompt sign).
fn trim_span(chars: &[char]) -> (usize, usize) {
    let deco = |c: char| c.is_whitespace() || "·•›»>|│─—-:,;*$#›❯➜".contains(c);
    let mut a = 0;
    let mut b = chars.len();
    while a < b && deco(chars[a]) {
        a += 1;
    }
    while b > a && (chars[b - 1].is_whitespace() || "·•›»|│─—-:,;*".contains(chars[b - 1])) {
        b -= 1;
    }
    (a, b)
}

/// A hash or generated id: long, hex-ish, digits mixed with letters.
fn looks_like_id(w: &str) -> bool {
    let hex = w.chars().filter(|c| c.is_ascii_hexdigit()).count();
    let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
    hex >= 8 && digits >= 2 && hex * 10 >= w.chars().count() * 7
}

fn count(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    hay.match_indices(needle).count()
}

const MAX_WAIT: usize = 48;

/// The best text to wait for after a step: new since `before`, unique on
/// `after`, made of cells that do not change by themselves, preferably free
/// of digits (timings and counters change from run to run). Returns the
/// tape argument (a literal, or a `/^row$/` regex for a whole new row).
pub fn distinctive(before: &Screen, after: &Screen, volatile: &Mask) -> Option<String> {
    candidates(before, after, volatile).into_iter().next().filter(|c| c.0 > 0).map(|c| c.1)
}

/// Every text `distinctive` could pick, best first, with its score.
pub fn candidates(before: &Screen, after: &Screen, volatile: &Mask) -> Vec<(i64, String)> {
    let before_text = format!("{}\n{}", before.text(), before.text_flat());
    let after_text = after.text();
    let mut found: Vec<(i64, String)> = Vec::new();
    let mut consider = |score: i64, arg: String| found.push((score, arg));
    for y in 0..after.rows {
        let row = after.row_text(y);
        let row_new = !before.text().lines().any(|l| l == row);
        for seg in segments(after, y) {
            let chars: Vec<char> = seg.text.chars().collect();
            // the whole segment and its digit-free word runs
            let mut spans = vec![(0, chars.len())];
            let mut start = None;
            let mut i = 0;
            while i <= chars.len() {
                let word_end = i == chars.len() || chars[i] == ' ';
                if word_end {
                    // find the word that just ended
                    let ws = chars[..i].iter().rposition(|c| *c == ' ').map_or(0, |p| p + 1);
                    let has_digit = chars[ws..i].iter().any(|c| c.is_ascii_digit());
                    if has_digit {
                        if let Some(s) = start.take() {
                            spans.push((s, ws));
                        }
                    } else if start.is_none() && ws < i {
                        start = Some(ws);
                    }
                }
                i += 1;
            }
            if let Some(s) = start {
                spans.push((s, chars.len()));
            }
            for (a, b) in spans {
                let (ta, tb) = trim_span(&chars[a..b]);
                let (mut a, mut b) = (a + ta, a + tb);
                if b - a > MAX_WAIT {
                    // cut at a word boundary
                    let cut = chars[a..a + MAX_WAIT].iter().rposition(|c| *c == ' ').unwrap_or(MAX_WAIT);
                    b = a + cut;
                    let (ta, tb) = trim_span(&chars[a..b]);
                    (a, b) = (a + ta, a + tb);
                }
                if b <= a {
                    continue;
                }
                let text: String = chars[a..b].iter().collect();
                let alnum = text.chars().filter(|c| c.is_alphanumeric()).count();
                if alnum < 3 || text.contains('\'') && text.contains('"') {
                    continue;
                }
                if seg.cols[a..b].iter().any(|x| volatile.get(*x, seg.y)) {
                    continue;
                }
                if before_text.contains(&text) || count(&after_text, &text) != 1 {
                    continue;
                }
                let digits = text.chars().any(|c| c.is_ascii_digit());
                let mut score = alnum.min(40) as i64;
                score += if digits { -20 } else { 8 };
                if text.split_whitespace().any(looks_like_id) {
                    score -= 10;
                }
                if row_new {
                    score += 4;
                }
                consider(score, quote(&text));
            }
        }
        // a whole new row, matched as a line
        let alnum = row.chars().filter(|c| c.is_alphanumeric()).count();
        let unique_line = after_text.lines().filter(|l| *l == row).count() == 1;
        let stable = (0..after.cols).all(|x| !volatile.get(x, y) || after.cell(x, y).is_blank());
        if row_new && alnum >= 2 && unique_line && stable && row.chars().count() <= 80 {
            let digits = row.chars().any(|c| c.is_ascii_digit());
            let score = alnum.min(40) as i64 - 2 + if digits { -20 } else { 8 };
            consider(score, quote(&format!("/^{}$/", regex::escape(&row))));
        }
    }
    // best score first; on a tie the one higher on screen (found first)
    found.sort_by_key(|c| std::cmp::Reverse(c.0));
    found
}

/// The tape text for a session.
pub fn write(s: &Session) -> String {
    let mut lines = vec![
        "# recorded with `anina init`; replay with: anina record THIS.tape".to_string(),
        format!("Set Cols {}", s.cols),
        format!("Set Rows {}", s.rows),
    ];
    if let Some(cwd) = &s.cwd {
        lines.push(format!("Set Cwd {}", quote(cwd)));
    }
    if s.clean_env {
        lines.push("Set CleanEnv on".into());
    }
    for (k, v) in &s.env {
        lines.push(format!("Set Env {}", quote(&format!("{k}={v}"))));
    }
    lines.push(format!("Spawn {}", s.argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")));

    let steps = group(s);
    // the screen right before step k starts, and right before step k+1
    let before_of = |idx: usize| -> &Screen { &s.inputs[idx].before };
    let blank = Screen::blank(s.cols, s.rows);
    // wait for the program to be ready before the first input
    let first_t = steps.first().map_or(s.end, |st| st.1);
    let first_screen = steps.first().map_or(&s.last, |st| before_of(st.2));
    let first_quiet = steps.first().map_or(&s.last_quiet, |st| &s.inputs[st.2].quiet);
    push_sync(&mut lines, &blank, first_screen, &union(&s.volatile, first_quiet), first_t);

    let mut resizes = s.resizes.iter().peekable();
    let mut out: Vec<String> = Vec::new();
    for (k, (step, t, idx)) in steps.iter().enumerate() {
        while let Some((rt, c, r)) = resizes.peek() {
            if rt <= t {
                out.push(fmt(&Out::Resize(*c, *r)));
                resizes.next();
            } else {
                break;
            }
        }
        out.push(fmt(step));
        let (next_t, after, quiet) = match steps.get(k + 1) {
            Some((_, nt, nidx)) => (*nt, before_of(*nidx), &s.inputs[*nidx].quiet),
            None => (s.end, &s.last, &s.last_quiet),
        };
        let gap = next_t - t;
        if k + 1 == steps.len() && s.last.text().trim().is_empty() {
            break; // the program closed its screen: nothing to wait for
        }
        push_sync(&mut out, before_of(*idx), after, &union(&s.volatile, quiet), gap);
    }
    lines.extend(merge_keys(out));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Both masks (sizes may differ after a resize: then the first wins).
fn union(a: &Mask, b: &Mask) -> Mask {
    let mut m = a.clone();
    if a.cols == b.cols && a.rows == b.rows {
        m.union(b);
    }
    m
}

fn push_sync(out: &mut Vec<String>, before: &Screen, after: &Screen, volatile: &Mask, gap: f64) {
    if before.cells != after.cells
        && let Some(w) = distinctive(before, after, volatile)
    {
        out.push(format!("Wait {w}"));
        return;
    }
    if gap >= 0.25 {
        out.push(format!("Sleep {:.2}", gap.min(3.0)));
    }
}

fn fmt(o: &Out) -> String {
    match o {
        Out::Type(t) => format!("Type {}", type_args(t)),
        Out::Key(k, 1) => format!("Key {}", quote(k)),
        Out::Key(k, n) => format!("Key {}*{n}", quote(k)),
        Out::Resize(c, r) => format!("Resize {c} {r}"),
        Out::Comment(c) => format!("# {c}"),
    }
}

/// `Key down` x3 with nothing in between -> `Key down*3`.
fn merge_keys(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if let (Some(prev), Some(k)) = (out.last_mut(), l.strip_prefix("Key "))
            && !k.contains(' ')
            && !k.contains('*')
            && let Some(pk) = prev.strip_prefix("Key ")
        {
            let (name, n) = match pk.rsplit_once('*') {
                Some((nm, n)) => (nm.to_string(), n.parse::<u32>().unwrap_or(1)),
                None => (pk.to_string(), 1),
            };
            if name == k && !name.contains(' ') {
                *prev = format!("Key {name}*{}", n + 1);
                continue;
            }
        }
        out.push(l);
    }
    out
}
