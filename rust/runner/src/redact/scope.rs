//! What one command shares from its start to the check at its end: the
//! redactor every other one starts from, with the count of what was hidden,
//! and the folders it writes to. The caller makes one and passes it down;
//! nothing here outlives it, so two commands in one process (two tests) do
//! not see each other.

use super::{Redactor, config_paths};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct Scope {
    standard: Redactor,
    outputs: Mutex<Vec<PathBuf>>,
}

impl Scope {
    /// The rules of the config files, then this machine's identity and the
    /// secret shapes. A broken config is an error here, before anything runs.
    pub fn load() -> Result<Scope> {
        let mut r = Redactor::default();
        for path in config_paths() {
            r.load_config(&path)?;
        }
        r.rules.extend(Redactor::identity().rules);
        Ok(Scope::of(r))
    }

    /// A scope that starts from `standard`.
    pub fn of(standard: Redactor) -> Scope {
        Scope { standard, outputs: Mutex::default() }
    }

    /// The rules every run gets. A clone of it, or a `blank` with rules put
    /// before these, counts what it hides where this one does.
    pub fn redactor(&self) -> &Redactor {
        &self.standard
    }

    /// Note `dir` as a folder this command writes to, once it exists. When
    /// the command ends, worked or failed, `anina` checks what was written
    /// there: a failed wait leaves its screen in `failed.txt` too.
    pub fn writes_to(&self, dir: &Path) {
        if let Ok(mut v) = self.outputs.lock()
            && !v.iter().any(|d| d == dir)
        {
            v.push(dir.to_path_buf());
        }
    }

    /// The folders noted by `writes_to`, in order.
    pub fn outputs(&self) -> Vec<PathBuf> {
        self.outputs.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        let mut r = Redactor::default();
        r.add("home path", "/Users/alice", Some("~")).unwrap();
        Scope::of(r)
    }

    #[test]
    fn a_scope_counts_what_its_own_redactors_hid_and_nothing_else() {
        let (a, b) = (scope(), scope());
        assert_eq!(a.redactor().summary(), None);
        // a clone, and a blank with a rule that goes first, count with the scope they came from
        let mut first = a.redactor().blank();
        first.push("run-[0-9]+", "run-N").unwrap();
        first.rules.extend(a.redactor().rules.clone());
        assert_eq!(first.text("run-7 in /Users/alice/x"), "run-N in ~/x");
        a.redactor().clone().text("/Users/alice/y and /Users/alice/z");
        assert_eq!(a.redactor().summary().as_deref(), Some("home path ×1, rule ×1"));
        // the terminal's copy of a line is not counted, and the other scope saw nothing
        assert_eq!(b.redactor().quiet("/Users/alice"), "~");
        assert_eq!(b.redactor().summary(), None);
    }

    #[test]
    fn a_scope_keeps_the_folders_written_to_once_each_in_order() {
        let (a, b) = (scope(), scope());
        for dir in ["out/b", "out/a", "out/b"] {
            a.writes_to(Path::new(dir));
        }
        assert_eq!(a.outputs(), [PathBuf::from("out/b"), PathBuf::from("out/a")]);
        assert!(b.outputs().is_empty());
    }
}
