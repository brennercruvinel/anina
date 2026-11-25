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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[Variant]) -> Vec<&str> {
        v.iter().map(|x| x.name.as_str()).collect()
    }

    /// How many of size, colors and locale differ between two terminals.
    fn axes_apart(a: &Variant, b: &Variant) -> usize {
        [(a.cols, a.rows) != (b.cols, b.rows), a.colors != b.colors, a.locale != b.locale]
            .iter()
            .filter(|d| **d)
            .count()
    }

    #[test]
    fn the_default_matrix_moves_one_axis_at_a_time_from_the_baseline() {
        let v = variants(&[], false, false);
        assert_eq!(
            names(&v),
            [
                "120x40-truecolor",
                "80x24-truecolor",
                "60x20-truecolor",
                "120x40-256",
                "120x40-16",
                "120x40-nocolor",
                "120x40-truecolor-C"
            ]
        );
        // the baseline is first, alone, a roomy truecolor terminal in the host's locale
        assert_eq!(v.iter().map(|x| x.baseline).collect::<Vec<_>>(), [true, false, false, false, false, false, false]);
        assert_eq!((v[0].cols, v[0].rows, v[0].colors, v[0].locale), (120, 40, Colors::Truecolor, Locale::Host));
        for x in &v[1..] {
            assert_eq!(axes_apart(&v[0], x), 1, "{}", x.name);
        }
    }

    #[test]
    fn sizes_given_replace_the_two_usual_ones() {
        let v = variants(&[(100, 30), (40, 10)], false, false);
        assert_eq!(names(&v)[..3], ["120x40-truecolor", "100x30-truecolor", "40x10-truecolor"]);
        assert_eq!(v.len(), 7);
        // the baseline's own size is not run twice
        let v = variants(&[(120, 40)], false, false);
        assert_eq!(names(&v), ["120x40-truecolor", "120x40-256", "120x40-16", "120x40-nocolor", "120x40-truecolor-C"]);
    }

    #[test]
    fn quick_keeps_the_baseline_the_smallest_size_no_color_and_16_colors() {
        assert_eq!(
            names(&variants(&[], true, false)),
            ["120x40-truecolor", "60x20-truecolor", "120x40-nocolor", "120x40-16"]
        );
        // the smallest by area, wherever it sits in the list
        let v = variants(&[(200, 10), (40, 30), (90, 30)], true, false);
        assert_eq!(names(&v)[1], "40x30-truecolor");
        assert!(v[0].baseline && v[1..].iter().all(|x| !x.baseline));
    }

    #[test]
    fn full_is_every_size_with_every_color_depth_and_locale() {
        let v = variants(&[], false, true);
        // 3 sizes x 4 color depths x 2 locales, the baseline counted once and first
        assert_eq!(v.len(), 24);
        assert_eq!(v[0].name, "120x40-truecolor");
        assert_eq!(v.iter().filter(|x| x.baseline).count(), 1);
        let mut seen: Vec<&str> = names(&v);
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 24, "a terminal is listed twice");
        for size in [(120, 40), (80, 24), (60, 20)] {
            for colors in [Colors::Truecolor, Colors::C256, Colors::C16, Colors::NoColor] {
                for locale in [Locale::Host, Locale::C] {
                    let n = v.iter().filter(|x| ((x.cols, x.rows), x.colors, x.locale) == (size, colors, locale));
                    assert_eq!(n.count(), 1, "{size:?} {colors:?} {locale:?}");
                }
            }
        }
        assert_eq!(variants(&[(120, 40), (80, 24)], false, true).len(), 16);
    }

    #[test]
    fn a_terminal_announces_its_colors_and_locale_through_the_environment() {
        let all = variants(&[], false, true);
        let env = |name: &str| all.iter().find(|x| x.name == name).map(Variant::env).expect(name);
        // an empty value is how a variable is unset
        assert_eq!(env("120x40-truecolor"), ["TERM=xterm-256color", "COLORTERM=truecolor", "NO_COLOR="]);
        assert_eq!(env("120x40-256"), ["TERM=xterm-256color", "COLORTERM=", "NO_COLOR="]);
        assert_eq!(env("120x40-16"), ["TERM=xterm", "COLORTERM=", "NO_COLOR="]);
        assert_eq!(env("120x40-nocolor"), ["TERM=xterm-256color", "COLORTERM=truecolor", "NO_COLOR=1"]);
        assert_eq!(env("80x24-16-C"), ["TERM=xterm", "COLORTERM=", "NO_COLOR=", "LANG=C", "LC_ALL=C", "LC_CTYPE=C"]);
    }

    #[test]
    fn a_terminal_forces_its_size_rate_and_environment_on_the_recorder() {
        let v = variants(&[], false, false);
        let small = &v[2];
        let want: Vec<(String, Vec<String>)> = vec![
            ("cols".into(), vec!["60".into()]),
            ("rows".into(), vec!["20".into()]),
            ("fps".into(), vec!["60".into()]),
            ("env".into(), small.env()),
        ];
        assert_eq!(small.force(60.0), want);
    }

    #[test]
    fn sizes_are_read_as_cols_x_rows() {
        let parse = |list: &[&str]| parse_sizes(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(parse(&["80x24", "100X30", " 132x43 "]).unwrap(), [(80, 24), (100, 30), (132, 43)]);
        assert_eq!(parse(&[]).unwrap(), []);
        let err = parse(&["80"]).unwrap_err().to_string();
        assert!(err.contains("\"80\"") && err.contains("COLSxROWS"), "{err}");
        for bad in ["x24", "80x", "wide x tall", "80x24x2", "70000x24", "-80x24"] {
            assert!(parse(&[bad]).is_err(), "{bad:?} was read as a size");
        }
    }
}
