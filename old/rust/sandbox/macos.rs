//! macOS: `sandbox-exec` with a generated profile. Everything is allowed
//! (reads, network, processes) except writing outside the sandbox root, the
//! devices and the per-user temp dirs.

use std::path::Path;

/// The SBPL profile for a sandbox rooted at `root` (a real, canonical path).
pub fn profile(root: &Path) -> String {
    let q = |p: &str| p.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "(version 1)\n\
         (allow default)\n\
         (deny file-write*)\n\
         (allow file-write*\n\
         \x20 (subpath \"{}\")\n\
         \x20 (subpath \"/dev\")\n\
         \x20 (subpath \"/private/var/folders\")\n\
         \x20 (literal \"/private/tmp\"))\n",
        q(&root.display().to_string())
    )
}

/// The argv prefix: `sandbox-exec -p PROFILE`.
pub fn wrap(root: &Path) -> Vec<String> {
    vec!["/usr/bin/sandbox-exec".into(), "-p".into(), profile(root)]
}
