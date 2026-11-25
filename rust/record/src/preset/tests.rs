use super::*;
use std::os::unix::fs::PermissionsExt;

/// Every entry under `root`: path, mode and mtime (links aside), a file's bytes.
fn listing(root: &Path) -> Vec<(PathBuf, u32, Option<std::time::SystemTime>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let (p, md) = (e.path(), std::fs::symlink_metadata(e.path()).unwrap());
            let bytes = if md.is_file() { std::fs::read(&p).unwrap() } else { Vec::new() };
            let mtime = (!md.is_symlink()).then(|| md.modified().unwrap());
            out.push((p.strip_prefix(root).unwrap().to_path_buf(), md.permissions().mode(), mtime, bytes));
            if md.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_sandbox_has_a_saved_copy_of_its_own() {
    // review of #70, 8
    let sb = Path::new("/cache/anina/sandbox");
    let (a, b) = (pristine_of(&sb.join("audit-tool.a")), pristine_of(&sb.join("audit-tool.b")));
    assert_ne!(a, b);
    assert_eq!(a, Path::new("/cache/anina/pristine/audit-tool.a"));
    assert_ne!(pristine_of(&sb.join("x")), sb.join("x.pristine"));
}

#[test]
fn put_back_undoes_what_a_play_did_and_nothing_else() {
    // review of #58, 3 and 4: chmod, an edit that kept size and mtime, a
    // name that is not UTF-8, inside a read-only folder too
    use std::os::unix::ffi::OsStrExt;
    let dir = std::env::temp_dir().join(format!("anina-putback-{}", std::process::id()));
    let _ = remove_tree(&dir);
    let (root, saved) = (dir.join("root"), dir.join("saved"));
    std::fs::create_dir_all(root.join("work/bin")).unwrap();
    std::fs::create_dir_all(root.join("gocache/m")).unwrap();
    std::fs::write(root.join("work/bin/run"), "#!/bin/sh").unwrap();
    std::fs::write(root.join("work/conf"), "aaaa").unwrap();
    std::fs::write(root.join("gocache/m/go.mod"), "module m").unwrap();
    // a build in the next terminal compares these times
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_577_836_800);
    for p in ["work/conf", "work/bin/run", "work/bin", "work"] {
        let f = std::fs::File::open(root.join(p)).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(old).set_accessed(old)).unwrap();
    }
    std::fs::set_permissions(root.join("gocache/m"), std::fs::Permissions::from_mode(0o555)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    copy_tree(&root, &saved).unwrap();
    let base = Snapshot::take(&root);
    let want = listing(&saved);
    assert_eq!(want, listing(&root), "freeze keeps every time");
    std::thread::sleep(std::time::Duration::from_millis(20));

    std::fs::set_permissions(root.join("work/bin/run"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(root.join("work/bin"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mtime = std::fs::metadata(root.join("work/conf")).unwrap().modified().unwrap();
    std::fs::write(root.join("work/conf"), "bbbb").unwrap();
    std::fs::File::options().write(true).open(root.join("work/conf")).unwrap().set_modified(mtime).unwrap();
    let _ = std::fs::write(root.join("work").join(std::ffi::OsStr::from_bytes(b"caf\xe9")), "x");
    std::fs::set_permissions(root.join("gocache/m"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("gocache/m/extra"), "y").unwrap();
    std::fs::set_permissions(root.join("gocache/m"), std::fs::Permissions::from_mode(0o555)).unwrap();

    let mut now = Snapshot::take(&root);
    let changed = base.differs(&now);
    put_back(&root, &saved, &changed).unwrap();
    assert_eq!(listing(&root), want);
    now.refresh(&root, &changed);
    assert!(now.differs(&Snapshot::take(&root)).is_empty());
    remove_tree(&dir).unwrap();
}

#[test]
fn a_tree_with_sockets_fifos_and_read_only_folders_copies_and_goes() {
    let dir = std::env::temp_dir().join(format!("anina-tree-{}", std::process::id()));
    let _ = remove_tree(&dir);
    let (from, to) = (dir.join("from"), dir.join("to"));
    std::fs::create_dir_all(from.join("pkg/mod/x@v1")).unwrap();
    std::fs::write(from.join("pkg/mod/x@v1/go.mod"), "module x").unwrap();
    let _sock = std::os::unix::net::UnixListener::bind(from.join("agent.sock")).unwrap();
    assert!(std::process::Command::new("mkfifo").arg(from.join("pipe")).status().unwrap().success());
    std::os::unix::fs::symlink("pkg", from.join("link")).unwrap();
    for d in ["pkg/mod/x@v1", "pkg/mod"] {
        std::fs::set_permissions(from.join(d), std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    copy_tree(&from, &to).unwrap();
    assert_eq!(std::fs::read_to_string(to.join("pkg/mod/x@v1/go.mod")).unwrap(), "module x");
    assert!(std::fs::symlink_metadata(to.join("link")).unwrap().file_type().is_symlink());
    assert!(!to.join("agent.sock").exists() && !to.join("pipe").exists());
    let mode = std::fs::metadata(to.join("pkg/mod")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o555);

    assert!(std::fs::remove_dir_all(&to).is_err());
    remove_tree(&to).unwrap();
    remove_tree(&dir).unwrap();
    assert!(!dir.exists());
}
