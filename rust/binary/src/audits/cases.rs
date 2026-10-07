//! The terminals an audit runs the journey in. By default one axis moves at
//! a time from a baseline (a roomy truecolor UTF-8 terminal), so each
//! finding has one cause; `--full` runs the whole cross product.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Colors {
    /// COLORTERM=truecolor
    Truecolor,
    /// TERM=xterm-256color, no COLORTERM
    C256,
    /// TERM=xterm, no COLORTERM
    C16,
    /// NO_COLOR=1 (https://no-color.org)
    NoColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    /// whatever the tape and the host give (normally UTF-8)
    Host,
    /// LANG=C LC_ALL=C
    C,
}

#[derive(Clone, Debug, Serialize)]
pub struct Variant {
    pub name: String,
    pub cols: u16,
    pub rows: u16,
    pub colors: Colors,
    pub locale: Locale,
    pub baseline: bool,
}

impl Variant {
    fn new(cols: u16, rows: u16, colors: Colors, locale: Locale) -> Self {
        let c = match colors {
            Colors::Truecolor => "truecolor",
            Colors::C256 => "256",
            Colors::C16 => "16",
            Colors::NoColor => "nocolor",
        };
        let l = match locale {
            Locale::Host => "",
            Locale::C => "-C",
        };
        Variant { name: format!("{cols}x{rows}-{c}{l}"), cols, rows, colors, locale, baseline: false }
    }

    /// The environment that puts the program in this terminal. Unsetting is
    /// done by an empty value, which programs treat as unset.
    pub fn env(&self) -> Vec<String> {
        let mut e: Vec<String> = match self.colors {
            Colors::Truecolor => vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into(), "NO_COLOR=".into()],
            Colors::C256 => vec!["TERM=xterm-256color".into(), "COLORTERM=".into(), "NO_COLOR=".into()],
            Colors::C16 => vec!["TERM=xterm".into(), "COLORTERM=".into(), "NO_COLOR=".into()],
            Colors::NoColor => vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into(), "NO_COLOR=1".into()],
        };
        if self.locale == Locale::C {
            e.extend(["LANG=C".into(), "LC_ALL=C".into(), "LC_CTYPE=C".into()]);
        }
        e
    }

    /// `Set` overrides for the recorder.
    pub fn force(&self, fps: f64) -> Vec<(String, Vec<String>)> {
        vec![
            ("cols".into(), vec![self.cols.to_string()]),
            ("rows".into(), vec![self.rows.to_string()]),
            ("fps".into(), vec![fps.to_string()]),
            ("env".into(), self.env()),
        ]
    }
}

pub const BASE: (u16, u16) = (120, 40);
pub const SIZES: [(u16, u16); 2] = [(80, 24), (60, 20)];

/// Parse `80x24,120x40`.
pub fn parse_sizes(list: &[String]) -> anyhow::Result<Vec<(u16, u16)>> {
    list.iter()
        .map(|s| {
            let (c, r) =
                s.trim().split_once(['x', 'X']).ok_or_else(|| anyhow::anyhow!("size {s:?} is not COLSxROWS"))?;
            Ok((c.parse()?, r.parse()?))
        })
        .collect()
}

/// The variants to run. The first one is the baseline.
pub fn variants(sizes: &[(u16, u16)], quick: bool, full: bool) -> Vec<Variant> {
    let mut base = Variant::new(BASE.0, BASE.1, Colors::Truecolor, Locale::Host);
    base.baseline = true;
    let sizes: Vec<(u16, u16)> = if sizes.is_empty() { SIZES.to_vec() } else { sizes.to_vec() };
    let mut v = vec![base];
    if full {
        let mut all_sizes = vec![BASE];
        all_sizes.extend(sizes.iter().filter(|s| **s != BASE));
        for (c, r) in all_sizes {
            for colors in [Colors::Truecolor, Colors::C256, Colors::C16, Colors::NoColor] {
                for locale in [Locale::Host, Locale::C] {
                    let x = Variant::new(c, r, colors, locale);
                    if x.name != v[0].name {
                        v.push(x);
                    }
                }
            }
        }
        return v;
    }
    let (bc, br) = BASE;
    if quick {
        // the smallest size, no color, 16 colors
        let small = sizes.iter().min_by_key(|(c, r)| *c as u32 * *r as u32).copied().unwrap_or((60, 20));
        v.push(Variant::new(small.0, small.1, Colors::Truecolor, Locale::Host));
        v.push(Variant::new(bc, br, Colors::NoColor, Locale::Host));
        v.push(Variant::new(bc, br, Colors::C16, Locale::Host));
        return v;
    }
    for (c, r) in sizes.iter().filter(|s| **s != BASE) {
        v.push(Variant::new(*c, *r, Colors::Truecolor, Locale::Host));
    }
    for colors in [Colors::C256, Colors::C16, Colors::NoColor] {
        v.push(Variant::new(bc, br, colors, Locale::Host));
    }
    v.push(Variant::new(bc, br, Colors::Truecolor, Locale::C));
    v
}
