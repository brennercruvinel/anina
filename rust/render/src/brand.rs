//! A theme derived from the program's own colors: the ground it paints, the
//! ink it writes with and its most used saturated color as the accent, with
//! the window, captions and progress bar built around them. This is the
//! "auto" theme: a recording looks like its program with no configuration.

use crate::theme::Theme;
use anina_screen::frames::Cast;
use anina_screen::{Color, Screen};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// A cast to read colors from
    pub cast: PathBuf,
    /// Where to write the theme JSON (default: stdout)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

pub fn run(a: Args) -> Result<()> {
    let cast = Cast::load(&a.cast)?;
    let th = derive(&cast);
    let json = serde_json::to_string_pretty(&th)?;
    match a.out {
        Some(p) => {
            std::fs::write(&p, json + "\n").with_context(|| format!("writing {}", p.display()))?;
            eprintln!(
                "anina: theme -> {} (bg {}, fg {}, accent {})",
                p.display(),
                th.terminal.bg,
                th.terminal.fg,
                th.caption.accent
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

/// The theme of a cast's own colors (up to 64 screens, spread evenly).
pub fn derive(cast: &Cast) -> Theme {
    let n = cast.screens.len();
    let step = (n / 64).max(1);
    let screens: Vec<Screen> = (0..n).step_by(step).map(|i| cast.screen(i)).collect();
    derive_screens(&screens, &cast.title)
}

/// The theme of `screens`' colors; `title`'s first word becomes the brand
/// when it reads like a name.
pub fn derive_screens(screens: &[Screen], title: &str) -> Theme {
    let base = Theme::default_theme();
    let mut bgs: HashMap<Color, usize> = HashMap::new();
    let mut fgs: HashMap<Color, usize> = HashMap::new();
    for s in screens {
        for c in &s.cells {
            *bgs.entry(c.bg).or_default() += 1;
            if !c.ch.trim().is_empty() {
                *fgs.entry(c.fg).or_default() += 1;
            }
        }
    }
    let bg = bgs
        .iter()
        .max_by_key(|(c, n)| (**n, format!("{c:?}")))
        .map_or(base.terminal.bg.clone(), |(c, _)| base.color(*c, false));
    // text the program leaves in the terminal's default color gets a
    // readable ink for this ground (the real terminal's is unknown)
    let default_ink = if luminance(&bg) < 0.4 { "#e6edf3" } else { "#1f2328" }.to_string();
    let resolve = |c: Color| if c == Color::Default { default_ink.clone() } else { base.color(c, true) };
    // the chrome's ink: the most used color that reads well on the ground
    // (dim decoration like a hash rain can outnumber the text)
    let ink = fgs
        .iter()
        .map(|(c, n)| (resolve(*c), *n))
        .filter(|(h, _)| contrast(h, &bg) >= 4.5)
        .max_by_key(|(h, n)| (*n, h.clone()))
        .map_or(default_ink.clone(), |(h, _)| h);
    // the accent: the most used saturated color that stands out from the ground
    let mut accents: HashMap<String, usize> = HashMap::new();
    for s in screens {
        for c in &s.cells {
            let ink = !c.ch.trim().is_empty();
            for (col, is_fg) in [(c.fg, true), (c.bg, false)] {
                if (is_fg && !ink) || col == Color::Default {
                    continue;
                }
                let hex = base.color(col, is_fg);
                if saturation(&hex) >= 0.3 && contrast(&hex, &bg) >= 1.8 {
                    *accents.entry(hex).or_default() += 1;
                }
            }
        }
    }
    let accent =
        accents.into_iter().max_by_key(|(h, n)| (*n, h.clone())).map_or(base.caption.accent.clone(), |(h, _)| h);
    build(&base, &bg, &default_ink, &ink, &accent, title)
}

/// `term_fg` is the terminal default ink, `fg` the chrome's.
fn build(base: &Theme, bg: &str, term_fg: &str, fg: &str, accent: &str, title: &str) -> Theme {
    let mut t = base.clone();
    let dark = luminance(bg) < 0.4;
    t.name = "auto".into();
    t.about = "derived from the program's own colors".into();
    t.window.bg = if dark {
        vec![mix(bg, fg, 0.10), bg.into(), mix(bg, "#000000", 0.35)]
    } else {
        vec![mix(bg, "#ffffff", 0.5), bg.into(), mix(bg, "#000000", 0.08)]
    };
    t.window.bar = vec![mix(bg, fg, 0.12), mix(bg, fg, 0.06)];
    t.window.title_color = mix(fg, bg, 0.45);
    t.terminal.fg = term_fg.into();
    t.terminal.bg = bg.into();
    t.terminal.border = mix(bg, fg, 0.18);
    t.terminal.cursor = accent.into();
    t.caption.bg = mix(bg, fg, 0.07);
    t.caption.fg = fg.into();
    t.caption.accent = readable_on(accent, &t.caption.bg);
    t.caption.muted = mix(fg, bg, 0.5);
    t.progress.track = mix(bg, fg, 0.15);
    t.progress.fill = accent.into();
    t.brand.text = brand_word(title);
    t.brand.gradient = vec![accent.into(), mix(accent, fg, 0.35)];
    t.brand.tagline = String::new();
    t
}

pub(crate) fn rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0;
    if h.len() == 3 {
        let q = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).unwrap_or(0) as f64 / 255.0;
        return (q(0), q(1), q(2));
    }
    (p(0), p(2), p(4))
}

fn hex((r, g, b): (f64, f64, f64)) -> String {
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

/// `a` moved `k` of the way to `b`.
pub fn mix(a: &str, b: &str, k: f64) -> String {
    let (x, y) = (rgb(a), rgb(b));
    hex((x.0 + (y.0 - x.0) * k, x.1 + (y.1 - x.1) * k, x.2 + (y.2 - x.2) * k))
}

/// WCAG relative luminance.
pub fn luminance(c: &str) -> f64 {
    let lin = |v: f64| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = rgb(c);
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG contrast ratio, 1 to 21.
pub fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn saturation(c: &str) -> f64 {
    let (r, g, b) = rgb(c);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min { 0.0 } else { (max - min) / (1.0 - (2.0 * l - 1.0).abs()) }
}

/// `color`, darkened or lightened until it reads on `bg` (contrast >= 3).
pub fn readable_on(color: &str, bg: &str) -> String {
    let target = if luminance(bg) > 0.5 { "#000000" } else { "#ffffff" };
    let mut c = color.to_string();
    for i in 1..=10 {
        if contrast(&c, bg) >= 3.0 {
            break;
        }
        c = mix(color, target, i as f64 * 0.1);
    }
    c
}

/// The first word of a title when it reads like a name (`anina — from zero`
/// -> `anina`); none for titles that start with an article or a short word
/// (`a shell, recorded by anina`).
fn brand_word(title: &str) -> String {
    const SKIP: &[&str] = &["the", "and", "any", "how", "with", "from", "this", "that", "your", "our", "demo"];
    let w = title
        .split(|c: char| c.is_whitespace() || matches!(c, '·' | '—' | ':' | ','))
        .find(|w| !w.is_empty())
        .unwrap_or("");
    if w.chars().count() < 3 || SKIP.contains(&w.to_lowercase().as_str()) {
        return String::new();
    }
    w.to_string()
}
