//! Folders copied, emptied and made writable: what a sandbox needs done to
//! a tree whatever a program left in it (sockets, read-only folders, links).

use super::*;

/// Copy a directory tree: symlinks as symlinks, folders with their
/// permissions. Sockets, FIFOs and devices are left out: they belong to the
/// process that made them (an agent, a language server), not to the state.
pub(super) fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        copy_entry(&e.path(), &to.join(e.file_name()), e.file_type()?)?;
    }
    // after the entries: writing them moved the folder's mtime
    let md = std::fs::metadata(from)?;
    keep_times(&md, to);
    std::fs::set_permissions(to, md.permissions())
}

/// Give `to` the times in `md`. Build tools tell what to redo by mtime
/// (cargo, make, tsc): a file put back with a new one is built again in
/// every terminal. `fs::copy` keeps a file's times only where it clones
/// (APFS), and never a folder's. Best effort: a file that can't be opened
/// for reading keeps its new time.
pub(super) fn keep_times(md: &std::fs::Metadata, to: &Path) {
    let (Ok(m), Ok(a)) = (md.modified(), md.accessed()) else { return };
    if let Ok(f) = std::fs::File::open(to) {
        let _ = f.set_times(std::fs::FileTimes::new().set_modified(m).set_accessed(a));
    }
}

pub(super) fn copy_entry(src: &Path, dst: &Path, t: std::fs::FileType) -> std::io::Result<()> {
    use std::os::unix::fs::FileTypeExt;
    if t.is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(src)?, dst)
    } else if t.is_dir() {
        copy_tree(src, dst)
    } else if t.is_socket() || t.is_fifo() || t.is_block_device() || t.is_char_device() {
        Ok(())
    } else {
        std::fs::copy(src, dst)?;
        keep_times(&std::fs::metadata(src)?, dst);
        Ok(())
    }
}

/// Remove a tree, read-only folders included (Go's module cache is one):
/// a folder without write permission cannot be emptied, so it gets it back.
pub(super) fn remove_tree(p: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(p) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            writable(p);
            std::fs::remove_dir_all(p)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// Give every folder under `dir` (itself included) back its owner's write
/// and search bits.
pub(super) fn writable(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(md) = std::fs::symlink_metadata(dir) else { return };
    if !md.is_dir() {
        return;
    }
    let mut perm = md.permissions();
    perm.set_mode(perm.mode() | 0o700);
    let _ = std::fs::set_permissions(dir, perm);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        writable(&e.path());
    }
}
