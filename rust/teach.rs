//! Errors that teach: when a wait fails, say what was on screen instead,
//! point at the closest text ("did you mean"), and leave the screen on disk.

use crate::screen::{Pattern, Screen};
use std::path::Path;

/// The closest stretch of on-screen text to `want`, by edit distance over
/// windows of the same length, with its row. `None` when nothing is close.
pub fn nearest(screen: &Screen, want: &str) -> Option<(String, u16, usize)> {
    let want: Vec<char> = want.chars().collect();
    if want.is_empty() {
        return None;
    }
    let mut best: Option<(String, u16, usize)> = None;
    for y in 0..screen.rows {
        let row: Vec<char> = screen.row_text(y).chars().collect();
        if row.is_empty() {
            continue;
        }
        let w = want.len().min(row.len());
        for start in 0..=row.len() - w {
            for len in [w.saturating_sub(2).max(1), w, (w + 2).min(row.len() - start)] {
                if start + len > row.len() || len == 0 {
                    continue;
                }
                let cand: Vec<char> = row[start..start + len].to_vec();
                let d = levenshtein(&want, &cand);
                if best.as_ref().is_none_or(|b| d < b.2) {
                    best = Some((cand.iter().collect::<String>().trim().to_string(), y, d));
                }
            }
        }
    }
    // close enough to be a typo or a reworded label
    best.filter(|b| b.2 <= (want.len() / 3).max(2) && !b.0.is_empty())
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(!ca.eq_ignore_ascii_case(cb));
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The error for a wait that timed out, with the screen it saw. When `dump`
/// is a directory, the screen is saved there as `failed.txt` / `failed.ansi`.
pub fn wait_failed(screen: &Screen, pat: &Pattern, gone: bool, secs: f64, dump: Option<&Path>) -> anyhow::Error {
    let want = pat.as_str();
    let mut msg = if gone {
        format!("{want:?} was still on screen after {secs}s")
    } else {
        format!("{want:?} did not appear on screen in {secs}s")
    };
    if !gone
        && let Pattern::Text(t) = pat
        && let Some((near, row, _)) = nearest(screen, t)
    {
        msg.push_str(&format!("\n  did you mean {near:?}? (row {})", row + 1));
    }
    let text = screen.text();
    let shown: Vec<String> = text.lines().map(|l| format!("  │ {l}")).collect();
    msg.push_str(&format!("\n  the screen was ({}x{}):\n{}", screen.cols, screen.rows, shown.join("\n")));
    if let Some(dir) = dump
        && std::fs::create_dir_all(dir).is_ok()
        && std::fs::write(dir.join("failed.txt"), &text).is_ok()
        && std::fs::write(dir.join("failed.ansi"), screen.to_ansi()).is_ok()
    {
        msg.push_str(&format!("\n  saved: {}", dir.join("failed.txt").display()));
    }
    anyhow::anyhow!(msg)
}
