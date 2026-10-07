//! What a run changed on disk: snapshot a tree before and after, compare.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    kind: char, // f d l
    size: u64,
    mtime: u128,
    /// Status-change time: moves on every write, chmod or rename, and no
    /// program can set it back (an edit that keeps size and mtime shows here).
    ctime: i128,
    mode: u32,
}

impl Entry {
    /// What `diff` reports: the content, not the metadata around it.
    fn same_content(&self, o: &Entry) -> bool {
        self.kind == o.kind && self.size == o.size && self.mtime == o.mtime
    }
}

/// Paths relative to the root. Kept as paths, not text: a name that is not
/// UTF-8 must lead back to its file.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    entries: BTreeMap<PathBuf, Entry>,
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

/// A path that differs between two snapshots, and what putting it back takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Differ {
    /// Created, deleted or changed: put back whole (a folder with all it holds).
    Path(PathBuf),
    /// A folder whose permissions alone changed: its content is compared
    /// entry by entry.
    Mode(PathBuf),
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
            let shown = p.to_string_lossy().into_owned();
            match self.entries.get(p) {
                None => c.created.push(shown.clone()),
                Some(b) if !b.same_content(e) && e.kind != 'd' => c.modified.push(shown.clone()),
                _ => continue,
            }
            if e.kind == 'f' {
                *c.bytes_by_dir.entry(top(&shown)).or_default() += e.size;
                c.bytes += e.size;
            }
        }
        c.deleted = self
            .entries
            .keys()
            .filter(|p| !after.entries.contains_key(*p))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        c
    }

    /// Everything that differs from `now`: a file or link that changed in any
    /// way (content, permissions, an edit that kept size and mtime), a path
    /// created or deleted, a folder become a file or the reverse, the topmost
    /// only (below it, everything is taken as it stands); and folders whose
    /// permissions changed.
    pub fn differs(&self, now: &Snapshot) -> Vec<Differ> {
        let mut out = Vec::new();
        let mut whole: Option<&PathBuf> = None;
        let paths: BTreeSet<&PathBuf> = self.entries.keys().chain(now.entries.keys()).collect();
        // component order: a folder's entries follow it, before any sibling
        for p in paths {
            if whole.is_some_and(|w| p.starts_with(w)) {
                continue;
            }
            match (self.entries.get(p), now.entries.get(p)) {
                (Some(a), Some(b)) if a.kind == 'd' && b.kind == 'd' => {
                    if a.mode != b.mode {
                        out.push(Differ::Mode(p.clone()));
                    }
                }
                (Some(a), Some(b)) if a == b => {}
                _ => {
                    whole = Some(p);
                    out.push(Differ::Path(p.clone()));
                }
            }
        }
        out
    }

    /// Read again what `put_back` touched: each path it replaced with all
    /// it holds, each folder whose permissions it set as the folder alone.
    /// The rest of the snapshot still holds, so the sandbox is not walked
    /// a second time.
    pub fn refresh(&mut self, root: &Path, changed: &[Differ]) {
        for d in changed {
            let (p, whole) = match d {
                Differ::Path(p) => (p, true),
                Differ::Mode(p) => (p, false),
            };
            if whole {
                let below: Vec<PathBuf> =
                    self.entries.range(p.clone()..).map(|(k, _)| k).take_while(|k| k.starts_with(p)).cloned().collect();
                for k in below {
                    self.entries.remove(&k);
                }
            }
            let full = root.join(p);
            match entry(&full) {
                Some(e) => {
                    let dir = e.kind == 'd';
                    self.entries.insert(p.clone(), e);
                    if whole && dir {
                        walk(root, &full, &mut self.entries);
                    }
                }
                None => {
                    self.entries.remove(p);
                }
            }
        }
    }
}

/// `home/.npm/_cacache/x` -> `home/.npm`; `work/app/x` -> `work/app`.
fn top(p: &str) -> String {
    p.splitn(3, '/').take(2).collect::<Vec<_>>().join("/")
}

fn entry(path: &Path) -> Option<Entry> {
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::symlink_metadata(path).ok()?;
    let kind = if md.file_type().is_symlink() {
        'l'
    } else if md.is_dir() {
        'd'
    } else {
        'f'
    };
    let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    Some(Entry {
        kind,
        size: if kind == 'f' { md.len() } else { 0 },
        mtime,
        ctime: md.ctime() as i128 * 1_000_000_000 + md.ctime_nsec() as i128,
        mode: md.mode(),
    })
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Entry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let Some(en) = entry(&path) else { continue };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let dir = en.kind == 'd';
        out.insert(rel, en);
        if dir {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("anina-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn paths(d: &[Differ]) -> Vec<String> {
        d.iter()
            .map(|x| match x {
                Differ::Path(p) => p.display().to_string(),
                Differ::Mode(p) => format!("mode {}", p.display()),
            })
            .collect()
    }

    #[test]
    fn differs_names_the_topmost_paths_only() {
        let dir = tmp("differs");
        for d in ["a/b", "a-c", "keep/deep"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("keep/deep/f"), "same").unwrap();
        std::fs::write(dir.join("a/b/f"), "x").unwrap();
        let before = Snapshot::take(&dir);
        std::fs::remove_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a"), "now a file").unwrap();
        std::fs::write(dir.join("a-c/new"), "y").unwrap();
        std::fs::create_dir_all(dir.join("fresh/inner")).unwrap();
        assert_eq!(paths(&before.differs(&Snapshot::take(&dir))), ["a", "a-c/new", "fresh"]);
        assert!(before.differs(&before).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn differs_sees_chmod_and_an_edit_that_kept_size_and_mtime() {
        // review of #58, 3
        let dir = tmp("differs-meta");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        for f in ["bin/run", "conf", "untouched"] {
            std::fs::write(dir.join(f), "aaaa").unwrap();
        }
        let before = Snapshot::take(&dir);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::set_permissions(dir.join("bin/run"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(dir.join("bin"), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mtime = std::fs::metadata(dir.join("conf")).unwrap().modified().unwrap();
        std::fs::write(dir.join("conf"), "bbbb").unwrap();
        std::fs::File::options().write(true).open(dir.join("conf")).unwrap().set_modified(mtime).unwrap();
        let now = Snapshot::take(&dir);
        assert_eq!(paths(&before.differs(&now)), ["mode bin", "bin/run", "conf"]);
        // what a run reports stays the content: same size, same mtime
        assert!(before.diff(&now).modified.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_is_not_utf8_leads_back_to_its_file() {
        // review of #58, 4
        use std::os::unix::ffi::OsStrExt;
        let dir = tmp("differs-bytes");
        let before = Snapshot::take(&dir);
        let name = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
        if std::fs::write(dir.join(name), "x").is_err() {
            return; // a file system that refuses such names (APFS normalizes, some refuse)
        }
        let d = before.differs(&Snapshot::take(&dir));
        let [Differ::Path(p)] = d.as_slice() else { panic!("{d:?}") };
        assert!(dir.join(p).exists(), "{p:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_reads_again_only_what_it_is_given() {
        let dir = tmp("refresh");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/x"), "1").unwrap();
        std::fs::write(dir.join("b"), "1").unwrap();
        let mut snap = Snapshot::take(&dir);
        std::fs::remove_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("a/y")).unwrap();
        std::fs::write(dir.join("c"), "new").unwrap();
        snap.refresh(&dir, &[Differ::Path("a".into()), Differ::Path("c".into())]);
        assert!(snap.differs(&Snapshot::take(&dir)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
