//! Errors that teach: when a wait fails, say what was on screen instead,
//! point at the closest text ("did you mean"), and leave the screen on disk.

use crate::{Pattern, Screen};
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
        msg.push_str(&format!("\n  saved: {}", self::shown(&dir.join("failed.txt"))));
    }
    anyhow::anyhow!(msg)
}

/// A path the way a status line shows it: relative to the working directory
/// when it is under it, `$TMPDIR/...` under a per-user temp folder (macOS's
/// `/var/folders/<id>/T`), `~/...` under HOME, as is otherwise. Output pasted into
/// a README or an issue then names no machine. A working directory at `/` or
/// above HOME (`/Users`) does not count: relative to it, a path would still
/// spell out the user's folder.
pub fn shown(p: &std::path::Path) -> String {
    let cwd: Vec<std::path::PathBuf> = std::env::current_dir()
        .map(|c| vec![std::fs::canonicalize(&c).unwrap_or_else(|_| c.clone()), c])
        .unwrap_or_default();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    shown_from(p, &cwd, home.as_deref(), &user_tmp())
}

/// `TMPDIR` as written and as resolved, when it is not the shared `/tmp`.
fn user_tmp() -> Vec<std::path::PathBuf> {
    let Some(t) = std::env::var_os("TMPDIR").map(std::path::PathBuf::from) else { return Vec::new() };
    if t.parent().is_none() || t == std::path::Path::new("/tmp") || t == std::path::Path::new("/var/tmp") {
        return Vec::new();
    }
    let real = std::fs::canonicalize(&t).ok().filter(|r| *r != t);
    [t].into_iter().chain(real).collect()
}

fn shown_from(
    p: &std::path::Path,
    cwd: &[std::path::PathBuf],
    home: Option<&std::path::Path>,
    tmp: &[std::path::PathBuf],
) -> String {
    if !p.is_absolute() {
        return p.display().to_string();
    }
    let rel = |base: &std::path::Path| p.strip_prefix(base).ok().map(|r| r.display().to_string());
    let above_home = |d: &std::path::Path| d.parent().is_none() || home.is_some_and(|h| h != d && h.starts_with(d));
    if let Some(r) = cwd.iter().filter(|d| !above_home(d)).find_map(|d| rel(d)) {
        return if r.is_empty() { ".".into() } else { r };
    }
    if let Some(r) = tmp.iter().find_map(|d| rel(d)) {
        return if r.is_empty() { "$TMPDIR".into() } else { format!("$TMPDIR/{r}") };
    }
    if let Some(r) = home.filter(|h| h.parent().is_some()).and_then(rel) {
        return if r.is_empty() { "~".into() } else { format!("~/{r}") };
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_path_names_no_machine_from_any_working_directory() {
        let home = Some(Path::new("/home/runner"));
        let out = Path::new("/home/runner/anina-out/shell/index.html");
        let at = |d: &str| shown_from(out, &[PathBuf::from(d)], home, &[]);
        assert_eq!(at("/home/runner/anina-out"), "shell/index.html");
        assert_eq!(at("/home/runner"), "anina-out/shell/index.html");
        assert_eq!(at("/"), "~/anina-out/shell/index.html");
        assert_eq!(at("/home"), "~/anina-out/shell/index.html");
        assert_eq!(at("/srv"), "~/anina-out/shell/index.html");
        assert_eq!(shown_from(Path::new("/tmp/x.svg"), &[PathBuf::from("/")], home, &[]), "/tmp/x.svg");
        assert_eq!(shown_from(out, &[], Some(Path::new("/")), &[]), out.display().to_string());
    }

    #[test]
    fn a_per_user_temp_folder_shows_as_tmpdir() {
        let home = Some(Path::new("/Users/alice"));
        let tmp = [PathBuf::from("/var/folders/mj/abc123/T"), PathBuf::from("/private/var/folders/mj/abc123/T")];
        let at = |p: &str| shown_from(Path::new(p), &[PathBuf::from("/Users/alice/proj")], home, &tmp);
        assert_eq!(at("/var/folders/mj/abc123/T/tmp.X/eof/eof.svg"), "$TMPDIR/tmp.X/eof/eof.svg");
        assert_eq!(at("/private/var/folders/mj/abc123/T/x.svg"), "$TMPDIR/x.svg");
        assert_eq!(at("/Users/alice/proj/out/x.svg"), "out/x.svg");
        assert_eq!(at("/tmp/x.svg"), "/tmp/x.svg");
    }
}
