//! macOS: `sandbox-exec` with a generated profile. Everything is allowed
//! (reads, network, processes) except writing outside the sandbox root, the
//! devices and the per-user temp dirs.

use std::path::{Path, PathBuf};

/// The SBPL profile for a sandbox rooted at `root`, with `more` folders
/// writable besides it (real, canonical paths: the rules match those).
pub fn profile(root: &Path, more: &[PathBuf]) -> String {
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
    p
}

/// The argv prefix: `sandbox-exec -p PROFILE`.
pub fn wrap(root: &Path, more: &[PathBuf]) -> Vec<String> {
    vec!["/usr/bin/sandbox-exec".into(), "-p".into(), profile(root, more)]
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
    #[test]
    fn the_profile_allows_writes_under_the_root_and_the_system_s_own_folders() {
        assert_eq!(profile(Path::new("/c/sandbox/x"), &[]), ROOT_ONLY);
    }

    #[test]
    fn more_folders_are_writable_besides_the_root() {
        let p = profile(Path::new("/c/sandbox/x"), &[PathBuf::from("/c")]);
        assert!(p.contains("  (subpath \"/c/sandbox/x\")\n  (subpath \"/c\")\n  (subpath \"/dev\")\n"), "{p}");
    }

    #[test]
    fn a_quote_or_a_backslash_in_a_path_stays_inside_its_string() {
        let p = profile(Path::new("/c/a\"b\\c"), &[PathBuf::from("/d/\")(allow default)(\"")]);
        assert!(p.contains(r#"(subpath "/c/a\"b\\c")"#), "{p}");
        assert!(p.contains(r#"(subpath "/d/\")(allow default)(\"")"#), "{p}");
        assert_eq!(wrap(Path::new("/r"), &[]).len(), 3);
    }
}
