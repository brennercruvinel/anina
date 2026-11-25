//! macOS: `sandbox-exec` with a generated profile. Everything is allowed
//! (reads, processes, talking to other programs) except two things: writing
//! outside the sandbox root, the devices and the per-user temp dirs, and,
//! unless the run declares it, the network.

use std::path::{Path, PathBuf};

/// The SBPL profile for a sandbox rooted at `root`, with `more` folders
/// writable besides it (real, canonical paths: the rules match those).
///
/// Without `network`, no address is reachable, the loopback included. A
/// socket file under the root still is: a program talks to its own helpers.
pub fn profile(root: &Path, more: &[PathBuf], network: bool) -> String {
    let q = |p: &Path| p.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    let root = q(root);
    let mut p = format!(
        "(version 1)\n\
         (allow default)\n\
         (deny file-write*)\n\
         (allow file-write*\n\
         \x20 (subpath \"{root}\")\n"
    );
    for m in more {
        p.push_str(&format!("  (subpath \"{}\")\n", q(m)));
    }
    p.push_str(
        "\x20 (subpath \"/dev\")\n\
         \x20 (subpath \"/private/var/folders\")\n\
         \x20 (literal \"/private/tmp\"))\n",
    );
    if !network {
        p.push_str(&format!(
            "(deny network*)\n\
             (allow network*\n\
             \x20 (local unix-socket (subpath \"{root}\"))\n\
             \x20 (remote unix-socket (subpath \"{root}\")))\n"
        ));
    }
    p
}

/// The argv prefix: `sandbox-exec -p PROFILE`.
pub fn wrap(root: &Path, more: &[PathBuf], network: bool) -> Vec<String> {
    vec!["/usr/bin/sandbox-exec".into(), "-p".into(), profile(root, more, network)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_ONLY: &str = r#"(version 1)
(allow default)
(deny file-write*)
(allow file-write*
  (subpath "/c/sandbox/x")
  (subpath "/dev")
  (subpath "/private/var/folders")
  (literal "/private/tmp"))
"#;
    const NO_NETWORK: &str = r#"(deny network*)
(allow network*
  (local unix-socket (subpath "/c/sandbox/x"))
  (remote unix-socket (subpath "/c/sandbox/x")))
"#;

    #[test]
    fn the_profile_denies_the_network_unless_it_is_declared() {
        let root = Path::new("/c/sandbox/x");
        assert_eq!(profile(root, &[], true), ROOT_ONLY);
        assert_eq!(profile(root, &[], false), format!("{ROOT_ONLY}{NO_NETWORK}"));
    }

    #[test]
    fn more_folders_are_writable_besides_the_root() {
        let p = profile(Path::new("/c/sandbox/x"), &[PathBuf::from("/c")], true);
        assert!(p.contains("  (subpath \"/c/sandbox/x\")\n  (subpath \"/c\")\n  (subpath \"/dev\")\n"), "{p}");
    }

    #[test]
    fn a_quote_or_a_backslash_in_a_path_stays_inside_its_string() {
        let p = profile(Path::new("/c/a\"b\\c"), &[PathBuf::from("/d/\")(allow default)(\"")], true);
        assert!(p.contains(r#"(subpath "/c/a\"b\\c")"#), "{p}");
        assert!(p.contains(r#"(subpath "/d/\")(allow default)(\"")"#), "{p}");
        assert_eq!(wrap(Path::new("/r"), &[], false).len(), 3);
    }
}
