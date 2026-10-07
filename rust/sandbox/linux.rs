//! Linux: bubblewrap. The whole filesystem is mounted read-only, the root is
//! bound writable, /tmp is a fresh tmpfs, the program sees only its own
//! processes (a process list shows nothing of the machine's), and it dies
//! with anina.

use std::path::Path;

/// The argv prefix: `bwrap ... --`.
pub fn wrap(root: &Path, home: &Path, tmp: &Path) -> Vec<String> {
    let s = |p: &Path| p.display().to_string();
    let mut v: Vec<String> =
        ["bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--unshare-pid", "--proc", "/proc", "--tmpfs", "/tmp"]
            .iter()
            .map(|x| x.to_string())
            .collect();
    v.extend(["--bind".into(), s(root), s(root)]);
    v.extend(["--setenv".into(), "HOME".into(), s(home)]);
    v.extend(["--setenv".into(), "TMPDIR".into(), s(tmp)]);
    v.extend(["--die-with-parent".into(), "--".into()]);
    v
}
