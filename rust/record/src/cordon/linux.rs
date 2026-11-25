//! Linux: bubblewrap. The whole filesystem is mounted read-only, the root is
//! bound writable, /tmp is a fresh tmpfs, /dev is a new one, the program sees
//! only its own processes (a process list shows nothing of the machine's),
//! and it dies with anina.
//!
//! HOME and TMPDIR are not set here: the program gets them from the
//! sandbox's environment, the same way on macOS.

use std::path::{Path, PathBuf};

/// The argv prefix: `bwrap ... --`, with `more` folders writable besides
/// `root`.
pub fn wrap(root: &Path, more: &[PathBuf]) -> Vec<String> {
    let mut v: Vec<String> =
        ["bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--unshare-pid", "--proc", "/proc", "--tmpfs", "/tmp"]
            .iter()
            .map(|x| x.to_string())
            .collect();
    // after the tmpfs: a folder under /tmp is bound over it. The root last,
    // so it is writable whatever holds it.
    for dir in more.iter().map(PathBuf::as_path).chain([root]) {
        let s = dir.display().to_string();
        v.extend(["--bind".into(), s.clone(), s]);
    }
    v.extend(["--die-with-parent".into(), "--".into()]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "bwrap --ro-bind / / --dev /dev --unshare-pid --proc /proc --tmpfs /tmp";

    #[test]
    fn the_argv_binds_the_root_and_what_else_is_writable() {
        let root = Path::new("/c/sandbox/x");
        let cache = [PathBuf::from("/c")];
        let tail = "--die-with-parent --";
        for (more, want) in [
            (&[][..], format!("{HEAD} --bind /c/sandbox/x /c/sandbox/x {tail}")),
            (&cache[..], format!("{HEAD} --bind /c /c --bind /c/sandbox/x /c/sandbox/x {tail}")),
        ] {
            assert_eq!(wrap(root, more).join(" "), want, "more {more:?}");
        }
    }

    #[test]
    fn a_path_with_a_space_is_one_argument() {
        let v = wrap(Path::new("/c/my sandbox"), &[]);
        assert_eq!(v.iter().filter(|a| *a == "/c/my sandbox").count(), 2);
        assert_eq!(v.last().map(String::as_str), Some("--"));
    }
}
