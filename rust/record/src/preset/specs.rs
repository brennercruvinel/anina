//! `preset.toml`: what a preset needs, how it prepares, and its parts.
//!
//! ```toml
//! name = "burn"
//! description = "Burn's training dashboard, from clone to stopped run"
//! homepage = "https://burn.dev"
//! sandbox = "env"                 # off | env | strict
//! theme = "theme.json"            # optional, next to preset.toml
//! network = true                  # informational
//!
//! [[requires]]
//! program = "cargo"
//! hint = "install Rust: https://rustup.rs"
//!
//! [[vars]]                        # the environment wins over `default`
//! name = "BURN_DIR"
//! default = "${ANINA_CACHE}/burn"
//! about = "a burn checkout (cloned when missing)"
//!
//! [[prepare]]                     # sh, before the parts; skipped when `creates` exists
//! about = "clone burn"
//! run = "git clone --depth 1 https://github.com/tracel-ai/burn \"$BURN_DIR\""
//! creates = "${BURN_DIR}/Cargo.toml"
//!
//! [[redact]]
//! pattern = "run-[0-9a-f]{8}"
//! with = "run-••••"
//!
//! [[parts]]
//! name = "train"
//! tape = "train.tape"
//! title = "Training dashboard"
//! needs = ["BURN_DIR"]            # skipped (with a note) while a var is empty
//! ```

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub sandbox: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub requires: Vec<Require>,
    #[serde(default)]
    pub vars: Vec<Var>,
    #[serde(default)]
    pub prepare: Vec<Prepare>,
    #[serde(default)]
    pub redact: Vec<Redact>,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Require {
    pub program: String,
    #[serde(default)]
    pub hint: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Var {
    pub name: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub about: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    pub run: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub creates: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redact {
    pub pattern: String,
    pub with: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub name: String,
    pub tape: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub needs: Vec<String>,
}

impl Manifest {
    pub fn load(dir: &Path) -> Result<Manifest> {
        let path = dir.join("preset.toml");
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let m: Manifest = toml::from_str(&text).with_context(|| format!("{} is not a valid preset", path.display()))?;
        for p in &m.parts {
            anyhow::ensure!(dir.join(&p.tape).is_file(), "{}: part {} has no tape {}", path.display(), p.name, p.tape);
        }
        Ok(m)
    }

    /// The parts named in `names`, or every part when it is empty. Only reads
    /// the manifest, so a wrong `--part` fails before any work.
    pub fn pick(&self, names: &[String]) -> Result<Vec<&Part>> {
        if names.is_empty() {
            return Ok(self.parts.iter().collect());
        }
        names
            .iter()
            .map(|p| {
                self.parts.iter().find(|x| &x.name == p).ok_or_else(|| {
                    anyhow!(
                        "{} has no part {p:?}; parts: {}",
                        self.name,
                        self.parts.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ")
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_names_a_wrong_part_from_the_manifest_alone() {
        let m: Manifest = toml::from_str(
            r#"name = "burn"
[[parts]]
name = "train"
tape = "train.tape"
[[parts]]
name = "stop"
tape = "stop.tape""#,
        )
        .unwrap();
        assert_eq!(m.pick(&[]).unwrap().len(), 2);
        assert_eq!(m.pick(&["stop".into()]).unwrap()[0].name, "stop");
        let e = m.pick(&["trian".into()]).unwrap_err().to_string();
        assert_eq!(e, r#"burn has no part "trian"; parts: train, stop"#);
    }
}
