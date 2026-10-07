//! `anina check`: look through what anina wrote before it goes anywhere. Every
//! text file (casts inside their gzip too) is searched for what redaction
//! hides: this machine's home path, user, host and git identity, secret
//! shapes, and the `[redact]` rules of the config. PNGs are pixels and are
//! skipped; their SVG and text twins are checked instead.

use anina_runner::redact::Redactor;
use anyhow::{Result, bail};
use regex::Regex;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

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
    let red = Redactor::shared();
    let (mut files, mut skipped, mut found) = (0, 0, Vec::new());
    for p in &paths {
        if !p.exists() {
            bail!("{} does not exist", p.display());
        }
        for f in walk(p) {
            match read_text(&f)? {
                Some(text) => {
                    files += 1;
                    found.extend(scan(&f, &text, red));
                }
                None => skipped += 1,
            }
        }
    }
    for f in &found {
        println!("{}:{}: {}: {}", f.file.display(), f.line, f.label, f.context);
    }
    let checked = format!("{files} files checked, {skipped} binary skipped");
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

/// A file's text (gunzipped when it is gzip), or None for binary files.
fn read_text(p: &Path) -> Result<Option<String>> {
    let mut bytes = std::fs::read(p)?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out)?;
        bytes = out;
    }
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
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
        assert_eq!(read_text(&gz).unwrap().as_deref(), Some("{\"tape\":\"/Users/alice/t\"}"));
        let png = dir.join("a.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0").unwrap();
        assert_eq!(read_text(&png).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
