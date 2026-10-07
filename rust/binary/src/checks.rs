//! `anina check`: look through what anina wrote before it goes anywhere. Every
//! text file (casts inside their gzip too) is searched for what redaction
//! hides: this machine's home path, user, host and git identity, secret
//! shapes, and the `[redact]` rules of the config. A PNG is read through the
//! text chunks anina writes into it (the screen it shows); a PNG without
//! text is only pixels, and the summary counts it apart.

use anina_runner::redact::Redactor;
use anyhow::{Result, bail};
use regex::Regex;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, SystemTime};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Files or directories to check (default: anina-out)
    pub paths: Vec<PathBuf>,
}

/// Embedded fonts and images: base64 that a short user name can turn up in
/// by chance.
static DATA_URI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"data:[a-z]+/[a-z0-9.+-]+;base64,[A-Za-z0-9+/=]+").unwrap());

pub struct Finding {
    pub file: PathBuf,
    pub line: usize,
    pub label: &'static str,
    pub context: String,
}

pub fn run(a: Args) -> Result<()> {
    let paths = if a.paths.is_empty() { vec![PathBuf::from("anina-out")] } else { a.paths };
    check(&paths)
}

/// When the command started: `verify` checks what was written since.
static STARTED: OnceLock<SystemTime> = OnceLock::new();

/// Mark the start of the command, before it writes anything.
pub fn mark_start() {
    let _ = STARTED.set(SystemTime::now());
}

/// Check what a command just wrote, before it reports success: the files
/// under `out` written since `mark_start`, not a note the user keeps there
/// or what an earlier run left (`anina check` reads those).
pub fn verify(out: &Path) -> Result<()> {
    // a little before the start: some file systems keep whole seconds
    let since = STARTED.get().map(|t| *t - Duration::from_secs(2));
    check_since(&[out.to_path_buf()], since)
}

fn check(paths: &[PathBuf]) -> Result<()> {
    check_since(paths, None)
}

fn written_since(f: &Path, since: Option<SystemTime>) -> bool {
    since.is_none_or(|t| std::fs::metadata(f).and_then(|m| m.modified()).is_ok_and(|m| m >= t))
}

fn check_since(paths: &[PathBuf], since: Option<SystemTime>) -> Result<()> {
    let red = Redactor::shared();
    let (mut files, mut pngs, mut blind, mut skipped, mut found) = (0, 0, 0, 0, Vec::new());
    for p in paths {
        if !p.exists() {
            bail!("{} does not exist", p.display());
        }
        for f in walk(p).into_iter().filter(|f| written_since(f, since)) {
            match read(&f)? {
                Content::Text(text) => {
                    files += 1;
                    found.extend(scan(&f, &text, red));
                }
                Content::Png(text) => {
                    pngs += 1;
                    found.extend(scan(&f, &text, red));
                }
                Content::Pixels => blind += 1,
                Content::Binary => skipped += 1,
            }
        }
    }
    for f in &found {
        println!("{}:{}: {}: {}", f.file.display(), f.line, f.label, f.context);
    }
    let mut checked = format!("{files} files checked");
    if pngs > 0 {
        checked.push_str(&format!(", {pngs} PNG read through their screen text"));
    }
    if blind > 0 {
        checked.push_str(&format!(", {blind} PNG with no text, not checked"));
    }
    if skipped > 0 {
        checked.push_str(&format!(", {skipped} binary skipped"));
    }
    if found.is_empty() {
        eprintln!("anina check: nothing to hide ({checked})");
        return Ok(());
    }
    let nfiles = {
        let mut v: Vec<&PathBuf> = found.iter().map(|f| &f.file).collect();
        v.dedup();
        v.len()
    };
    let n = found.len();
    let s = |k: usize| if k == 1 { "" } else { "s" };
    bail!("{n} thing{} to hide in {nfiles} file{} ({checked})", s(n), s(nfiles))
}

/// What `text` shows that redaction would hide, a finding per match.
pub fn scan(file: &Path, text: &str, red: &Redactor) -> Vec<Finding> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = DATA_URI.replace_all(line, "data:…");
        for (label, r) in red.leaks(&line) {
            out.push(Finding { file: file.to_path_buf(), line: n + 1, label, context: around(&line, r) });
        }
    }
    out
}

/// Some 30 characters on each side of the match, on one line.
fn around(line: &str, r: std::ops::Range<usize>) -> String {
    let before: String = line[..r.start].chars().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect();
    let after: String = line[r.end..].chars().take(30).collect();
    format!("{before}[{}]{after}", &line[r]).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every file under `p`, sorted, `p` itself when it is one.
fn walk(p: &Path) -> Vec<PathBuf> {
    if p.is_file() {
        return vec![p.to_path_buf()];
    }
    let mut out = Vec::new();
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t) if t.is_file() => out.push(path),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// What a file holds, as far as `check` can read it.
#[derive(Debug, PartialEq)]
enum Content {
    /// Text, gunzipped when it was gzip.
    Text(String),
    /// A PNG's text chunks, one per line.
    Png(String),
    /// A PNG with no text chunk.
    Pixels,
    Binary,
}

fn read(p: &Path) -> Result<Content> {
    let mut bytes = std::fs::read(p)?;
    if bytes.starts_with(PNG_SIGNATURE) {
        let text = png_text(&bytes);
        return Ok(if text.is_empty() { Content::Pixels } else { Content::Png(text) });
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out)?;
        bytes = out;
    }
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Ok(Content::Binary);
    }
    Ok(String::from_utf8(bytes).map_or(Content::Binary, Content::Text))
}

pub const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A PNG's chunks as (kind, data), in order, up to IEND or the first one
/// that runs past the end. Empty when `png` is not a PNG.
pub fn png_chunks(png: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    if !png.starts_with(PNG_SIGNATURE) {
        return out;
    }
    let mut at = PNG_SIGNATURE.len();
    while at + 12 <= png.len() {
        let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
        let kind = &png[at + 4..at + 8];
        let Some(data) = png.get(at + 8..at + 8 + len) else { break };
        out.push((kind, data));
        if kind == b"IEND" {
            break;
        }
        at += 12 + len;
    }
    out
}

/// The text of every tEXt, zTXt and iTXt chunk of a PNG, joined by lines.
/// A chunk that does not parse is left out; the rest still counts.
fn png_text(png: &[u8]) -> String {
    let text: Vec<String> = png_chunks(png)
        .into_iter()
        .filter_map(|(kind, data)| match kind {
            b"tEXt" => data.splitn(2, |b| *b == 0).nth(1).map(latin1),
            b"zTXt" => {
                data.splitn(2, |b| *b == 0).nth(1).and_then(|r| r.get(1..)).and_then(inflate).map(|b| latin1(&b))
            }
            b"iTXt" => itxt(data),
            _ => None,
        })
        .collect();
    text.join("\n")
}

/// keyword NUL flag method language NUL translated NUL text
fn itxt(data: &[u8]) -> Option<String> {
    let mut parts = data.splitn(2, |b| *b == 0);
    parts.next()?;
    let rest = parts.next()?;
    let (flag, rest) = (*rest.first()?, rest.get(2..)?);
    let mut fields = rest.splitn(3, |b| *b == 0);
    let (_lang, _translated, text) = (fields.next()?, fields.next()?, fields.next()?);
    let text = if flag == 1 { inflate(text)? } else { text.to_vec() };
    String::from_utf8(text).ok()
}

fn inflate(z: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(z).read_to_end(&mut out).ok()?;
    Some(out)
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Redactor {
        let mut r = Redactor::default();
        r.add("home path", "/Users/alice", Some("~")).unwrap();
        r.add("user name", r"\balice\b", Some("user")).unwrap();
        r
    }

    #[test]
    fn verify_reads_what_was_written_since_the_start_only() {
        let dir = std::env::temp_dir().join(format!("anina-since-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (old, new) = (dir.join("note.md"), dir.join("run.json"));
        std::fs::write(&old, "/Users/alice").unwrap();
        std::fs::write(&new, "{}").unwrap();
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(hour_ago).unwrap();
        let since = Some(SystemTime::now() - Duration::from_secs(60));
        assert!(!written_since(&old, since) && written_since(&new, since));
        assert!(written_since(&old, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_what_redaction_hides_with_the_line() {
        let f = scan(Path::new("a.txt"), "fine\nsee /Users/alice/x\n", &red());
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].line, f[0].label), (2, "home path"));
        assert!(f[0].context.contains("[/Users/alice]"), "{}", f[0].context);
    }

    #[test]
    fn embedded_fonts_are_not_read() {
        let svg = r#"<style>@font-face{src:url(data:font/ttf;base64,AAAA+alice/BBBB)}</style>"#;
        assert!(scan(Path::new("a.svg"), svg, &red()).is_empty());
    }

    #[test]
    fn gzip_is_read_and_binaries_are_skipped() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("anina-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gz = dir.join("a.cast.json.gz");
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(b"{\"tape\":\"/Users/alice/t\"}").unwrap();
        std::fs::write(&gz, e.finish().unwrap()).unwrap();
        assert_eq!(read(&gz).unwrap(), Content::Text("{\"tape\":\"/Users/alice/t\"}".into()));
        let png = dir.join("a.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0").unwrap();
        assert_eq!(read(&png).unwrap(), Content::Pixels);
        let bin = dir.join("a.bin");
        std::fs::write(&bin, b"\0\x01\x02").unwrap();
        assert_eq!(read(&bin).unwrap(), Content::Binary);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_png_is_read_through_its_screen_text() {
        let dir = std::env::temp_dir().join(format!("anina-check-png-{}", std::process::id()));
        let png = dir.join("s.png");
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#123"/></svg>"##;
        anina_render::pixel::svg_to_png_with_text(svg, &png, 1.0, Some("shell\n$ ls /Users/alice")).unwrap();
        let Content::Png(text) = read(&png).unwrap() else { panic!("not read as a png with text") };
        let f = scan(&png, &text, &red());
        assert_eq!(f.len(), 1, "{text}");
        assert_eq!(f[0].label, "home path");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
