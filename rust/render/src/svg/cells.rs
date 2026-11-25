//! One screen's rows as SVG: runs of cells with one style become text, the
//! block, box and braille glyphs become shapes, and each color gets a class.

use super::*;

pub(super) struct Renderer<'a> {
    pub(super) theme: &'a Theme,
    pub(super) cw: f64,
    pub(super) ch: f64,
    pub(super) cols: u16,
    pub(super) base: f64,
    pub(super) row_ids: HashMap<Vec<Cell>, Option<String>>,
    pub(super) row_defs: Vec<String>,
    pub(super) classes: HashMap<String, usize>,
    pub(super) class_order: Vec<String>,
    pub(super) regular: BTreeSet<char>,
    pub(super) bold: BTreeSet<char>,
}

impl<'a> Renderer<'a> {
    pub(super) fn new(theme: &'a Theme, cols: u16) -> Self {
        let (cw, ch) = (theme.terminal.cell_w, theme.terminal.cell_h);
        Renderer {
            theme,
            cw,
            ch,
            cols,
            base: (ch * 0.76).round(),
            row_ids: HashMap::new(),
            row_defs: Vec::new(),
            classes: HashMap::new(),
            class_order: Vec::new(),
            regular: BTreeSet::new(),
            bold: BTreeSet::new(),
        }
    }

    /// A short class per color: .fN fills it, .sN strokes it.
    pub(super) fn cls(&mut self, color: &str) -> usize {
        let c = color.to_ascii_lowercase();
        if let Some(n) = self.classes.get(&c) {
            return *n;
        }
        let n = self.class_order.len();
        self.classes.insert(c.clone(), n);
        self.class_order.push(c);
        n
    }

    pub(super) fn css(&self) -> String {
        self.class_order.iter().enumerate().map(|(n, c)| format!(".f{n}{{fill:{c}}}.s{n}{{stroke:{c}}}")).collect()
    }

    /// (fg, bg) of a cell, inverse applied.
    pub(super) fn colors(&self, c: &Cell) -> (String, String) {
        let fg = self.theme.color(c.fg, true);
        let bg = self.theme.color(c.bg, false);
        if c.has(attr::INVERSE) { (bg, fg) } else { (fg, bg) }
    }

    pub(super) fn note_chars(&mut self, text: &str, bold: bool) {
        let set = if bold { &mut self.bold } else { &mut self.regular };
        set.extend(text.chars().filter(|c| !c.is_whitespace()));
    }

    /// One row, drawn once in <defs> with its text baseline at y=0; `None`
    /// for an empty row.
    pub(super) fn row(&mut self, cells: &[Cell]) -> Option<String> {
        let cells = &cells[..cells.len().min(self.cols as usize)];
        if let Some(id) = self.row_ids.get(cells) {
            return id.clone();
        }
        let (cw, ch, base) = (self.cw, self.ch, self.base);
        let ground = self.theme.terminal.bg.to_ascii_lowercase();
        let mut parts: Vec<String> = Vec::new();
        // backgrounds: runs of one color that is not the terminal's ground
        let bgs: Vec<String> = cells.iter().map(|c| self.colors(c).1.to_ascii_lowercase()).collect();
        let mut x = 0;
        while x < cells.len() {
            let mut end = x;
            while end < cells.len() && bgs[end] == bgs[x] {
                end += 1;
            }
            if bgs[x] != ground {
                let bg = bgs[x].clone();
                let n = self.cls(&bg);
                parts.push(format!(
                    r#"<rect x="{}" y="-{}" width="{}" height="{}" class="f{n}"/>"#,
                    g(x as f64 * cw),
                    g(base),
                    g((end - x) as f64 * cw),
                    g(ch)
                ));
            }
            x = end;
        }
        // glyphs: text runs, braille dots, block rectangles, box lines
        let mut dots: Vec<(String, String)> = Vec::new(); // (color, path)
        let mut lines = Lines::default();
        let mut x = 0;
        while x < cells.len() {
            let c = &cells[x];
            if c.is_spacer() || c.has(attr::INVISIBLE) {
                x += 1;
                continue;
            }
            let fg = self.colors(c).0;
            if let Some(chr) = single(c) {
                if is_braille(chr) {
                    let bits = chr as u32 - 0x2800;
                    let mut d = String::new();
                    for (i, (dx, dy)) in BRAILLE_BITS.iter().enumerate() {
                        if bits & (1 << i) != 0 {
                            let px = (x as f64 + 0.27 + 0.46 * dx) * cw;
                            let py = (0.14 + 0.24 * dy) * ch - base;
                            d.push_str(&format!("M{px:.1} {py:.1}h0"));
                        }
                    }
                    push_path(&mut dots, fg, d);
                    x += 1;
                    continue;
                }
                if let Some((bx, by, bw, bh)) = block(chr) {
                    let n = self.cls(&fg);
                    parts.push(format!(
                        r#"<rect x="{}" y="{}" width="{}" height="{}" class="f{n}"/>"#,
                        g((x as f64 + bx) * cw),
                        g(by * ch - base),
                        g(bw * cw),
                        g(bh * ch)
                    ));
                    x += 1;
                    continue;
                }
                if let Some(arms) = box_arms(chr) {
                    self.box_lines(&mut lines, &fg, x, arms);
                    x += 1;
                    continue;
                }
            }
            // a run of text in one style; spacers of wide chars ride along
            let style =
                (fg.clone(), c.attrs & (attr::BOLD | attr::DIM | attr::ITALIC | attr::UNDERLINE | attr::STRIKE));
            let mut end = x;
            while end < cells.len() {
                let c2 = &cells[end];
                if c2.is_spacer() {
                    end += 1;
                    continue;
                }
                if c2.has(attr::INVISIBLE) {
                    break;
                }
                if let Some(k) = single(c2)
                    && (is_braille(k) || block(k).is_some() || box_arms(k).is_some())
                {
                    break;
                }
                let s2 = (
                    self.colors(c2).0,
                    c2.attrs & (attr::BOLD | attr::DIM | attr::ITALIC | attr::UNDERLINE | attr::STRIKE),
                );
                if s2 != style {
                    break;
                }
                end += 1;
            }
            let run = &cells[x..end];
            let bold = style.1 & attr::BOLD != 0;
            let n = self.cls(&style.0);
            let mut attrs = format!(r#"class="f{n}""#);
            if bold {
                attrs.push_str(r#" font-weight="700""#);
            }
            if style.1 & attr::DIM != 0 {
                attrs.push_str(r#" opacity="0.6""#);
            }
            if style.1 & attr::ITALIC != 0 {
                attrs.push_str(r#" font-style="italic""#);
            }
            let deco: Vec<&str> = [(attr::UNDERLINE, "underline"), (attr::STRIKE, "line-through")]
                .iter()
                .filter(|(f, _)| style.1 & f != 0)
                .map(|(_, d)| *d)
                .collect();
            if !deco.is_empty() {
                attrs.push_str(&format!(r#" text-decoration="{}""#, deco.join(" ")));
            }
            // one <text> per stretch between gaps of 2+ blanks (a decorated
            // run stays whole): renderers that collapse spaces then cannot
            // squeeze a padded label into letter-spaced text
            for (start, piece) in pieces(run, deco.is_empty()) {
                let text: String = piece
                    .iter()
                    .filter(|c| !c.is_spacer())
                    .map(|c| if c.ch == "\u{2800}" { " " } else { c.ch.as_str() })
                    .collect();
                self.note_chars(&text, bold);
                // textLength pins a run to the grid; one glyph needs only its x
                let fit = if text.chars().count() > 1 {
                    format!(r#" textLength="{}""#, g(piece.len() as f64 * cw))
                } else {
                    String::new()
                };
                parts.push(format!(r#"<text x="{}"{fit} {attrs}>{}</text>"#, g((x + start) as f64 * cw), esc(&text)));
            }
            x = end;
        }
        for (fg, d) in dots {
            let n = self.cls(&fg);
            parts.push(format!(r#"<path d="{d}" class="d s{n}"/>"#));
        }
        for (fg, class, d) in lines.paths() {
            let n = self.cls(&fg);
            parts.push(format!(r#"<path d="{d}" class="{class} s{n}"/>"#));
        }
        let id = if parts.is_empty() {
            None
        } else {
            let rid = format!("r{}", self.row_defs.len());
            self.row_defs.push(format!(r#"<g id="{rid}">{}</g>"#, parts.concat()));
            Some(rid)
        };
        self.row_ids.insert(cells.to_vec(), id.clone());
        id
    }

    /// Box drawing of one cell: segments from the cell center to its edges
    /// (straight through when both sides have the same weight), merged across
    /// cells later; rounded corners as a quadratic curve.
    pub(super) fn box_lines(&self, lines: &mut Lines, fg: &str, x: usize, arms: [u8; 4]) {
        let (cw, ch, base) = (self.cw, self.ch, self.base);
        let (left, right) = (x as f64 * cw, (x + 1) as f64 * cw);
        let (top, bottom) = (-base, ch - base);
        let (cx, cy) = (left + cw / 2.0, top + ch / 2.0);
        if arms.contains(&4) {
            let r = cw / 2.0;
            let (hx, hy) = if arms[1] == 4 { (right, cx + r) } else { (left, cx - r) };
            let (vy, vend) = if arms[2] == 4 { (cy + r, bottom) } else { (cy - r, top) };
            let d = format!("M{} {}H{}Q{} {} {} {}V{}", g(hx), g(cy), g(hy), g(cx), g(cy), g(cx), g(vy), g(vend));
            lines.curves.push((fg.to_string(), "l", d));
            return;
        }
        let gap = (cw * 0.18).max(1.2);
        let class = |w: u8| match w {
            2 => "h",
            3 => "q",
            _ => "l",
        };
        let offsets = |w: u8| if w == 3 { vec![-gap, gap] } else { vec![0.0] };
        // horizontal: left arm [3], right arm [1]; vertical: up [0], down [2]
        for (a, b, horizontal) in [(arms[3], arms[1], true), (arms[0], arms[2], false)] {
            let spans: Vec<(u8, f64, f64)> = if a == b && a > 0 {
                vec![(a, 0.0, 1.0)]
            } else {
                [(a, 0.0, 0.5), (b, 0.5, 1.0)].into_iter().filter(|s| s.0 > 0).collect()
            };
            for (w, from, to) in spans {
                for o in offsets(w) {
                    if horizontal {
                        lines.h.push((fg.to_string(), class(w), cy + o, left + from * cw, left + to * cw));
                    } else {
                        lines.v.push((fg.to_string(), class(w), cx + o, top + from * ch, top + to * ch));
                    }
                }
            }
        }
    }
}

/// Box drawing segments of a row: horizontal ones merge across cells.
#[derive(Default)]
pub(super) struct Lines {
    /// (color, class, y, x1, x2)
    h: Vec<(String, &'static str, f64, f64, f64)>,
    /// (color, class, x, y1, y2)
    v: Vec<(String, &'static str, f64, f64, f64)>,
    curves: Vec<(String, &'static str, String)>,
}

impl Lines {
    /// One path per (color, class), touching horizontal segments joined.
    pub(super) fn paths(mut self) -> Vec<(String, &'static str, String)> {
        let mut out: Vec<(String, &'static str, String)> = Vec::new();
        self.h.sort_by(|a, b| {
            (&a.0, a.1, a.2, a.3).partial_cmp(&(&b.0, b.1, b.2, b.3)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut merged: Vec<(String, &'static str, f64, f64, f64)> = Vec::new();
        for s in self.h {
            match merged.last_mut() {
                Some(m) if m.0 == s.0 && m.1 == s.1 && m.2 == s.2 && s.3 <= m.4 + 0.01 => m.4 = m.4.max(s.4),
                _ => merged.push(s),
            }
        }
        for (c, k, y, x1, x2) in merged {
            push_line(&mut out, &c, k, format!("M{} {}H{}", g(x1), g(y), g(x2)));
        }
        for (c, k, x, y1, y2) in self.v {
            push_line(&mut out, &c, k, format!("M{} {}V{}", g(x), g(y1), g(y2)));
        }
        for (c, k, d) in self.curves {
            push_line(&mut out, &c, k, d);
        }
        out
    }
}

pub(super) fn blank(c: &Cell) -> bool {
    c.ch == " " || c.ch == "\u{2800}"
}

/// The non-blank stretches of a run as (offset, cells), trimmed; with
/// `split`, gaps of two or more blanks separate stretches.
pub(super) fn pieces(run: &[Cell], split: bool) -> Vec<(usize, &[Cell])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < run.len() {
        while i < run.len() && blank(&run[i]) {
            i += 1;
        }
        if i >= run.len() {
            break;
        }
        let start = i;
        let mut last = i; // last non-blank cell (or its spacer)
        while i < run.len() {
            if blank(&run[i]) {
                let gap = run[i..].iter().take_while(|c| blank(c)).count();
                if (split && gap >= 2) || i + gap == run.len() {
                    break;
                }
                i += gap;
                continue;
            }
            last = i;
            i += 1;
        }
        out.push((start, &run[start..=last]));
    }
    out
}

pub(super) fn push_path(v: &mut Vec<(String, String)>, color: String, d: String) {
    match v.iter_mut().find(|(c, _)| *c == color) {
        Some((_, p)) => p.push_str(&d),
        None => v.push((color, d)),
    }
}

pub(super) fn push_line(v: &mut Vec<(String, &'static str, String)>, color: &str, class: &'static str, d: String) {
    match v.iter_mut().find(|(c, k, _)| c == color && *k == class) {
        Some((_, _, p)) => p.push_str(&d),
        None => v.push((color.to_string(), class, d)),
    }
}
