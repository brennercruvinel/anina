//! Terminal input bytes -> what a tape can say: typed text or named keys.
//! Reads what terminals send in raw mode: control bytes, ESC-prefixed alt
//! keys, CSI and SS3 sequences (with xterm modifiers), kitty's CSI-u,
//! bracketed paste. Mouse reports are dropped (tapes have no mouse steps).

/// One thing the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ev {
    /// Printable text (typed or pasted).
    Text(String),
    /// A key in the `mod+key` form `engine::normalize_key` produces.
    Key(String),
    /// Bytes no tape step can express (shown as a comment).
    Unknown(String),
}

/// Decodes a byte stream that may arrive split across reads: an incomplete
/// escape sequence waits in `pending` until more bytes come or `flush`.
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
    paste: Option<Vec<u8>>,
}

const PASTE_END: &[u8] = b"\x1b[201~";

impl Decoder {
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Decode `bytes`; an unfinished sequence at the end is kept for later.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Ev> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if let Some(buf) = &mut self.paste {
                // inside a bracketed paste everything is text until the end marker
                match find(&self.pending, PASTE_END) {
                    Some(i) => {
                        buf.extend_from_slice(&self.pending[..i]);
                        let text = String::from_utf8_lossy(buf).into_owned();
                        self.pending.drain(..i + PASTE_END.len());
                        self.paste = None;
                        push_text(&mut out, &text);
                        continue;
                    }
                    None => {
                        // keep a tail that could be the start of the end marker
                        let keep =
                            (1..PASTE_END.len()).rev().find(|k| self.pending.ends_with(&PASTE_END[..*k])).unwrap_or(0);
                        let cut = self.pending.len() - keep;
                        buf.extend_from_slice(&self.pending[..cut]);
                        self.pending.drain(..cut);
                        return out;
                    }
                }
            }
            if self.pending.is_empty() {
                return out;
            }
            match decode_one(&self.pending) {
                Step::Need => return out,
                Step::Got(n, ev) => {
                    self.pending.drain(..n);
                    match ev {
                        Some(Ev::Text(t)) => push_text(&mut out, &t),
                        Some(Ev::Key(k)) if k == "\x00paste" => self.paste = Some(Vec::new()),
                        Some(e) => out.push(e),
                        None => {}
                    }
                }
            }
        }
    }

    /// End of input for now: a lone ESC is the escape key, any other
    /// leftover is decoded as far as it goes.
    pub fn flush(&mut self) -> Vec<Ev> {
        let mut out = Vec::new();
        while !self.pending.is_empty() {
            if self.pending[0] == 0x1b {
                out.push(Ev::Key("esc".into()));
                self.pending.remove(0);
                let rest = std::mem::take(&mut self.pending);
                out.extend(self.feed(&rest));
                if self.has_pending() {
                    continue;
                }
            } else {
                let rest = std::mem::take(&mut self.pending);
                out.push(Ev::Unknown(escape_bytes(&rest)));
            }
        }
        out
    }
}

fn push_text(out: &mut Vec<Ev>, t: &str) {
    if let Some(Ev::Text(prev)) = out.last_mut() {
        prev.push_str(t);
    } else {
        out.push(Ev::Text(t.to_string()));
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

enum Step {
    /// More bytes needed.
    Need,
    /// Consumed n bytes, producing an event (or nothing, for ignored input).
    Got(usize, Option<Ev>),
}

/// Bytes shown in a comment: printable as is, the rest as \xNN.
pub fn escape_bytes(b: &[u8]) -> String {
    b.iter()
        .map(|c| if c.is_ascii_graphic() || *c == b' ' { (*c as char).to_string() } else { format!("\\x{c:02x}") })
        .collect()
}

fn key(s: &str) -> Option<Ev> {
    Some(Ev::Key(s.to_string()))
}

/// A control byte (other than ESC) as a key.
fn control(b: u8) -> String {
    match b {
        b'\r' => "enter".into(),
        b'\t' => "tab".into(),
        0x7f => "backspace".into(),
        0x00 => "ctrl+space".into(),
        0x01..=0x1a => format!("ctrl+{}", (b'a' + b - 1) as char),
        0x1c => "ctrl+\\".into(),
        0x1d => "ctrl+]".into(),
        0x1e => "ctrl+^".into(),
        0x1f => "ctrl+_".into(),
        _ => format!("\\x{b:02x}"),
    }
}

/// xterm modifier parameter (1 + bits: shift 1, alt 2, ctrl 4, super 8).
fn mods(m: u32) -> String {
    let bits = m.saturating_sub(1);
    let mut s = String::new();
    for (bit, name) in [(4, "ctrl"), (2, "alt"), (1, "shift"), (8, "super")] {
        if bits & bit != 0 {
            s.push_str(name);
            s.push('+');
        }
    }
    s
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

fn decode_one(b: &[u8]) -> Step {
    let c = b[0];
    if c == 0x1b {
        return decode_esc(b);
    }
    if c < 0x20 || c == 0x7f {
        return Step::Got(1, key(&control(c)));
    }
    let n = utf8_len(c);
    if b.len() < n {
        return Step::Need;
    }
    match std::str::from_utf8(&b[..n]) {
        Ok(s) => Step::Got(n, Some(Ev::Text(s.to_string()))),
        Err(_) => Step::Got(1, Some(Ev::Unknown(escape_bytes(&b[..1])))),
    }
}

fn decode_esc(b: &[u8]) -> Step {
    let Some(&next) = b.get(1) else {
        return Step::Need; // lone ESC: the key, or the start of a sequence (flush decides)
    };
    match next {
        b'[' => decode_csi(b),
        b'O' => match b.get(2) {
            None => Step::Need,
            Some(&f) => Step::Got(3, ss3_final(f, "")),
        },
        0x1b => Step::Got(1, key("esc")), // ESC ESC: the first is the escape key
        _ => {
            // ESC + key = alt+key
            match decode_one(&b[1..]) {
                Step::Need => Step::Need,
                Step::Got(n, Some(Ev::Text(t))) if t.chars().count() == 1 => {
                    let ch = t.chars().next().unwrap_or(' ');
                    let name = if ch.is_ascii_uppercase() {
                        format!("alt+shift+{}", ch.to_ascii_lowercase())
                    } else {
                        format!("alt+{ch}")
                    };
                    Step::Got(1 + n, Some(Ev::Key(name)))
                }
                Step::Got(n, Some(Ev::Key(k))) => Step::Got(1 + n, Some(Ev::Key(format!("alt+{k}")))),
                Step::Got(n, _) => Step::Got(1 + n, Some(Ev::Unknown(escape_bytes(&b[..1 + n])))),
            }
        }
    }
}

fn ss3_final(f: u8, m: &str) -> Option<Ev> {
    let name = match f {
        b'A' => "up",
        b'B' => "down",
        b'C' => "right",
        b'D' => "left",
        b'H' => "home",
        b'F' => "end",
        b'P' => "f1",
        b'Q' => "f2",
        b'R' => "f3",
        b'S' => "f4",
        b'M' => "enter", // keypad enter in application mode
        _ => return Some(Ev::Unknown(format!("\\x1bO{}", f as char))),
    };
    Some(Ev::Key(format!("{m}{name}")))
}

fn decode_csi(b: &[u8]) -> Step {
    // ESC [ params intermediates final
    let mut i = 2;
    // old X10 mouse: ESC [ M cb cx cy
    if b.get(2) == Some(&b'M') {
        return if b.len() < 6 { Step::Need } else { Step::Got(6, None) };
    }
    while i < b.len() && (0x30..=0x3f).contains(&b[i]) {
        i += 1;
    }
    while i < b.len() && (0x20..=0x2f).contains(&b[i]) {
        i += 1;
    }
    if i >= b.len() {
        return Step::Need;
    }
    let fin = b[i];
    let len = i + 1;
    if !(0x40..=0x7e).contains(&fin) {
        return Step::Got(len, Some(Ev::Unknown(escape_bytes(&b[..len]))));
    }
    let params = String::from_utf8_lossy(&b[2..i]).into_owned();
    let raw = || Some(Ev::Unknown(escape_bytes(&b[..len])));
    if params.starts_with('<') {
        return Step::Got(len, None); // SGR mouse report
    }
    if params.starts_with('?') || params.starts_with('>') {
        return Step::Got(len, raw()); // replies to queries, not keys
    }
    let nums: Vec<Vec<u32>> =
        params.split(';').map(|p| p.split(':').map(|n| n.parse().unwrap_or(0)).collect()).collect();
    let p0 = nums.first().and_then(|p| p.first()).copied().unwrap_or(0);
    let m = nums.get(1).and_then(|p| p.first()).map(|m| mods(*m)).unwrap_or_default();
    let ev = match fin {
        b'A' | b'B' | b'C' | b'D' | b'H' | b'F' | b'P' | b'Q' | b'S' => ss3_final(fin, &m),
        b'R' if !params.is_empty() && nums.len() == 2 && m.is_empty() => return Step::Got(len, raw()), // cursor report
        b'R' => ss3_final(fin, &m),
        b'Z' => key("shift+tab"),
        b'I' | b'O' if params.is_empty() => None, // focus in / out
        b'~' => {
            let name = match p0 {
                1 | 7 => "home",
                2 => "insert",
                3 => "delete",
                4 | 8 => "end",
                5 => "pageup",
                6 => "pagedown",
                11 => "f1",
                12 => "f2",
                13 => "f3",
                14 => "f4",
                15 => "f5",
                17 => "f6",
                18 => "f7",
                19 => "f8",
                20 => "f9",
                21 => "f10",
                23 => "f11",
                24 => "f12",
                200 => return Step::Got(len, Some(Ev::Key("\x00paste".into()))),
                201 => return Step::Got(len, None),
                _ => return Step::Got(len, raw()),
            };
            Some(Ev::Key(format!("{m}{name}")))
        }
        b'u' => kitty(p0, &m),
        _ => raw(),
    };
    Step::Got(len, ev)
}

/// kitty keyboard protocol: CSI code ; mods u
fn kitty(code: u32, m: &str) -> Option<Ev> {
    let name = match code {
        13 => "enter".to_string(),
        9 => "tab".into(),
        27 => "esc".into(),
        127 => "backspace".into(),
        32 => "space".into(),
        c => match char::from_u32(c) {
            Some(ch) if !ch.is_control() => {
                if m.is_empty() {
                    return Some(Ev::Text(ch.to_string()));
                }
                ch.to_string()
            }
            _ => return Some(Ev::Unknown(format!("\\x1b[{c}u"))),
        },
    };
    if name == "tab" && m == "shift+" {
        return key("shift+tab");
    }
    Some(Ev::Key(format!("{m}{name}")))
}
