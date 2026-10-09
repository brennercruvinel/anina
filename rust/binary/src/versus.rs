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

/// Report fields that hold time or sampling, never compared: the run's
/// clock (`recorded`, `generated`), its durations, its frame counts and the
/// latencies measured from them (`latency_ms` in an exploration's graph).
const TIMING_KEYS: [&str; 10] = [
    "seconds",
    "recorded",
    "generated",
    "frames",
    "screens",
    "first_screen",
    "first_screen_s",
    "duration_s",
    "latency",
    "latency_ms",
];

/// Report fields measured over the screens the run happened to sample: a
/// frame caught mid-redraw adds colored cells, moves a contrast count or
/// picks another "fullest" screen, and how long a screen was watched decides
/// which cells count as volatile. Shown side by side when they differ, not
/// judged.
const SAMPLED_KEYS: [&str; 8] = [
    "colored_cells",
    "truecolor_cells",
    "beyond16_cells",
    "visible_cells",
    "used_rows",
    "used_cols",
    "contrast",
    "volatile_cells",
];

/// Audit scores computed from those timings or samples: their number and
/// explanation move with the clock, so only their category and rule are
/// compared.
const TIMED_SCORES: [&str; 2] = ["responsiveness", "readability"];

/// Audit scores whose verdict holds but whose explanation quotes sampled
/// counts ("3 of 1775 visible cells colored"): the explanation is not compared.
const QUOTING_SCORES: [&str; 1] = ["compatibility"];

/// The screenshot an audit picks per terminal, the fullest screen it
/// sampled: `terminals/<name>/screen.png`.
fn sampled_png(f: &str) -> bool {
    f.ends_with("/screen.png") && (f.starts_with("terminals/") || f.contains("/terminals/"))
}

/// `[4.7s]` in a transcript: when a step happened.
static STAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\d+(?:\.\d+)?s\]").unwrap());

#[derive(Default)]
struct Tally {
    texts: usize,
    pngs: usize,
    reports: usize,
    bytes: usize,
    timed: usize,
    sampled: usize,
    diffs: Vec<String>,
    /// Sampled measures that differ, shown and not judged.
    notes: Vec<String>,
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
    for line in sampling(&a.a, &a.b).iter().chain(&t.notes) {
        eprintln!("  {line}");
    }
    let summary = format!(
        "{both} files: {} texts, {} PNG by pixels, {} reports, {} by bytes; {} timing-bound and {} sampled, present on both sides",
        t.texts, t.pngs, t.reports, t.bytes, t.timed, t.sampled
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
    } else if sampled_png(f) {
        t.sampled += 1;
        true
    } else if f.ends_with(".png") {
        t.pngs += 1;
        pixels(a) == pixels(b)
    } else if f.ends_with(".json") && !f.ends_with(".cast.json") {
        t.reports += 1;
        if let (Ok(x), Ok(y)) = (serde_json::from_slice(a), serde_json::from_slice(b)) {
            sampled_notes(f, "", &x, &y, &mut t.notes);
        }
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
            m.retain(|k, _| !TIMING_KEYS.contains(&k.as_str()) && !SAMPLED_KEYS.contains(&k.as_str()));
            match m.get("category").and_then(|c| c.as_str()) {
                Some(c) if TIMED_SCORES.contains(&c) => m.retain(|k, _| k == "category" || k == "rule"),
                Some(c) if QUOTING_SCORES.contains(&c) => {
                    m.remove("explain");
                }
                _ => {}
            }
            m.values_mut().for_each(strip);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
        _ => {}
    }
}

/// A line per sampled measure that differs between `a` and `b` (same
/// shape), named by where it sits: `report.json 60x20-truecolor
/// colored_cells: 1325 vs 1323`.
fn sampled_notes(file: &str, at: &str, a: &serde_json::Value, b: &serde_json::Value, out: &mut Vec<String>) {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            // an audit's terminal names itself
            let name = x.get("variant").and_then(|v| v.get("name")).and_then(Value::as_str);
            let here = name.map_or(at.to_string(), |n| format!("{at} {n}"));
            for (k, va) in x {
                let Some(vb) = y.get(k) else { continue };
                if SAMPLED_KEYS.contains(&k.as_str()) {
                    if va != vb {
                        let show = |v: &Value| if v.is_object() { "…".to_string() } else { v.to_string() };
                        let vs = if va.is_object() {
                            "differs".to_string()
                        } else {
                            format!("{} vs {}", show(va), show(vb))
                        };
                        out.push(format!("{file}{here} {k}: {vs} (sampled, not compared)"));
                    }
                } else {
                    sampled_notes(file, &here, va, vb, out);
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => x.iter().zip(y).for_each(|(va, vb)| sampled_notes(file, at, va, vb, out)),
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
    fn two_audits_of_the_same_run_match() {
        let a = br#"{"generated":"2026-10-07T20:00:00Z","scores":[{"category":"responsiveness","score":91,"rule":"r","explain":["first screen after 180 ms"]},{"category":"stability","score":100,"rule":"s","explain":["ran"]}],"variants":[{"first_screen_s":0.18,"duration_s":4.1,"latency":{"p50_ms":20.0},"exit_code":0}]}"#;
        let b = br#"{"generated":"2026-10-07T20:01:00Z","scores":[{"category":"responsiveness","score":88,"rule":"r","explain":["first screen after 240 ms"]},{"category":"stability","score":100,"rule":"s","explain":["ran"]}],"variants":[{"first_screen_s":0.24,"duration_s":4.3,"latency":{"p50_ms":31.0},"exit_code":0}]}"#;
        assert_eq!(report(a), report(b));
        let c = br#"{"generated":"x","scores":[{"category":"stability","score":50,"rule":"s","explain":["broke"]}],"variants":[]}"#;
        assert_ne!(report(a), report(c));
    }

    #[test]
    fn sampled_measures_are_shown_not_judged() {
        // review of #70, 3 and 4
        let a = br#"{"scores":[{"category":"readability","score":93,"rule":"r","explain":["145 of 1778"]},{"category":"compatibility","score":100,"rule":"c","explain":["3 of 2050 colored"]}],"variants":[{"variant":{"name":"120x40-16"},"status":"ok","colored_cells":1325,"visible_cells":2050,"contrast":{"text_cells":1778}}]}"#;
        let b = br#"{"scores":[{"category":"readability","score":94,"rule":"r","explain":["139 of 1775"]},{"category":"compatibility","score":100,"rule":"c","explain":["3 of 2047 colored"]}],"variants":[{"variant":{"name":"120x40-16"},"status":"ok","colored_cells":1323,"visible_cells":2047,"contrast":{"text_cells":1775}}]}"#;
        assert_eq!(report(a), report(b));
        let mut notes = Vec::new();
        let (x, y) = (serde_json::from_slice(a).unwrap(), serde_json::from_slice(b).unwrap());
        sampled_notes("report.json", "", &x, &y, &mut notes);
        assert!(
            notes.contains(&"report.json 120x40-16 colored_cells: 1325 vs 1323 (sampled, not compared)".to_string()),
            "{notes:?}"
        );
        // a verdict that changes still counts
        let c = String::from_utf8_lossy(b).replace(r#""score":100"#, r#""score":75"#);
        assert_ne!(report(a), report(c.as_bytes()));
        // two explorations: latency and volatile cells move, the graph does not
        let g1 = br#"{"states":[{"id":0,"title":"Main","volatile_cells":12}],"edges":[{"from":0,"to":1,"key":"j","latency_ms":34}]}"#;
        let g2 = br#"{"states":[{"id":0,"title":"Main","volatile_cells":9}],"edges":[{"from":0,"to":1,"key":"j","latency_ms":51}]}"#;
        assert_eq!(report(g1), report(g2));
        assert!(sampled_png("terminals/60x20-truecolor/screen.png") && !sampled_png("shell/01-start.png"));
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
