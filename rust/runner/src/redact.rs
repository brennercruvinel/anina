//! Redaction before anything is written: home paths, user and host names,
//! tokens, and whatever `[redact]` in `anina.toml` names. Applied to every
//! screen a command reads (the recorder's, explore's, init's) and to the
//! text of reports, so none of it reaches a cast, a PNG, an SVG or a JSON.
//!
//! ```toml
//! # ./anina.toml, or ~/.config/anina/config.toml (both apply, this one first)
//! [redact]
//! replace = { 'ChatGPT\.app' = "Notes.app" }  # regex = example text
//! mask = ['/Applications/[^/ ]+\.app']          # regex, shown as ••• cell for cell
//! ```
//!
//! A replacement keeps the grid: the text after a match, up to the next gap
//! (two blank cells, a box-drawing border or the row end), slides left or
//! right so columns and borders beyond it stay where the program drew them.

use crate::engine::{Engine, Modes, Spec};
use anina_screen::{Cell, Screen};
use anyhow::{Context, Result};
use regex::{Captures, Regex};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug)]
pub struct Rule {
    pub re: Regex,
    /// Replacement; `$1`, `${name}` expand from the match's groups. `None`
    /// masks the match with `•`, one per cell.
    pub with: Option<String>,
    /// What the summary calls what this rule hid; empty is not counted.
    pub label: &'static str,
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

/// The redactor every command starts from, built once by `init`.
static STANDARD: OnceLock<Redactor> = OnceLock::new();

/// What was hidden this run, by label: hashes of the distinct texts.
static SEEN: LazyLock<Mutex<BTreeMap<&'static str, HashSet<u64>>>> = LazyLock::new(Default::default);

/// The folders this command writes to. When it ends, worked or failed,
/// `anina` checks what was written there: a failed wait leaves its screen
/// in `failed.txt` too.
static OUTPUTS: LazyLock<Mutex<Vec<PathBuf>>> = LazyLock::new(Default::default);

/// Note `dir` as a folder this command writes to, once it exists.
pub fn writes_to(dir: &Path) {
    if let Ok(mut v) = OUTPUTS.lock()
        && !v.iter().any(|d| d == dir)
    {
        v.push(dir.to_path_buf());
    }
}

/// The folders noted by `writes_to`, in order.
pub fn outputs() -> Vec<PathBuf> {
    OUTPUTS.lock().map(|v| v.clone()).unwrap_or_default()
}

/// Load the config files and this machine's identity once, so every
/// `Redactor::defaults()` after it has them. A broken config is an error here,
/// before anything runs.
pub fn init() -> Result<()> {
    let mut r = Redactor::default();
    for path in config_paths() {
        r.load_config(&path)?;
    }
    r.rules.extend(Redactor::identity().rules);
    let _ = STANDARD.set(r);
    Ok(())
}

/// `./anina.toml` and the user config, the ones that exist; `$ANINA_CONFIG`
/// alone when set.
pub fn config_paths() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("ANINA_CONFIG") {
        return vec![PathBuf::from(p)];
    }
    let user = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("anina/config.toml"));
    std::iter::once(PathBuf::from("anina.toml")).chain(user).filter(|p| p.is_file()).collect()
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    redact: RedactConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactConfig {
    #[serde(default)]
    replace: BTreeMap<String, String>,
    #[serde(default)]
    mask: Vec<String>,
}

/// One line of what was hidden this run (`home path ×1, user name ×1`), if
/// anything was.
pub fn summary() -> Option<String> {
    let seen = SEEN.lock().ok()?;
    let parts: Vec<String> =
        seen.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k} ×{}", v.len())).collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn note(label: &'static str, text: &str) {
    if label.is_empty() {
        return;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    if let Ok(mut seen) = SEEN.lock() {
        seen.entry(label).or_default().insert(h.finish());
    }
}

impl Redactor {
    /// The rules every run gets: the config's, this machine's identity and
    /// secret shapes.
    pub fn defaults() -> Self {
        Self::shared().clone()
    }

    /// The same rules, borrowed: for redacting text as it is written.
    pub fn shared() -> &'static Self {
        STANDARD.get_or_init(Self::identity)
    }

    /// This machine's identity and secret shapes, without any config.
    pub fn identity() -> Self {
        let mut r = Redactor::default();
        if let Some(home) = std::env::var("HOME").ok().filter(|h| h.len() > 1) {
            r.literal_as("home path", home.trim_end_matches('/'), "~");
        }
        for (k, v) in identity() {
            match k {
                Who::User => r.word_as("user name", &v, "user"),
                Who::Host => r.word_as("host name", &v, "host"),
                Who::Name => r.word_as("git name", &v, "Your Name"),
                Who::Email => r.literal_as("git email", &v, "you@example.com"),
            }
        }
        for (pat, with) in SECRETS {
            r.add("secret", pat, Some(with)).expect("built-in secret pattern");
        }
        r
    }

    /// Add the `[redact]` rules of a config file.
    pub fn load_config(&mut self, path: &std::path::Path) -> Result<()> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: ConfigFile =
            toml::from_str(&text).with_context(|| format!("{} is not a valid config", path.display()))?;
        for (pat, with) in &cfg.redact.replace {
            self.add("config rule", pat, Some(with)).with_context(|| path.display().to_string())?;
        }
        for pat in &cfg.redact.mask {
            self.add("config rule", pat, None).with_context(|| path.display().to_string())?;
        }
        Ok(())
    }

    /// Add a regex rule.
    pub fn push(&mut self, pattern: &str, with: &str) -> Result<()> {
        self.add("rule", pattern, Some(with))
    }

    /// Add a regex rule: replaced by `with`, or masked when it is `None`.
    pub fn add(&mut self, label: &'static str, pattern: &str, with: Option<&str>) -> Result<()> {
        let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("redact pattern {pattern:?}: {e}"))?;
        // a replacement its own pattern matches would loop forever
        if let Some(with) = with
            && re.is_match(with)
            && !with.is_empty()
        {
            anyhow::bail!("redact: the replacement {with:?} matches its own pattern {pattern:?}");
        }
        self.rules.push(Rule { re, with: with.map(String::from), label });
        Ok(())
    }

    /// Replace an exact text anywhere.
    pub fn literal(&mut self, text: &str, with: &str) {
        self.literal_as("", text, with);
    }

    fn literal_as(&mut self, label: &'static str, text: &str, with: &str) {
        if text.is_empty() || text == with {
            return;
        }
        let _ = self.add(label, &regex::escape(text), Some(&with.replace('$', "$$")));
    }

    /// Replace an exact text only as a whole word, so a short user name does
    /// not eat the middle of other words.
    fn word_as(&mut self, label: &'static str, text: &str, with: &str) {
        if text.trim().is_empty() || text == with {
            return;
        }
        let _ = self.add(label, &format!(r"(?:^|\b){}(?:\b|$)", regex::escape(text)), Some(&with.replace('$', "$$")));
    }

    /// Redact plain text: a message, a path, a field of a report.
    pub fn text(&self, s: &str) -> String {
        self.redact_text(s, true)
    }

    /// Redact for the terminal (an error, a status line) without counting:
    /// the summary keeps telling what the written files had.
    pub fn quiet(&self, s: &str) -> String {
        self.redact_text(s, false)
    }

    fn redact_text(&self, s: &str, count: bool) -> String {
        let mut out = s.to_string();
        for rule in &self.rules {
            out = rule
                .re
                .replace_all(&out, |caps: &Captures| {
                    let with = rule.replacement(caps);
                    if count && with != caps[0] {
                        note(rule.label, &caps[0]);
                    }
                    with
                })
                .into_owned();
        }
        out
    }

    /// What in `text` these rules would hide, as (label, byte range): only
    /// counted rules, so a sandbox path is not a leak.
    pub fn leaks(&self, text: &str) -> Vec<(&'static str, std::ops::Range<usize>)> {
        let mut out = Vec::new();
        for rule in self.rules.iter().filter(|r| !r.label.is_empty()) {
            for caps in rule.re.captures_iter(text) {
                let m = caps.get(0).expect("group 0");
                if !m.as_str().is_empty() && rule.replacement(&caps) != m.as_str() {
                    out.push((rule.label, m.range()));
                }
            }
        }
        // one finding per place: `/Users/me` is a home path, not also a user name
        out.sort_by_key(|(_, r)| (r.start, std::cmp::Reverse(r.end)));
        let mut end = 0;
        out.retain(|(_, r)| {
            let keep = r.start >= end;
            end = end.max(r.end);
            keep
        });
        out
    }

    /// Redact every string in a JSON value (keys stay).
    pub fn json(&self, v: &mut serde_json::Value) {
        match v {
            serde_json::Value::String(s) => *s = self.text(s),
            serde_json::Value::Array(a) => a.iter_mut().for_each(|x| self.json(x)),
            serde_json::Value::Object(o) => o.values_mut().for_each(|x| self.json(x)),
            _ => {}
        }
    }

    /// `value` as pretty JSON with every string redacted.
    pub fn to_json(&self, value: &impl serde::Serialize) -> Result<String> {
        let mut v = serde_json::to_value(value)?;
        self.json(&mut v);
        Ok(serde_json::to_string_pretty(&v)?)
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
    /// What a match becomes: the expanded replacement, or as many `•` as it
    /// takes cells.
    fn replacement(&self, caps: &Captures) -> String {
        match &self.with {
            Some(w) => {
                let mut out = String::new();
                caps.expand(w, &mut out);
                out
            }
            None => "•".repeat(caps[0].chars().map(|c| c.width().unwrap_or(0)).sum::<usize>().max(1)),
        }
    }

    /// The first match in `text` as a cell range and its replacement.
    fn find(&self, text: &str, at: &[u16], cells: &[Cell]) -> Option<(usize, usize, String)> {
        let caps = self.re.captures(text)?;
        let m = caps.get(0)?;
        if m.as_str().is_empty() {
            return None;
        }
        let with = self.replacement(&caps);
        if with == m.as_str() {
            return None;
        }
        note(self.label, m.as_str());
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

/// An engine whose screens come out redacted: what a command reads is what it
/// may write.
pub struct Redacted {
    inner: Box<dyn Engine>,
    red: Redactor,
}

impl Redacted {
    pub fn new(inner: Box<dyn Engine>, red: Redactor) -> Self {
        Redacted { inner, red }
    }
}

impl Engine for Redacted {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn spawn(&mut self, spec: &Spec) -> Result<()> {
        self.inner.spawn(spec)
    }
    fn write(&mut self, data: &str) -> Result<()> {
        self.inner.write(data)
    }
    fn key(&mut self, key: &str) -> Result<()> {
        self.inner.key(key)
    }
    fn screen(&mut self) -> Result<Screen> {
        let mut s = self.inner.screen()?;
        self.red.apply(&mut s);
        Ok(s)
    }
    fn modes(&mut self) -> Result<Modes> {
        self.inner.modes()
    }
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.inner.resize(cols, rows)
    }
    fn exit_code(&mut self) -> Result<Option<i32>> {
        self.inner.exit_code()
    }
    fn close(&mut self) -> Result<()> {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Redactor {
        let mut r = Redactor::default();
        r.literal_as("home path", "/Users/alice", "~");
        r.word_as("user name", "alice", "user");
        r.add("config rule", r"/Applications/Secret[^ ]*", None).unwrap();
        r.add("config rule", r"Dia\.app", Some("Browser.app")).unwrap();
        r
    }

    #[test]
    fn a_mask_keeps_the_width_and_the_columns_after_it() {
        let mut s = anina_screen::escape::parse("/Applications/Secret.app/x      |end\r\n", 40, 2);
        red().apply(&mut s);
        assert_eq!(s.row_text(0).trim_end(), "••••••••••••••••••••••••••      |end");
    }

    #[test]
    fn a_replacement_slides_the_text_but_not_the_next_column() {
        let mut s = anina_screen::escape::parse("/Users/alice/notes   |end\r\n", 40, 2);
        red().apply(&mut s);
        let row = s.row_text(0);
        assert!(row.starts_with("~/notes"), "{row:?}");
        assert_eq!(row.find("|end"), Some(21), "{row:?}");
    }

    #[test]
    fn a_user_name_goes_only_as_a_whole_word() {
        assert_eq!(red().text("alice ran malice"), "user ran malice");
    }

    #[test]
    fn text_and_json_are_redacted() {
        let r = red();
        assert_eq!(r.text("open /Users/alice/Dia.app"), "open ~/Browser.app");
        let mut v = serde_json::json!({"tape": "/Users/alice/t.tape", "n": 1, "list": ["alice"]});
        r.json(&mut v);
        assert_eq!(v, serde_json::json!({"tape": "~/t.tape", "n": 1, "list": ["user"]}));
    }

    #[test]
    fn a_leak_is_reported_once_per_place() {
        let r = red();
        let leaks = r.leaks("pid 1 alice /Users/alice/x");
        let labels: Vec<&str> = leaks.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["user name", "home path"]);
        assert!(r.leaks("pid 1 user ~/x").is_empty());
    }

    #[test]
    fn uncounted_rules_are_not_leaks() {
        let mut r = Redactor::default();
        r.literal("/tmp/sandbox", "/sandbox");
        assert!(r.leaks("/tmp/sandbox/work").is_empty());
    }

    #[test]
    fn a_replacement_its_pattern_matches_is_refused() {
        assert!(Redactor::default().push("a+", "aa").is_err());
    }

    #[test]
    fn the_config_adds_replace_and_mask_rules() {
        let dir = std::env::temp_dir().join(format!("anina-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("anina.toml");
        std::fs::write(&p, "[redact]\nreplace = { 'Zed\\.app' = \"Editor.app\" }\nmask = ['tok-[0-9]+']\n").unwrap();
        let mut r = Redactor::default();
        r.load_config(&p).unwrap();
        assert_eq!(r.text("Zed.app tok-1234"), "Editor.app ••••••••");
        std::fs::write(&p, "[redact]\nmasks = ['x']\n").unwrap();
        assert!(Redactor::default().load_config(&p).is_err(), "a typo in a key is an error");
        let _ = std::fs::remove_file(&p);
    }
}
