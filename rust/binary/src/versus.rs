//! `anina compare A B`: are two runs the same, timing aside? Two runs of the
//! same journey never match byte for byte: frame times, durations and
//! sampling move. What must match is what the run shows and leaves: the same
//! files, the same screens as text, the same pixels, the same reports once
//! their timings are taken out.

use anyhow::{Result, bail};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The run kept as reference (a folder anina wrote)
    pub a: PathBuf,
    /// The run to compare with it
    pub b: PathBuf,
}

/// Report fields that hold time or sampling, never compared.
const TIMING_KEYS: [&str; 5] = ["seconds", "recorded", "frames", "screens", "first_screen"];

/// `[4.7s]` in a transcript: when a step happened.
static STAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\d+(?:\.\d+)?s\]").unwrap());

#[derive(Default)]
struct Tally {
    texts: usize,
    pngs: usize,
    reports: usize,
    bytes: usize,
    timed: usize,
    diffs: Vec<String>,
}

pub fn run(a: Args) -> Result<()> {
    for d in [&a.a, &a.b] {
        if !d.is_dir() {
            bail!("{} is not a folder", d.display());
        }
    }
    let (fa, fb) = (files(&a.a), files(&a.b));
    let mut t = Tally::default();
    for f in fa.difference(&fb) {
        t.diffs.push(format!("only in {}: {f}", a.a.display()));
    }
    for f in fb.difference(&fa) {
        t.diffs.push(format!("only in {}: {f}", a.b.display()));
    }
    for f in fa.intersection(&fb) {
        compare(f, &std::fs::read(a.a.join(f))?, &std::fs::read(a.b.join(f))?, &mut t);
    }
    let both = fa.intersection(&fb).count();
    for line in sampling(&a.a, &a.b) {
        eprintln!("  {line}");
    }
    let summary = format!(
        "{both} files: {} texts, {} PNG by pixels, {} reports, {} by bytes; {} timing-bound, present on both sides",
        t.texts, t.pngs, t.reports, t.bytes, t.timed
    );
    if t.diffs.is_empty() {
        eprintln!("anina compare: same ({summary})");
        return Ok(());
    }
    for d in &t.diffs {
        println!("{d}");
    }
    let n = t.diffs.len();
    bail!("{n} difference{} ({summary})", if n == 1 { "" } else { "s" })
}

fn compare(f: &str, a: &[u8], b: &[u8], t: &mut Tally) {
    let same = if f.ends_with(".txt") {
        t.texts += 1;
        untimed(a) == untimed(b)
    } else if f.ends_with(".png") {
        t.pngs += 1;
        pixels(a) == pixels(b)
    } else if f.ends_with(".json") && !f.ends_with(".cast.json") {
        t.reports += 1;
        match (report(a), report(b)) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        }
    } else if f.ends_with(".svg") || f.ends_with(".gz") || f.ends_with(".html") || f.ends_with(".cast") {
        t.timed += 1;
        true
    } else {
        t.bytes += 1;
        a == b
    };
    if !same {
        t.diffs.push(format!("differs: {f}"));
    }
}

/// Every file under `dir`, as a path relative to it.
fn files(dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => {
                    if let Ok(rel) = p.strip_prefix(dir) {
                        out.insert(rel.display().to_string());
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn untimed(b: &[u8]) -> String {
    STAMP.replace_all(&String::from_utf8_lossy(b), "[•s]").into_owned()
}

/// A PNG without its text chunks: what it draws, not what it says.
fn pixels(png: &[u8]) -> Vec<u8> {
    let chunks = crate::checks::png_chunks(png);
    if chunks.is_empty() {
        return png.to_vec();
    }
    chunks
        .into_iter()
        .filter(|(k, _)| !matches!(*k, b"tEXt" | b"zTXt" | b"iTXt"))
        .flat_map(|(k, d)| k.iter().chain(d).copied().collect::<Vec<u8>>())
        .collect()
}

/// A JSON report with every timing field taken out.
fn report(b: &[u8]) -> Option<serde_json::Value> {
    let mut v: serde_json::Value = serde_json::from_slice(b).ok()?;
    strip(&mut v);
    Some(v)
}

fn strip(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            m.retain(|k, _| !TIMING_KEYS.contains(&k.as_str()));
            m.values_mut().for_each(strip);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
        _ => {}
    }
}

/// Frames and screens per part, side by side: they move with sampling, so
/// they are shown, not judged.
fn sampling(a: &Path, b: &Path) -> Vec<String> {
    let parts = |d: &Path| -> Vec<(String, u64, u64)> {
        let Ok(text) = std::fs::read(d.join("run.json")) else { return Vec::new() };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&text) else { return Vec::new() };
        v["parts"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .map(|p| {
                        (
                            p["name"].as_str().unwrap_or("?").to_string(),
                            p["frames"].as_u64().unwrap_or(0),
                            p["screens"].as_u64().unwrap_or(0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let pb = parts(b);
    parts(a)
        .into_iter()
        .filter_map(|(n, f, s)| {
            let (_, f2, s2) = pb.iter().find(|(m, _, _)| *m == n)?;
            Some(format!("{n}: {f} vs {f2} frames, {s} vs {s2} screens (sampling, not compared)"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_do_not_count() {
        assert_eq!(untimed(b"[4.7s]\n$ ls"), untimed(b"[4.5s]\n$ ls"));
        assert_ne!(untimed(b"[4.7s]\n$ ls"), untimed(b"[4.7s]\n$ pwd"));
        let a = br#"{"seconds":16.4,"recorded":"x","parts":[{"name":"p","frames":149,"shots":["a"]}]}"#;
        let b = br#"{"seconds":16.2,"recorded":"y","parts":[{"name":"p","frames":148,"shots":["a"]}]}"#;
        assert_eq!(report(a), report(b));
        let c = br#"{"seconds":16.2,"parts":[{"name":"p","shots":["b"]}]}"#;
        assert_ne!(report(a), report(c));
    }

    #[test]
    fn a_png_compares_by_pixels_not_text() {
        let dir = std::env::temp_dir().join(format!("anina-versus-{}", std::process::id()));
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#123"/></svg>"##;
        let (p1, p2) = (dir.join("a.png"), dir.join("b.png"));
        anina_render::pixel::svg_to_png(svg, &p1, 1.0).unwrap();
        anina_render::pixel::svg_to_png_with_text(svg, &p2, 1.0, Some("screen")).unwrap();
        let (a, b) = (std::fs::read(&p1).unwrap(), std::fs::read(&p2).unwrap());
        assert_ne!(a, b);
        assert_eq!(pixels(&a), pixels(&b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
