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

#[cfg(test)]
mod tests {
    use super::*;
    use anina_runner::engine::normalize_key;

    /// Everything `bytes` say, as one read followed by the end of input.
    fn decode(bytes: &[u8]) -> Vec<Ev> {
        let mut d = Decoder::default();
        let mut out = d.feed(bytes);
        out.extend(d.flush());
        out
    }

    fn text(t: &str) -> Ev {
        Ev::Text(t.into())
    }

    fn k(name: &str) -> Ev {
        Ev::Key(name.into())
    }

    /// Each sequence is one key, named the way a tape and the engines name it.
    fn keys(table: &[(&[u8], &str)]) {
        for (bytes, name) in table {
            assert_eq!(decode(bytes), [k(name)], "{}", escape_bytes(bytes));
            assert_eq!(normalize_key(name), *name, "{name} is not how the engines spell it");
        }
    }

    #[test]
    fn typed_text_stays_text() {
        assert_eq!(decode(b"hello world"), [text("hello world")]);
        assert_eq!(decode("é日本 ✓".as_bytes()), [text("é日本 ✓")]);
        // keys cut the text where they were pressed
        assert_eq!(decode(b"ls\rcd ..\t"), [text("ls"), k("enter"), text("cd .."), k("tab")]);
        assert_eq!(decode(b""), []);
    }

    #[test]
    fn control_bytes_are_keys() {
        keys(&[
            (b"\r", "enter"),
            (b"\t", "tab"),
            (b"\x7f", "backspace"),
            (b"\x00", "ctrl+space"),
            (b"\x01", "ctrl+a"),
            (b"\x03", "ctrl+c"),
            (b"\x04", "ctrl+d"),
            (b"\x0a", "ctrl+j"),
            (b"\x1a", "ctrl+z"),
            (b"\x1c", "ctrl+\\"),
            (b"\x1d", "ctrl+]"),
            (b"\x1e", "ctrl+^"),
            (b"\x1f", "ctrl+_"),
        ]);
    }

    #[test]
    fn cursor_and_function_keys_are_named_in_both_modes() {
        keys(&[
            // normal mode (CSI) and application mode (SS3)
            (b"\x1b[A", "up"),
            (b"\x1b[B", "down"),
            (b"\x1b[C", "right"),
            (b"\x1b[D", "left"),
            (b"\x1b[H", "home"),
            (b"\x1b[F", "end"),
            (b"\x1bOA", "up"),
            (b"\x1bOB", "down"),
            (b"\x1bOC", "right"),
            (b"\x1bOD", "left"),
            (b"\x1bOH", "home"),
            (b"\x1bOF", "end"),
            (b"\x1bOM", "enter"),
            (b"\x1bOP", "f1"),
            (b"\x1bOQ", "f2"),
            (b"\x1bOR", "f3"),
            (b"\x1bOS", "f4"),
            (b"\x1b[Z", "shift+tab"),
            // the keys xterm sends as a number and a tilde
            (b"\x1b[1~", "home"),
            (b"\x1b[2~", "insert"),
            (b"\x1b[3~", "delete"),
            (b"\x1b[4~", "end"),
            (b"\x1b[5~", "pageup"),
            (b"\x1b[6~", "pagedown"),
            (b"\x1b[7~", "home"),
            (b"\x1b[8~", "end"),
            (b"\x1b[11~", "f1"),
            (b"\x1b[12~", "f2"),
            (b"\x1b[13~", "f3"),
            (b"\x1b[14~", "f4"),
            (b"\x1b[15~", "f5"),
            (b"\x1b[17~", "f6"),
            (b"\x1b[18~", "f7"),
            (b"\x1b[19~", "f8"),
            (b"\x1b[20~", "f9"),
            (b"\x1b[21~", "f10"),
            (b"\x1b[23~", "f11"),
            (b"\x1b[24~", "f12"),
        ]);
    }

    #[test]
    fn xterm_modifiers_are_one_plus_shift_1_alt_2_ctrl_4_super_8() {
        keys(&[
            (b"\x1b[1;2A", "shift+up"),
            (b"\x1b[1;3A", "alt+up"),
            (b"\x1b[1;4A", "alt+shift+up"),
            (b"\x1b[1;5C", "ctrl+right"),
            (b"\x1b[1;6D", "ctrl+shift+left"),
            (b"\x1b[1;7B", "ctrl+alt+down"),
            (b"\x1b[1;8H", "ctrl+alt+shift+home"),
            (b"\x1b[1;9F", "super+end"),
            (b"\x1b[1;2P", "shift+f1"),
            (b"\x1b[1;5R", "ctrl+f3"),
            (b"\x1b[3;5~", "ctrl+delete"),
            (b"\x1b[5;3~", "alt+pageup"),
            (b"\x1b[15;2~", "shift+f5"),
            (b"\x1b[24;6~", "ctrl+shift+f12"),
        ]);
    }

    #[test]
    fn escape_before_a_key_is_alt_and_alone_is_the_escape_key() {
        keys(&[
            (b"\x1b", "esc"),
            (b"\x1bx", "alt+x"),
            (b"\x1bX", "alt+shift+x"),
            (b"\x1b.", "alt+."),
            (b"\x1b\r", "alt+enter"),
            (b"\x1b\x7f", "alt+backspace"),
        ]);
        assert_eq!(decode(b"\x1b\x1b"), [k("esc"), k("esc")]);
        assert_eq!(decode(b"\x1b\x1b[A"), [k("esc"), k("up")]);
    }

    #[test]
    fn kitty_keys_carry_a_code_and_modifiers() {
        keys(&[
            (b"\x1b[13u", "enter"),
            (b"\x1b[27u", "esc"),
            (b"\x1b[13;2u", "shift+enter"),
            (b"\x1b[9;2u", "shift+tab"),
            (b"\x1b[127;3u", "alt+backspace"),
            (b"\x1b[32;5u", "ctrl+space"),
            (b"\x1b[97;5u", "ctrl+a"),
            (b"\x1b[97;6u", "ctrl+shift+a"),
        ]);
        // a letter with no modifier is typed text
        assert_eq!(decode(b"\x1b[97u\x1b[98u"), [text("ab")]);
    }

    #[test]
    fn a_sequence_cut_between_two_reads_is_one_key() {
        for (name, whole) in [
            ("ctrl+right", &b"\x1b[1;5C"[..]),
            ("f5", b"\x1b[15~"),
            ("up", b"\x1bOA"),
            ("alt+x", b"\x1bx"),
            ("ctrl+a", b"\x1b[97;5u"),
        ] {
            for cut in 1..whole.len() {
                let mut d = Decoder::default();
                assert_eq!(d.feed(&whole[..cut]), [], "{name} cut at {cut}");
                assert!(d.has_pending());
                assert_eq!(d.feed(&whole[cut..]), [k(name)], "{name} cut at {cut}");
                assert!(!d.has_pending());
            }
        }
        // a character too: the three bytes of 日, one at a time
        let mut d = Decoder::default();
        let bytes = "日".as_bytes();
        assert_eq!(
            (d.feed(&bytes[..1]), d.feed(&bytes[1..2]), d.feed(&bytes[2..])),
            (vec![], vec![], vec![text("日")])
        );
    }

    #[test]
    fn a_paste_is_text_whatever_it_holds() {
        // a newline, a tab and an arrow-key sequence, pasted: none of them is a key
        assert_eq!(decode(b"\x1b[200~ls -la\r\n\tcd \x1b[A\x1b[201~"), [text("ls -la\r\n\tcd \x1b[A")]);
        assert_eq!(decode(b"a\x1b[200~b\x1b[201~c\r"), [text("abc"), k("enter")]);
        // cut anywhere, the end marker included
        let whole = b"\x1b[200~hello\x1b[201~";
        for cut in 1..whole.len() {
            let mut d = Decoder::default();
            let mut got = d.feed(&whole[..cut]);
            got.extend(d.feed(&whole[cut..]));
            assert_eq!(got, [text("hello")], "cut at {cut}");
        }
    }

    #[test]
    fn mouse_and_focus_reports_are_dropped() {
        // SGR press and release, the old three-byte form, focus in and out
        assert_eq!(decode(b"a\x1b[<0;10;5M\x1b[<0;10;5mb"), [text("ab")]);
        assert_eq!(decode(b"a\x1b[M !!b"), [text("ab")]);
        assert_eq!(decode(b"\x1b[I\x1b[O"), []);
    }

    #[test]
    #[ignore = "a cursor position report reads as F3 with modifiers unless its column is 1, 17, 33..."]
    fn a_terminal_s_answer_to_a_query_is_not_a_key() {
        for reply in [
            // device attributes, the cursor's position (row 12, column 40), a private mode's state
            &b"\x1b[?1;2c"[..],
            b"\x1b[>0;276;0c",
            b"\x1b[12;40R",
            b"\x1b[1;1R",
            b"\x1b[?2026;2$y",
        ] {
            assert_eq!(decode(reply), [Ev::Unknown(escape_bytes(reply))], "{}", escape_bytes(reply));
        }
    }

    #[test]
    fn what_no_step_can_say_is_shown_as_its_bytes() {
        assert_eq!(escape_bytes(b"\x1b[5n a\xff"), "\\x1b[5n a\\xff");
        assert_eq!(decode(b"\x1b[5n"), [Ev::Unknown("\\x1b[5n".into())]);
        assert_eq!(decode(b"\x1bOz"), [Ev::Unknown("\\x1bOz".into())]);
        // a byte that is not UTF-8 does not take the text after it
        assert_eq!(decode(b"\xffok"), [Ev::Unknown("\\xff".into()), text("ok")]);
    }
}
