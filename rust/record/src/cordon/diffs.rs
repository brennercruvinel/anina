//! What a run changed on disk: snapshot a tree before and after, compare.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    kind: char, // f d l
    size: u64,
    mtime: u128,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    entries: BTreeMap<String, Entry>,
}

/// Paths relative to the snapshot root, and what they add up to.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Changes {
    pub created: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    /// Bytes created or modified, by top-level dir (`home/.npm`, `work`).
    pub bytes_by_dir: BTreeMap<String, u64>,
    pub bytes: u64,
}

impl Snapshot {
    /// Walk `root` (symlinks are recorded, not followed).
    pub fn take(root: &Path) -> Snapshot {
        let mut s = Snapshot::default();
        walk(root, root, &mut s.entries);
        s
    }

    pub fn diff(&self, after: &Snapshot) -> Changes {
        let mut c = Changes::default();
        for (p, e) in &after.entries {
            match self.entries.get(p) {
                None => c.created.push(p.clone()),
                Some(b) if b != e && e.kind != 'd' => c.modified.push(p.clone()),
                _ => continue,
            }
            if e.kind == 'f' {
                *c.bytes_by_dir.entry(top(p)).or_default() += e.size;
                c.bytes += e.size;
            }
        }
        c.deleted = self.entries.keys().filter(|p| !after.entries.contains_key(*p)).cloned().collect();
        c
    }
}

/// `home/.npm/_cacache/x` -> `home/.npm`; `work/app/x` -> `work/app`.
fn top(p: &str) -> String {
    p.splitn(3, '/').take(2).collect::<Vec<_>>().join("/")
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Entry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
        let kind = if md.file_type().is_symlink() {
            'l'
        } else if md.is_dir() {
            'd'
        } else {
            'f'
        };
        let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        out.insert(rel, Entry { kind, size: if kind == 'f' { md.len() } else { 0 }, mtime });
        if kind == 'd' {
            walk(root, &path, out);
        }
    }
}

impl Changes {
    /// One line per fact, biggest dirs first.
    pub fn summary(&self) -> String {
        let files = |v: &Vec<String>| v.len();
        let mut s = format!(
            "{} created, {} modified, {} deleted, {} written",
            files(&self.created),
            files(&self.modified),
            files(&self.deleted),
            human(self.bytes)
        );
        let mut dirs: Vec<(&String, &u64)> = self.bytes_by_dir.iter().collect();
        dirs.sort_by(|a, b| b.1.cmp(a.1));
        for (d, b) in dirs.iter().take(8) {
            s.push_str(&format!("\n    {:>9}  {d}", human(**b)));
        }
        s
    }
}

pub fn human(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", units[i]) }
}
