//! Key hints as TUIs print them: footers (`enter begin · q quit`), help
//! panels (`Quit : q`), popups (`[s] Stop`), menus (`(q)uit`, `<q> quit`),
//! function-key bars (`F1Help  F2Setup`) and nano-style bars (`^G Help`).
//! Nothing here knows a program: a row counts as a key bar only when most of
//! its pieces read as `key label`.

use crate::engine::normalize_key;
use crate::screen::Screen;
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hint {
    /// Normalized (`ctrl+q`, `up`, `shift+tab`, `q`).
    pub key: String,
    pub label: String,
    /// Screen row it was read from.
    pub row: u16,
}

const NAMED: &[&str] = &[
    "enter",
    "return",
    "esc",
    "escape",
    "tab",
    "btab",
    "space",
    "backspace",
    "bksp",
    "del",
    "delete",
    "ins",
    "insert",
    "home",
    "end",
    "pgup",
    "pgdn",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
];

fn arrow(c: char) -> Option<&'static str> {
    match c {
        '↑' | '⬆' | '▲' => Some("up"),
        '↓' | '⬇' | '▼' => Some("down"),
        '←' | '⬅' | '◀' => Some("left"),
        '→' | '➡' | '▶' => Some("right"),
        _ => None,
    }
}

/// Characters that are drawing, not keys.
fn decorative(c: char) -> bool {
    ('\u{2500}'..='\u{259f}').contains(&c) // box drawing, blocks
        || ('\u{2800}'..='\u{28ff}').contains(&c) // braille
        || "●○•·…◆◇■□✓✔✗✘★☆".contains(c)
}

/// The keys one word names, normalized; empty when it is not a key.
/// `↑↓` -> up, down; `pgup/pgdn` -> pageup, pagedown; `1-9` -> 1;
/// `^C`, `C-c`, `ctrl+c`, `⌃c` -> ctrl+c; `F5`; `q`.
pub fn key_token(tok: &str) -> Vec<String> {
    let tok = tok.trim();
    if tok.is_empty() {
        return vec![];
    }
    let chars: Vec<char> = tok.chars().collect();
    if chars.iter().all(|c| arrow(*c).is_some()) {
        let mut v: Vec<String> = chars.iter().filter_map(|c| arrow(*c)).map(String::from).collect();
        v.dedup();
        return v;
    }
    if chars.len() == 3 && chars[0].is_ascii_digit() && chars[1] == '-' && chars[2].is_ascii_digit() {
        return vec![chars[0].to_string()]; // a range: `1-9 recent` tries the first
    }
    if tok.contains('/') && chars.len() > 1 {
        let parts: Vec<&str> = tok.split('/').collect();
        if parts.iter().any(|p| p.is_empty() || p.chars().all(|c| c.is_ascii_digit())) {
            return vec![]; // `1/5`, `14592/320000`: counters, not keys
        }
        let keys: Vec<Vec<String>> = parts.iter().map(|p| single_key(p).into_iter().collect()).collect();
        if keys.iter().all(|k| !k.is_empty()) {
            return keys.into_iter().flatten().collect();
        }
        return vec![];
    }
    single_key(tok).into_iter().collect()
}

fn single_key(tok: &str) -> Option<String> {
    let chars: Vec<char> = tok.chars().collect();
    let lower = tok.to_ascii_lowercase();
    if chars.len() == 2 && (chars[0] == '^' || chars[0] == '⌃') && !chars[1].is_whitespace() {
        return Some(format!("ctrl+{}", chars[1].to_lowercase()));
    }
    if chars.len() == 2 && chars[0] == '⌥' {
        return Some(format!("alt+{}", chars[1].to_lowercase()));
    }
    if chars.len() == 2 && chars[0] == '⇧' {
        return single_key(&chars[1].to_string()).map(|k| format!("shift+{k}"));
    }
    if let Some(caps) = MODIFIED.captures(tok) {
        let rest = &caps[2];
        let base = single_key(rest)?;
        let m = caps[1].to_ascii_lowercase();
        // one-letter modifiers only in the tmux/emacs spelling: C-x, M-x, S-tab
        if m.len() == 1 && !tok.contains('-') {
            return None;
        }
        return Some(normalize_key(&format!("{m}+{base}")));
    }
    if FKEY.is_match(tok) {
        return Some(lower);
    }
    if NAMED.contains(&lower.as_str()) {
        return Some(normalize_key(&lower));
    }
    if chars.len() == 1 {
        let c = chars[0];
        if let Some(a) = arrow(c) {
            return Some(a.into());
        }
        if !c.is_whitespace() && !c.is_control() && !decorative(c) && (c.is_alphanumeric() || c.is_ascii_punctuation())
        {
            return Some(c.to_string());
        }
    }
    None
}

static MODIFIED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(ctrl|control|alt|meta|shift|super|cmd|option|opt|c|m|s)[+-](.+)$").unwrap());
static FKEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[Ff]([1-9]|1[0-2])$").unwrap());
/// `[s] Stop the training`, `<q> quit`: anywhere on a row.
static BRACKETED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\[<]([^\s\[\]<>]{1,8})[\]>][ :]+([A-Za-z][^\[\]<>│┃║]*?)(?:\s{2,}|[│┃║]|$)").unwrap()
});
/// `(q)uit`, `(Y)es`
static PAREN_INITIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(([A-Za-z0-9])\)([a-z]{2,})").unwrap());
/// `(Y/n)`, `[y/N]` prompts
static YES_NO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\(\[]([YyNn])/([YyNn])[\)\]]").unwrap());
/// htop: `F1Help  F2Setup  F3SearchF4Filter` (a key glued to its label)
static FKEY_BAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"F(1[0-2]|[1-9])[A-Za-z]").unwrap());
/// Pieces of a footer: ` · `, ` | `, ` • `, or a gap of 2+ spaces.
static SEPARATOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[·•∙|]\s*|\s{2,}").unwrap());

/// Read `key label` from one footer piece.
fn piece(seg: &str) -> Option<(Vec<String>, String)> {
    let seg = seg.trim();
    if seg.is_empty() {
        return None;
    }
    let words: Vec<&str> = seg.split_whitespace().collect();
    // `q: quit`, `q=quit`
    if let Some((k, rest)) = words[0].split_once([':', '=']) {
        let keys = key_token(k);
        let label = format!("{} {}", rest, words[1..].join(" "));
        if !keys.is_empty() && good_label(&label) {
            return Some((keys, clean_label(&label)));
        }
    }
    let mut keys = key_token(words[0]);
    if keys.is_empty() {
        return None;
    }
    // `⬅ ➡ switch`: more arrow words after an arrow word
    let mut i = 1;
    while i < words.len()
        && words[i].chars().all(|c| arrow(c).is_some())
        && words[i - 1].chars().all(|c| arrow(c).is_some())
    {
        keys.extend(key_token(words[i]));
        i += 1;
    }
    let label = words[i..].join(" ");
    good_label(&label).then(|| (keys, clean_label(&label)))
}

fn good_label(label: &str) -> bool {
    let l = label.trim();
    let words = l.split_whitespace().count();
    l.chars().next().is_some_and(|c| c.is_alphabetic()) && l.chars().count() <= 40 && (1..=6).contains(&words)
}

fn clean_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', ':', ',']).to_string()
}

/// Every key hint on a screen, first occurrence of each key wins.
pub fn parse(screen: &Screen) -> Vec<Hint> {
    let mut out: Vec<Hint> = Vec::new();
    for y in 0..screen.rows {
        let row = screen.row_text(y);
        for (key, label) in parse_row(&row) {
            if !out.iter().any(|h| h.key == key) {
                out.push(Hint { key, label, row: y });
            }
        }
    }
    out
}

/// The `(key, label)` pairs on one row of text.
pub fn parse_row(row: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut push = |keys: Vec<String>, label: &str| {
        for k in keys {
            out.push((k, clean_label(label)));
        }
    };
    for caps in BRACKETED.captures_iter(row) {
        // `[x] payload`, `[*] verify`: checkboxes, not keys
        let whole = caps.get(0).unwrap().as_str();
        if whole.starts_with('[') && matches!(&caps[1], "x" | "X" | "*" | "-" | "+" | "~" | "=" | "✓" | "✔") {
            continue;
        }
        let keys = key_token(&caps[1]);
        if !keys.is_empty() && good_label(&caps[2]) {
            push(keys, &caps[2]);
        }
    }
    for caps in PAREN_INITIAL.captures_iter(row) {
        push(key_token(&caps[1]), &format!("{}{}", &caps[1], &caps[2]));
    }
    for caps in YES_NO.captures_iter(row) {
        for k in [&caps[1], &caps[2]] {
            let label = if k.eq_ignore_ascii_case("y") { "yes" } else { "no" };
            push(vec![k.to_ascii_lowercase()], label);
        }
    }
    let fbar: Vec<_> = FKEY_BAR.captures_iter(row).collect();
    if fbar.len() >= 3 {
        for (i, caps) in fbar.iter().enumerate() {
            let m = caps.get(0).unwrap();
            let start = m.start() + 1 + caps[1].len();
            let end = fbar.get(i + 1).map_or(row.len(), |n| n.get(0).unwrap().start());
            let label = row[start..end].trim();
            if !label.is_empty() {
                push(vec![format!("f{}", &caps[1])], label);
            }
        }
        return out;
    }
    // panels side by side: read each between vertical borders
    for chunk in row.split(['│', '┃', '║']) {
        let chunk = chunk.trim_matches(|c: char| c.is_whitespace() || decorative(c));
        if chunk.is_empty() {
            continue;
        }
        if let Some((keys, label)) = help_line(chunk) {
            push(keys, &label);
            continue;
        }
        let pieces: Vec<&str> = SEPARATOR.split(chunk).filter(|p| !p.trim().is_empty()).collect();
        let parsed: Vec<(Vec<String>, String)> = pieces.iter().filter_map(|p| piece(p)).collect();
        // a key bar: at least two pieces and most of them `key label`
        if parsed.len() >= 2 && parsed.len() * 10 >= pieces.len() * 6 {
            for (keys, label) in parsed {
                push(keys, &label);
            }
        }
    }
    out
}

/// `Quit          : q    Stop the training.` (a help panel line: a short
/// label, a colon, the keys, then an optional description after a gap).
fn help_line(chunk: &str) -> Option<(Vec<String>, String)> {
    let (label, rest) = chunk.split_once(':')?;
    let label = label.trim();
    if !(1..=3).contains(&label.split_whitespace().count()) || !label.chars().all(|c| c.is_alphabetic() || c == ' ') {
        return None;
    }
    let rest = rest.trim_start();
    let keys_part = rest.split("  ").next()?.trim();
    let mut keys = Vec::new();
    for w in keys_part.split_whitespace() {
        let k = key_token(w);
        if k.is_empty() {
            return None;
        }
        keys.extend(k);
    }
    (!keys.is_empty() && good_label(label)).then(|| (keys, label.to_string()))
}

static QUIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(quit|exit|stop|abort|leave|close app|shut ?down|log ?out)\b").unwrap());
static RISKY: LazyLock<Regex> = LazyLock::new(|| {
    // stems, so `installs`, `deleting`, `saved` count too
    Regex::new(r"(?i)\b(instal|uninstal|kill|terminat|delet|remov|eras|format|wip|reset|drop|purg|destroy|overwrit|commit|push|publish|deploy|send|submit|buy|pay|appl|sav|writ|upload|download)\w{0,3}\b").unwrap()
});

/// Ends the program (tried last, only to record the edge).
pub fn is_quit(key: &str, label: &str) -> bool {
    matches!(key, "ctrl+c" | "ctrl+q" | "ctrl+d" | "ctrl+\\") || QUIT.is_match(label)
}

/// Changes the world outside the program (skipped unless allowed).
pub fn is_risky(label: &str) -> bool {
    RISKY.is_match(label)
}
