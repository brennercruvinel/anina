//! The tape language: one step per line, `#` comments, shell-like quoting,
//! `$VAR` / `${VAR}` / `${VAR:-default}` / `~` expansion. It reads anina's own
//! dialect and the charmbracelet/vhs one, so existing `.tape` files run as is.
//!
//! ```text
//! Set Cols 100 | Rows 30 | FPS 10 | TypingSpeed 75ms | WaitTimeout 30s | EndHold 3
//! Set Title "..." | Theme NAME | Cwd DIR | Env K=V ... | CleanEnv on | Shell "bash"
//! Exec CMD             run CMD with sh before or between steps (not recorded)
//! Spawn CMD ARGS...    start the program (no shell around it)
//! Type [@time] TEXT    type TEXT a key at a time       (vhs: Type@50ms "text")
//! Key KEY... | KEY*N   press keys: Enter Tab Up C-q ctrl+q ...  (vhs: Enter 3, Ctrl+C)
//! Wait TEXT|/re/ [T]   until it is on screen           (vhs: Wait+Screen@10s /re/)
//! Gone TEXT|/re/ [T]   until it is no longer on screen
//! Idle [T]             until the screen stops changing
//! Sleep T | Hold T     keep recording T | stretch the current frame by T
//! Caption TEXT         caption from now on ("" clears)
//! Speed X              play what follows X times faster
//! Hide | Show          cut the time in between
//! Snap [NAME]          force a frame; with NAME also save a screenshot (vhs: Screenshot x.png)
//! Poster               the still frame viewers without animation show
//! When TEXT STEP...    run STEP only when TEXT is on screen
//! Resize COLS ROWS     resize the terminal
//! Require PROGRAM      fail early when PROGRAM is not on PATH
//! Output PATH          where `anina record` writes (.svg, .cast.json, .cast)
//! Source FILE          include another tape
//! ```

use crate::screen::Pattern;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Step {
    Set(String, Vec<String>),
    Exec(String),
    Spawn(Vec<String>),
    Type { text: String, delay: Option<f64> },
    Key { keys: Vec<(String, u32)>, delay: Option<f64> },
    Wait { pat: Pattern, gone: bool, timeout: Option<f64> },
    Idle { timeout: Option<f64> },
    Sleep(f64),
    Hold(f64),
    Caption(String),
    Speed(f64),
    Hide,
    Show,
    Snap(Option<String>),
    Poster,
    When { pat: Pattern, then: Box<Step> },
    Resize(u16, u16),
    Require(String),
    Output(String),
}

#[derive(Clone, Debug)]
pub struct Line {
    pub file: PathBuf,
    pub n: usize,
    pub step: Step,
}

#[derive(Clone, Debug, Default)]
pub struct Tape {
    pub path: PathBuf,
    pub lines: Vec<Line>,
    /// Things read but not supported (vhs pixel settings, Copy/Paste...).
    pub warnings: Vec<String>,
}

impl Tape {
    pub fn load(path: &Path, vars: &HashMap<String, String>) -> Result<Tape> {
        let mut tape = Tape { path: path.to_path_buf(), ..Default::default() };
        load_into(&mut tape, path, vars, 0)?;
        Ok(tape)
    }

    pub fn parse_str(src: &str, name: &str, vars: &HashMap<String, String>) -> Result<Tape> {
        let mut tape = Tape { path: PathBuf::from(name), ..Default::default() };
        parse_lines(&mut tape, src, Path::new(name), Path::new("."), vars, 0)?;
        Ok(tape)
    }

    /// The `Output` paths the tape asks for.
    pub fn outputs(&self) -> Vec<String> {
        self.lines.iter().filter_map(|l| if let Step::Output(p) = &l.step { Some(p.clone()) } else { None }).collect()
    }
}

fn load_into(tape: &mut Tape, path: &Path, vars: &HashMap<String, String>, depth: usize) -> Result<()> {
    if depth > 8 {
        bail!("{}: Source nested too deep", path.display());
    }
    let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    parse_lines(tape, &src, path, &dir, vars, depth)
}

fn parse_lines(
    tape: &mut Tape,
    src: &str,
    file: &Path,
    dir: &Path,
    vars: &HashMap<String, String>,
    depth: usize,
) -> Result<()> {
    for (i, raw) in src.lines().enumerate() {
        let n = i + 1;
        let at = || format!("{}:{n}", file.display());
        // $VAR and ~ expand outside '...' only, as in a shell
        let words = split_with(raw, |seg| expand(seg, vars)).map_err(|e| anyhow!("{}: {e}", at()))?;
        if words.is_empty() {
            continue;
        }
        if words[0].eq_ignore_ascii_case("exec") {
            // the shell gets the line as written: its quotes are its own
            let raw = raw.trim_start();
            let cmd = raw[raw.find(char::is_whitespace).unwrap_or(raw.len())..].trim();
            tape.lines.push(Line { file: file.to_path_buf(), n, step: Step::Exec(expand(cmd, vars)) });
            continue;
        }
        if words[0].eq_ignore_ascii_case("source") {
            let p = dir.join(words.get(1).ok_or_else(|| anyhow!("{}: Source needs a file", at()))?);
            load_into(tape, &p, vars, depth + 1)?;
            continue;
        }
        match parse_step(&words, &mut tape.warnings) {
            Ok(Some(step)) => tape.lines.push(Line { file: file.to_path_buf(), n, step }),
            Ok(None) => {}
            Err(e) => bail!("{}: {e}", at()),
        }
    }
    Ok(())
}

/// Seconds from `500ms`, `2s`, `1.5`, `1m`.
pub fn duration(s: &str) -> Result<f64> {
    let s = s.trim();
    let (num, mul) = if let Some(v) = s.strip_suffix("ms") {
        (v, 0.001)
    } else if let Some(v) = s.strip_suffix('s') {
        (v, 1.0)
    } else if let Some(v) = s.strip_suffix('m') {
        (v, 60.0)
    } else {
        (s, 1.0)
    };
    let v: f64 = num.parse().map_err(|_| anyhow!("not a duration: {s:?}"))?;
    Ok(v * mul)
}

const VHS_KEYS: &[&str] = &[
    "enter",
    "tab",
    "space",
    "backspace",
    "escape",
    "esc",
    "up",
    "down",
    "left",
    "right",
    "pageup",
    "pagedown",
    "home",
    "end",
    "delete",
    "insert",
    "scrollup",
    "scrolldown",
];

fn is_key_command(cmd: &str) -> bool {
    let c = cmd.to_ascii_lowercase();
    VHS_KEYS.contains(&c.as_str())
        || c.starts_with("ctrl+")
        || c.starts_with("alt+")
        || c.starts_with("shift+")
        || (c.len() >= 2 && c.starts_with('f') && c[1..].chars().all(|d| d.is_ascii_digit()))
}

/// Parse one tokenized line. `Ok(None)` for lines that only warn.
pub fn parse_step(words: &[String], warnings: &mut Vec<String>) -> Result<Option<Step>> {
    // `Type@50ms`, `Wait+Screen@10s`, `Enter@100ms`
    let (head, at) = match words[0].split_once('@') {
        Some((h, t)) => (h.to_string(), Some(duration(t)?)),
        None => (words[0].clone(), None),
    };
    let (cmd, scope) = match head.split_once('+') {
        Some((c, s)) if c.eq_ignore_ascii_case("wait") => (c.to_ascii_lowercase(), Some(s.to_ascii_lowercase())),
        _ => (head.to_ascii_lowercase(), None),
    };
    let args = &words[1..];
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("{} needs an argument", words[0]));
    let rest = || args.join(" ");

    if is_key_command(&head) {
        let count = match args.first() {
            Some(n) => n.parse::<u32>().map_err(|_| anyhow!("{head}: count must be a number"))?,
            None => 1,
        };
        return Ok(Some(Step::Key { keys: vec![(crate::engine::normalize_key(&head), count)], delay: at }));
    }

    Ok(Some(match cmd.as_str() {
        "set" => {
            let name = arg(0)?.to_ascii_lowercase();
            let vals = args[1..].to_vec();
            match name.as_str() {
                "fontsize" | "fontfamily" | "width" | "height" | "letterspacing" | "lineheight" | "padding"
                | "margin" | "marginfill" | "windowbar" | "windowbarsize" | "borderradius" | "loopoffset"
                | "cursorblink" => {
                    warnings.push(format!(
                        "Set {} is a pixel/style setting of vhs; anina sizes by Cols/Rows and themes",
                        args[0]
                    ));
                    return Ok(None);
                }
                _ => Step::Set(name, vals),
            }
        }
        "env" => Step::Set("env".into(), vec![format!("{}={}", arg(0)?, args[1..].join(" "))]),
        "exec" => Step::Exec(rest()),
        "spawn" => {
            if args.is_empty() {
                bail!("Spawn needs a program");
            }
            Step::Spawn(args.to_vec())
        }
        "type" => {
            // ours: Type TEXT [SECS]; vhs: Type@50ms "TEXT" (several strings join)
            if args.len() == 2 && at.is_none() && duration(&args[1]).is_ok() {
                Step::Type { text: args[0].clone(), delay: Some(duration(&args[1])?) }
            } else {
                Step::Type { text: args.join(""), delay: at }
            }
        }
        "key" => {
            let mut keys = Vec::new();
            for k in args {
                let (name, n) = match k.rsplit_once('*') {
                    Some((name, n)) if !name.is_empty() => {
                        (name, n.parse::<u32>().map_err(|_| anyhow!("bad repeat in {k}"))?)
                    }
                    _ => (k.as_str(), 1),
                };
                keys.push((crate::engine::normalize_key(name), n));
            }
            if keys.is_empty() {
                bail!("Key needs at least one key");
            }
            Step::Key { keys, delay: at }
        }
        "wait" | "gone" => {
            let gone = cmd == "gone";
            // vhs reads `/a regex with spaces/` as one token
            let joined;
            let args = match args.iter().position(|a| a.len() > 1 && a.ends_with('/')) {
                Some(end) if end > 0 && args[0].starts_with('/') && !(args[0].len() > 1 && args[0].ends_with('/')) => {
                    joined = std::iter::once(args[..=end].join(" "))
                        .chain(args[end + 1..].iter().cloned())
                        .collect::<Vec<_>>();
                    &joined[..]
                }
                _ => args,
            };
            let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("{} needs an argument", words[0]));
            if args.is_empty() {
                // vhs: bare Wait waits for the prompt
                Step::Wait { pat: Pattern::parse("/[$>#%] ?$/")?, gone, timeout: at }
            } else {
                let mut pat = Pattern::parse(&arg(0)?)?;
                if scope.as_deref() == Some("line") {
                    if let Pattern::Regex(r) = &pat {
                        warnings.push(format!("Wait+Line {} matches anywhere on screen", r.as_str()));
                    }
                    pat = pat.clone();
                }
                let timeout = match args.get(1) {
                    Some(t) => Some(duration(t)?),
                    None => at,
                };
                Step::Wait { pat, gone, timeout }
            }
        }
        "idle" => Step::Idle { timeout: args.first().map(|t| duration(t)).transpose()?.or(at) },
        "sleep" => Step::Sleep(duration(&arg(0)?)?),
        "hold" => Step::Hold(duration(&arg(0)?)?),
        "caption" => Step::Caption(rest()),
        "speed" => Step::Speed(arg(0)?.parse().map_err(|_| anyhow!("Speed takes a number"))?),
        "hide" => Step::Hide,
        "show" => Step::Show,
        "snap" => Step::Snap(args.first().cloned()),
        "screenshot" => {
            let p = arg(0)?;
            let name = Path::new(&p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(p);
            Step::Snap(Some(name))
        }
        "poster" => Step::Poster,
        "when" => {
            if args.len() < 2 {
                bail!("When needs a text and a step");
            }
            let then = parse_step(&args[1..], warnings)?.ok_or_else(|| anyhow!("When: the step does nothing"))?;
            Step::When { pat: Pattern::parse(&args[0])?, then: Box::new(then) }
        }
        "resize" => Step::Resize(arg(0)?.parse()?, arg(1)?.parse()?),
        "require" => Step::Require(arg(0)?),
        "output" => Step::Output(arg(0)?),
        "copy" | "paste" | "scrollup" | "scrolldown" => {
            warnings.push(format!("{} is not supported yet; skipped", words[0]));
            return Ok(None);
        }
        other => bail!("unknown step {other:?}"),
    }))
}

/// Split a line like a shell: whitespace separates, '…' is literal, "…"
/// keeps backslashes except before " \\ $ `, `#` at a word start begins a comment.
pub fn split(line: &str) -> Result<Vec<String>> {
    split_with(line, |seg| seg.to_string())
}

/// `split`, passing every part of a word that is not in single quotes
/// through `expand` (so '$HOME' stays literal and "$HOME" does not).
pub fn split_with(line: &str, expand: impl Fn(&str) -> String) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new(); // expanded parts of the current word
    let mut cur = String::new(); // the part being read, not yet expanded
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '#' if !in_word => break,
            c if c.is_whitespace() => {
                if in_word {
                    word.push_str(&expand(&std::mem::take(&mut cur)));
                    out.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                word.push_str(&expand(&std::mem::take(&mut cur)));
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => bail!("unclosed '"),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        // POSIX: inside "..." a backslash only escapes " \\ $ `
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => cur.push(c),
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => bail!("unclosed \""),
                        },
                        Some(c) => cur.push(c),
                        None => bail!("unclosed \""),
                    }
                }
            }
            '`' => {
                // vhs allows `backtick strings`
                in_word = true;
                loop {
                    match chars.next() {
                        Some('`') => break,
                        Some(c) => cur.push(c),
                        None => bail!("unclosed `"),
                    }
                }
            }
            '\\' => {
                in_word = true;
                // a `/regex/` word is literal, as in vhs: `\$` stays an escape
                if cur.starts_with('/') {
                    cur.push('\\');
                }
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        word.push_str(&expand(&cur));
        out.push(word);
    }
    Ok(out)
}

/// `$VAR`, `${VAR}`, `${VAR:-default}` from `vars` then the process
/// environment; a leading `~` is $HOME. Unknown variables expand to "".
pub fn expand(s: &str, vars: &HashMap<String, String>) -> String {
    let lookup = |k: &str| vars.get(k).cloned().or_else(|| std::env::var(k).ok());
    let mut out = String::new();
    let s = if s == "~" || s.starts_with("~/") {
        format!("{}{}", lookup("HOME").unwrap_or_default(), &s[1..])
    } else {
        s.to_string()
    };
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '$' && i + 1 < b.len() {
            if b[i + 1] == '{' {
                if let Some(end) = b[i + 2..].iter().position(|c| *c == '}') {
                    let inner: String = b[i + 2..i + 2 + end].iter().collect();
                    let (name, def) = match inner.split_once(":-") {
                        Some((n, d)) => (n.to_string(), Some(d.to_string())),
                        None => (inner.clone(), None),
                    };
                    let v = lookup(&name).filter(|v| !v.is_empty()).or(def).unwrap_or_default();
                    out.push_str(&v);
                    i += end + 3;
                    continue;
                }
            } else if b[i + 1].is_ascii_alphabetic() || b[i + 1] == '_' {
                let mut j = i + 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == '_') {
                    j += 1;
                }
                let name: String = b[i + 1..j].iter().collect();
                out.push_str(&lookup(&name).unwrap_or_default());
                i = j;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The variables a tape can use besides the environment: ANINA_HOME (the
/// tape's directory), ANINA_OUT (the output directory).
pub fn base_vars(tape: &Path, out: &Path) -> HashMap<String, String> {
    let mut v = HashMap::new();
    let dir = tape.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    v.insert("TAPE_DIR".into(), dir.display().to_string());
    v.insert("ANINA_OUT".into(), out.display().to_string());
    v
}
