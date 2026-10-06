//! Themes: the window around the terminal, the brand in its title bar, the
//! caption and progress bars, and the 16 ANSI colors. A theme is a JSON file
//! (`themes/default.json` is built in), may `extends` another, and can be
//! derived from the program's own colors ("auto", see `crate::brand`).
//! charmbracelet/vhs theme entries ({background, foreground, black...}) load
//! too, as a palette over the default look.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const DEFAULT: &str = include_str!("../../themes/default.json");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Window {
    /// Background gradient, top-left to bottom-right.
    pub bg: Vec<String>,
    /// Title bar gradient, top to bottom.
    pub bar: Vec<String>,
    pub title_color: String,
    pub radius: f64,
    pub pad: f64,
    pub bar_h: f64,
    /// The three traffic lights.
    pub lights: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Brand {
    #[serde(default)]
    pub text: String,
    #[serde(default = "brand_size")]
    pub font_size: f64,
    #[serde(default)]
    pub gradient: Vec<String>,
    /// Shown in the caption bar while there is no caption.
    #[serde(default)]
    pub tagline: String,
}

fn brand_size() -> f64 {
    15.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terminal {
    pub fg: String,
    pub bg: String,
    #[serde(default)]
    pub border: String,
    pub cursor: String,
    #[serde(default = "radius")]
    pub radius: f64,
    pub font_family: String,
    pub font_size: f64,
    pub cell_w: f64,
    pub cell_h: f64,
    /// ANSI colors 0-15.
    pub palette16: Vec<String>,
}

fn radius() -> f64 {
    8.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Caption {
    pub height: f64,
    pub font_size: f64,
    pub bg: String,
    pub fg: String,
    pub accent: String,
    pub muted: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub height: f64,
    pub track: String,
    pub fill: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub about: String,
    /// The SVG's long description (accessibility); default: generated.
    #[serde(default)]
    pub desc: String,
    /// Longest a still screen stays during playback, in seconds.
    #[serde(default = "max_hold")]
    pub max_hold: f64,
    /// Playback frame rate cap: shorter frames give their slot to the next.
    #[serde(default = "max_fps")]
    pub max_fps: f64,
    pub window: Window,
    #[serde(default)]
    pub brand: Brand,
    pub terminal: Terminal,
    pub caption: Caption,
    pub progress: Progress,
}

fn max_hold() -> f64 {
    3.0
}
fn max_fps() -> f64 {
    8.0
}

/// The built-in theme names.
pub const BUILT_IN: &[&str] = &["default"];

/// One level of the Python port's merge: objects merge key by key, the rest
/// replaces.
fn merge(base: &mut Value, over: Value) {
    if let (Value::Object(b), Value::Object(o)) = (&mut *base, over) {
        for (k, v) in o {
            match (b.get_mut(&k), v) {
                (Some(Value::Object(bo)), Value::Object(vo)) => {
                    for (k2, v2) in vo {
                        bo.insert(k2, v2);
                    }
                }
                (_, v) => {
                    b.insert(k, v);
                }
            }
        }
    }
}

fn builtin(name: &str) -> Option<Value> {
    match name {
        "default" => serde_json::from_str(DEFAULT).ok(),
        _ => None,
    }
}

/// Resolve a theme JSON value: vhs entries become a palette over the default,
/// `extends` pulls the base in (a built-in name or a path next to `dir`).
fn resolve_value(v: Value, dir: Option<&Path>, depth: usize) -> Result<Value> {
    if depth > 8 {
        bail!("theme extends nest too deep");
    }
    if v.get("background").is_some() && v.get("black").is_some() {
        return Ok(from_vhs(&v));
    }
    let Some(parent) = v.get("extends").and_then(|e| e.as_str()).map(str::to_string) else {
        return Ok(v);
    };
    let base = match builtin(&parent) {
        Some(b) => b,
        None => {
            let p = dir.map_or_else(|| Path::new(&parent).to_path_buf(), |d| d.join(&parent));
            let p = if p.exists() { p } else { p.with_extension("json") };
            read_json(&p)?
        }
    };
    let mut base = resolve_value(base, dir, depth + 1)?;
    merge(&mut base, v);
    Ok(base)
}

fn read_json(p: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(p).with_context(|| format!("reading theme {}", p.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not JSON", p.display()))
}

/// A vhs theme entry as an anina theme: its colors on the default window.
fn from_vhs(v: &Value) -> Value {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let names = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightMagenta",
        "brightCyan",
        "brightWhite",
    ];
    let mut base: Value = serde_json::from_str(DEFAULT).expect("built-in theme");
    let (bg, fg) = (s("background"), s("foreground"));
    let cursor = if s("cursor").is_empty() { fg.clone() } else { s("cursor") };
    merge(
        &mut base,
        serde_json::json!({
            "name": s("name"),
            "terminal": {"fg": fg, "bg": bg, "border": bg, "cursor": cursor, "palette16": names.iter().map(|n| s(n)).collect::<Vec<_>>()},
        }),
    );
    base
}

impl Theme {
    /// The built-in default look.
    pub fn default_theme() -> Theme {
        serde_json::from_str(DEFAULT).expect("built-in theme parses")
    }

    /// A theme by built-in name or JSON path. "auto" is not handled here:
    /// see `resolve`.
    pub fn load(spec: &str) -> Result<Theme> {
        let v = match builtin(spec) {
            Some(v) => v,
            None => {
                let p = Path::new(spec);
                if !p.exists() {
                    bail!(
                        "no theme {spec:?}: give a path to a theme JSON, \"auto\", or one of {}",
                        BUILT_IN.join(", ")
                    );
                }
                read_json(p)?
            }
        };
        let dir = Path::new(spec).parent().filter(|d| !d.as_os_str().is_empty());
        let v = resolve_value(v, dir, 0)?;
        serde_json::from_value(v).map_err(|e| anyhow!("theme {spec:?}: {e}"))
    }

    /// The theme for a cast: `flag` wins, then the cast's own; empty or
    /// "auto" derives one from the cast's colors. A cast theme that cannot be
    /// found falls back to auto (with a warning); a flag must exist.
    pub fn resolve(flag: Option<&str>, cast: &crate::cast::Cast) -> Result<Theme> {
        match flag.filter(|f| !f.is_empty()) {
            Some("auto") => Ok(crate::brand::derive(cast)),
            Some(f) => Theme::load(f),
            None if cast.theme.is_empty() || cast.theme == "auto" => Ok(crate::brand::derive(cast)),
            None => Theme::load(&cast.theme).or_else(|e| {
                eprintln!("anina: warning: {e}; using colors derived from the recording");
                Ok(crate::brand::derive(cast))
            }),
        }
    }

    /// The same theme with light window chrome around the unchanged terminal.
    pub fn light_chrome(&self) -> Theme {
        let mut t = self.clone();
        t.window.bg = vec!["#ffffff".into(), "#f4f6f8".into(), "#e8ecf0".into()];
        t.window.bar = vec!["#ffffff".into(), "#f1f3f5".into()];
        t.window.title_color = "#57606a".into();
        let accent = crate::brand::readable_on(&self.caption.accent, "#ffffff");
        t.caption.bg = "#f6f8fa".into();
        t.caption.fg = "#1f2328".into();
        t.caption.muted = "#6e7781".into();
        t.caption.accent = accent.clone();
        t.progress.track = "#d0d7de".into();
        t.progress.fill = accent;
        if !t.brand.gradient.is_empty() {
            t.brand.gradient = t.brand.gradient.iter().map(|c| crate::brand::readable_on(c, "#ffffff")).collect();
        }
        t
    }

    /// The xterm palette with this theme's 16 colors.
    pub fn color(&self, c: crate::screen::Color, fg: bool) -> String {
        use crate::screen::Color;
        match c {
            Color::Default => {
                if fg {
                    self.terminal.fg.clone()
                } else {
                    self.terminal.bg.clone()
                }
            }
            Color::Idx(n) if (n as usize) < self.terminal.palette16.len() => {
                self.terminal.palette16[n as usize].clone()
            }
            Color::Idx(n) => xterm256(n),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// The xterm 256-color palette (16-255; 0-15 come from the theme).
pub fn xterm256(n: u8) -> String {
    const BASE16: [&str; 16] = [
        "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5", "#666666", "#f14c4c",
        "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
    ];
    if n < 16 {
        return BASE16[n as usize].into();
    }
    if n < 232 {
        let n = n - 16;
        let steps = [0u8, 95, 135, 175, 215, 255];
        return format!(
            "#{:02x}{:02x}{:02x}",
            steps[(n / 36) as usize],
            steps[(n / 6 % 6) as usize],
            steps[(n % 6) as usize]
        );
    }
    let v = 8 + (n - 232) * 10;
    format!("#{v:02x}{v:02x}{v:02x}")
}
