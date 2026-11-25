//! Linux: bubblewrap. The whole filesystem is mounted read-only, the root is
//! bound writable, /tmp is a fresh tmpfs, /dev is a new one, the program sees
//! only its own processes (a process list shows nothing of the machine's),
//! it has no network unless the run declares it, and it dies with anina.
//!
//! HOME and TMPDIR are not set here: the program gets them from the
//! sandbox's environment, the same way on macOS.

use std::path::{Path, PathBuf};

/// The argv prefix: `bwrap ... --`, with `more` folders writable besides
/// `root`. Without `network` the process is alone in a network namespace:
/// its loopback is its own, the machine's is out of reach.
pub fn wrap(root: &Path, more: &[PathBuf], network: bool) -> Vec<String> {
    let mut v: Vec<String> =
        ["bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--unshare-pid", "--proc", "/proc", "--tmpfs", "/tmp"]
            .iter()
            .map(|x| x.to_string())
            .collect();
    if !network {
        v.push("--unshare-net".into());
    }
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
    fn the_argv_leaves_the_network_namespace_only_when_it_is_declared() {
        let root = Path::new("/c/sandbox/x");
        let cache = [PathBuf::from("/c")];
        let tail = "--die-with-parent --";
        for (more, network, want) in [
            (&[][..], false, format!("{HEAD} --unshare-net --bind /c/sandbox/x /c/sandbox/x {tail}")),
            (&[][..], true, format!("{HEAD} --bind /c/sandbox/x /c/sandbox/x {tail}")),
            (&cache[..], false, format!("{HEAD} --unshare-net --bind /c /c --bind /c/sandbox/x /c/sandbox/x {tail}")),
            (&cache[..], true, format!("{HEAD} --bind /c /c --bind /c/sandbox/x /c/sandbox/x {tail}")),
        ] {
            assert_eq!(wrap(root, more, network).join(" "), want, "more {more:?}, network {network}");
        }
    }

    #[test]
    fn a_path_with_a_space_is_one_argument() {
        let v = wrap(Path::new("/c/my sandbox"), &[], true);
        assert_eq!(v.iter().filter(|a| *a == "/c/my sandbox").count(), 2);
        assert_eq!(v.last().map(String::as_str), Some("--"));
    }
}
