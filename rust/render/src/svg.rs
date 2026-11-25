//! A cast as one animated, self-contained SVG (and a screen as a static one,
//! which `png` rasterizes).
//!
//! The technique of a hand-made terminal demo, generalized: a window drawn in
//! SVG and SMIL `<animate>` on one shared, looping timeline, so it plays in a
//! browser, a README or an <img> with no script.
//! - every distinct terminal row is drawn once in <defs>; a frame is a list
//!   of <use> of its rows, shown only during its own intervals (discrete
//!   opacity), so a TUI recorded at 10 fps stays small;
//! - text runs are pinned to the cell grid with textLength; braille is drawn
//!   as dots, block elements as rectangles and box drawing as lines, the
//!   glyphs fonts get wrong or leave gaps between;
//! - the glyphs used are embedded as a font subset, so it looks the same
//!   everywhere;
//! - the theme owns the window, brand, captions, progress bar and the 16
//!   ANSI colors; nothing here knows the program.

use super::fonts;
use super::theme::Theme;
use anina_screen::frames::{Cast, Frame};
use anina_screen::notice::Notice;
use anina_screen::{Cell, Screen, attr};
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

mod cells;
use cells::*;

#[derive(Clone, Debug)]
pub struct Options {
    /// Longest a still screen stays (default: the theme's).
    pub max_hold: Option<f64>,
    /// Play everything X times faster.
    pub speed: f64,
    /// Playback frame rate cap (default: the theme's).
    pub max_fps: Option<f64>,
    /// Embed the font subset (off for PNG: resvg reads the font itself).
    pub embed_font: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { max_hold: None, speed: 1.0, max_fps: None, embed_font: true }
    }
}

/// What a render produced.
pub struct Rendered {
    pub svg: String,
    /// Playback length in seconds.
    pub total: f64,
    /// When the poster frame shows, on the playback clock.
    pub poster: f64,
    /// Distinct rows drawn.
    pub rows: usize,
}

// block elements drawn as rectangles: (x, y, w, h) as fractions of the cell
fn block(c: char) -> Option<(f64, f64, f64, f64)> {
    Some(match c {
        '█' => (0.0, 0.0, 1.0, 1.0),
        '▀' => (0.0, 0.0, 1.0, 0.5),
        '▄' => (0.0, 0.5, 1.0, 0.5),
        '▌' => (0.0, 0.0, 0.5, 1.0),
        '▐' => (0.5, 0.0, 0.5, 1.0),
        '▘' => (0.0, 0.0, 0.5, 0.5),
        '▝' => (0.5, 0.0, 0.5, 0.5),
        '▖' => (0.0, 0.5, 0.5, 0.5),
        '▗' => (0.5, 0.5, 0.5, 0.5),
        '▁' => (0.0, 7.0 / 8.0, 1.0, 1.0 / 8.0),
        '▂' => (0.0, 6.0 / 8.0, 1.0, 2.0 / 8.0),
        '▃' => (0.0, 5.0 / 8.0, 1.0, 3.0 / 8.0),
        '▅' => (0.0, 3.0 / 8.0, 1.0, 5.0 / 8.0),
        '▆' => (0.0, 2.0 / 8.0, 1.0, 6.0 / 8.0),
        '▇' => (0.0, 1.0 / 8.0, 1.0, 7.0 / 8.0),
        '▏' => (0.0, 0.0, 1.0 / 8.0, 1.0),
        '▎' => (0.0, 0.0, 2.0 / 8.0, 1.0),
        '▍' => (0.0, 0.0, 3.0 / 8.0, 1.0),
        '▋' => (0.0, 0.0, 5.0 / 8.0, 1.0),
        '▊' => (0.0, 0.0, 6.0 / 8.0, 1.0),
        '▉' => (0.0, 0.0, 7.0 / 8.0, 1.0),
        '▔' => (0.0, 0.0, 1.0, 1.0 / 8.0),
        '▕' => (7.0 / 8.0, 0.0, 1.0 / 8.0, 1.0),
        _ => return None,
    })
}

/// Box drawing as arms (up, right, down, left): 1 light, 2 heavy, 3 double;
/// 4 marks a rounded corner.
fn box_arms(c: char) -> Option<[u8; 4]> {
    let (l, h, d) = (1u8, 2u8, 3u8);
    Some(match c {
        '─' => [0, l, 0, l],
        '│' => [l, 0, l, 0],
        '┌' => [0, l, l, 0],
        '┐' => [0, 0, l, l],
        '└' => [l, l, 0, 0],
        '┘' => [l, 0, 0, l],
        '├' => [l, l, l, 0],
        '┤' => [l, 0, l, l],
        '┬' => [0, l, l, l],
        '┴' => [l, l, 0, l],
        '┼' => [l, l, l, l],
        '╴' => [0, 0, 0, l],
        '╵' => [l, 0, 0, 0],
        '╶' => [0, l, 0, 0],
        '╷' => [0, 0, l, 0],
        '━' => [0, h, 0, h],
        '┃' => [h, 0, h, 0],
        '┏' => [0, h, h, 0],
        '┓' => [0, 0, h, h],
        '┗' => [h, h, 0, 0],
        '┛' => [h, 0, 0, h],
        '┣' => [h, h, h, 0],
        '┫' => [h, 0, h, h],
        '┳' => [0, h, h, h],
        '┻' => [h, h, 0, h],
        '╋' => [h, h, h, h],
        '═' => [0, d, 0, d],
        '║' => [d, 0, d, 0],
        '╔' => [0, d, d, 0],
        '╗' => [0, 0, d, d],
        '╚' => [d, d, 0, 0],
        '╝' => [d, 0, 0, d],
        '╠' => [d, d, d, 0],
        '╣' => [d, 0, d, d],
        '╦' => [0, d, d, d],
        '╩' => [d, d, 0, d],
        '╬' => [d, d, d, d],
        '╭' => [0, 4, 4, 0],
        '╮' => [0, 0, 4, 4],
        '╯' => [4, 0, 0, 4],
        '╰' => [4, 4, 0, 0],
        _ => return None,
    })
}

fn is_braille(c: char) -> bool {
    ('\u{2801}'..='\u{28FF}').contains(&c)
}

/// bit i of (codepoint - 0x2800) is the dot at (column, row)
const BRAILLE_BITS: [(f64, f64); 8] =
    [(0.0, 0.0), (0.0, 1.0), (0.0, 2.0), (1.0, 0.0), (1.0, 1.0), (1.0, 2.0), (0.0, 3.0), (1.0, 3.0)];

/// A number the short way: at most 2 decimals, no trailing zeros.
pub(crate) fn g(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c if (c as u32) < 0x20 && c != '\t' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// The single (glyph-level) character of a cell, if it is one char.
fn single(c: &Cell) -> Option<char> {
    let mut it = c.ch.chars();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

/// (start, end, frame index) on the playback clock: each frame's real
/// duration over its speed, capped at `max_hold`; a frame shorter than
/// 1/max_fps gives its slot to the next one (the newer screen wins).
pub fn timeline(cast: &Cast, max_hold: f64, speed: f64, max_fps: f64) -> (Vec<(f64, f64, usize)>, f64) {
    let frames = &cast.frames;
    let min_dt = if max_fps > 0.0 { 1.0 / max_fps } else { 0.0 };
    let mut out: Vec<(f64, f64, usize)> = Vec::new();
    let mut t = 0.0;
    for (i, f) in frames.iter().enumerate() {
        let last = i + 1 == frames.len();
        let raw = if last { cast.end_hold.max(0.0) } else { frames[i + 1].t - f.t };
        let mut d = raw / if f.speed > 0.0 { f.speed } else { 1.0 } / speed;
        if !last {
            d = d.min(max_hold);
        }
        if d <= 0.001 {
            continue;
        }
        match out.last_mut() {
            Some(prev) if prev.1 - prev.0 < min_dt => *prev = (prev.0, t + d, i),
            _ => out.push((t, t + d, i)),
        }
        t += d;
    }
    (out, t)
}

/// A discrete opacity animation visible during `intervals`, and the static
/// opacity: 1 when visible at `poster`, the still a viewer without SMIL shows.
fn animate(intervals: &[(f64, f64)], total: f64, poster: f64) -> (String, &'static str) {
    let mut ivs = intervals.to_vec();
    ivs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (a, b) in ivs {
        match merged.last_mut() {
            Some(m) if a <= m.1 + 1e-6 => m.1 = m.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut times = vec![0.0];
    let mut values = vec![if merged.first().is_some_and(|m| m.0 <= 1e-6) { "1" } else { "0" }];
    for &(a, b) in &merged {
        for (tt, v) in [(a, "1"), (b, "0")] {
            let k = tt / total;
            if k <= 1e-6 {
                values[0] = v;
                continue;
            }
            if k >= 1.0 - 1e-6 {
                continue;
            }
            if (k - times[times.len() - 1]).abs() < 1e-6 {
                *values.last_mut().unwrap() = v;
            } else {
                times.push(k);
                values.push(v);
            }
        }
    }
    let anim = format!(
        r#"<animate attributeName="opacity" calcMode="discrete" values="{}" keyTimes="{}" dur="{:.2}s" repeatCount="indefinite"/>"#,
        values.join(";"),
        times.iter().map(|k| format!("{k:.5}")).collect::<Vec<_>>().join(";"),
        total
    );
    let still = if merged.iter().any(|&(a, b)| a - 1e-6 <= poster && poster < b) { "1" } else { "0" };
    (anim, still)
}

fn stops(cols: &[String]) -> String {
    let n = (cols.len().max(2) - 1) as f64;
    cols.iter().enumerate().map(|(i, c)| format!(r#"<stop offset="{:.2}" stop-color="{c}"/>"#, i as f64 / n)).collect()
}

/// The pieces of the document both renders share.
struct Frame2<'a> {
    title: &'a str,
    desc: String,
    cols: u16,
    rows: u16,
    /// With a caption bar and progress bar (animations) or without (stills).
    chrome: bool,
    /// The terminal content: frame groups or one screen.
    body: String,
    /// The caption texts (animations).
    captions: String,
    /// Loop length in seconds (animations).
    total: f64,
    embed_font: bool,
    /// Characters drawn outside the rows (captions), for the font subset.
    extra_chars: String,
}

fn document(r: &Renderer, th: &Theme, f: &Frame2) -> Result<String> {
    let (body, captions, total, opts_embed, extra_chars) =
        (&f.body, &f.captions, f.total, f.embed_font, f.extra_chars.as_str());
    let (win, brand, cap, prog, term) = (&th.window, &th.brand, &th.caption, &th.progress, &th.terminal);
    let (cw, ch) = (r.cw, r.ch);
    let (pad, bar_h) = (win.pad, win.bar_h);
    let (tw, tht) = (f.cols as f64 * cw, f.rows as f64 * ch);
    let w = (tw + 2.0 * pad).round();
    let (ox, oy) = (pad, bar_h + pad * 0.6);
    let cap_y = oy + tht + pad * 0.6;
    let h =
        if f.chrome { (cap_y + cap.height + prog.height + pad * 0.5).round() } else { (oy + tht + pad * 0.7).round() };
    let lights = if win.lights {
        ["#FF5F57", "#FEBC2E", "#28C840"]
            .iter()
            .enumerate()
            .map(|(i, c)| format!(r#"<circle cx="{}" cy="{}" r="5.5" fill="{c}"/>"#, 22 + 18 * i, g(bar_h / 2.0)))
            .collect::<String>()
    } else {
        String::new()
    };
    let brand_svg = if brand.text.is_empty() {
        String::new()
    } else {
        format!(
            r#"<text x="{}" y="{}" text-anchor="end" font-weight="800" font-size="{}px" fill="url(#brand)">{}</text>"#,
            g(w - pad),
            g(bar_h / 2.0 + 5.0),
            g(brand.font_size),
            esc(&brand.text)
        )
    };
    let mut family = format!("'{}',{}", fonts::SYSTEM_FAMILY, term.font_family);
    let mut faces = String::new();
    if opts_embed {
        let mut regular = r.regular.clone();
        let mut bold = r.bold.clone();
        regular.extend(f.title.chars().filter(|c| !c.is_whitespace()));
        regular.extend(extra_chars.chars().filter(|c| !c.is_whitespace()));
        bold.extend(brand.text.chars());
        bold.extend(extra_chars.chars().filter(|c| !c.is_whitespace()));
        faces = fonts::face_css(&regular, &bold)?;
        family = format!("{},{family}", fonts::FAMILY);
    }
    let brand_grad = if brand.gradient.is_empty() { vec![win.title_color.clone()] } else { brand.gradient.clone() };
    let border = if term.border.is_empty() { &term.bg } else { &term.border };
    let mut svg = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xml:space="preserve" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-labelledby="title desc">
<title id="title">{title}</title>
<desc id="desc">{desc}</desc>
<defs>
<linearGradient id="bg" x1="0" y1="0" x2="{w}" y2="{h}" gradientUnits="userSpaceOnUse">{bgs}</linearGradient>
<linearGradient id="bar" x1="0" y1="0" x2="0" y2="{bar_h}" gradientUnits="userSpaceOnUse">{bars}</linearGradient>
<linearGradient id="brand" x1="0" y1="0" x2="1" y2="0">{brands}</linearGradient>
<clipPath id="term"><rect x="0" y="0" width="{tw}" height="{tht}"/></clipPath>
{defs}
</defs>
<style>{faces}text{{font-family:{family};font-size:{fs}px;white-space:pre}}.d{{stroke-width:1.9;stroke-linecap:round}}.l,.h,.q{{fill:none;stroke-linecap:square}}.l{{stroke-width:1.15}}.h{{stroke-width:2.3}}.q{{stroke-width:.9}}{css}</style>
<rect width="{w}" height="{h}" rx="{radius}" fill="url(#bg)"/>
<rect width="{w}" height="{bar_h}" rx="{radius}" fill="url(#bar)"/>
<rect y="{bar_rad}" width="{w}" height="{radius}" fill="url(#bar)"/>
{lights}
<text x="{tx}" y="{ty}" text-anchor="middle" fill="{title_color}" font-size="12px">{title}</text>
{brand_svg}
<rect x="{rx}" y="{ry}" width="{rw}" height="{rh}" rx="{trad}" fill="{tbg}" stroke="{border}"/>
<g transform="translate({oxs} {oys})" clip-path="url(#term)">
{body}
</g>
"##,
        title = esc(f.title),
        desc = esc(&f.desc),
        bgs = stops(&win.bg),
        bars = stops(&win.bar),
        brands = stops(&brand_grad),
        tw = g(tw),
        tht = g(tht),
        defs = r.row_defs.join("\n"),
        fs = g(term.font_size),
        css = r.css(),
        radius = g(win.radius),
        bar_h = g(bar_h),
        bar_rad = g(bar_h - win.radius),
        tx = g(w / 2.0),
        ty = g(bar_h / 2.0 + 4.0),
        title_color = win.title_color,
        rx = g(ox - 6.0),
        ry = g(oy - 6.0),
        rw = g(tw + 12.0),
        rh = g(tht + 12.0),
        trad = g(term.radius),
        tbg = term.bg,
        oxs = g(ox),
        oys = g(oy),
    );
    if f.chrome {
        let py = cap_y + cap.height + 6.0;
        svg.push_str(&format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="8" fill="{}"/>
{captions}
<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}"/>
<rect x="{}" y="{}" width="0" height="{}" rx="{}" fill="{}"><animate attributeName="width" values="0;{}" dur="{:.2}s" repeatCount="indefinite"/></rect>
"#,
            g(ox - 6.0),
            g(cap_y),
            g(tw + 12.0),
            g(cap.height),
            cap.bg,
            g(ox - 6.0),
            g(py),
            g(tw + 12.0),
            g(prog.height),
            g(prog.height / 2.0),
            prog.track,
            g(ox - 6.0),
            g(py),
            g(prog.height),
            g(prog.height / 2.0),
            prog.fill,
            g(tw + 12.0),
            total
        ));
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

/// A frame group: a screen with its cursor cell.
type GroupKey = (usize, Option<[u16; 2]>);

/// The cast as an animated SVG.
pub fn render_cast(cast: &Cast, th: &Theme, opts: &Options) -> Result<Rendered> {
    let mut r = Renderer::new(th, cast.cols);
    let (cw, ch) = (r.cw, r.ch);
    let max_hold = opts.max_hold.unwrap_or(th.max_hold);
    let max_fps = opts.max_fps.unwrap_or(th.max_fps);
    let (tl, total) = timeline(cast, max_hold, opts.speed.max(0.01), max_fps);
    let total = total.max(0.01);
    // the poster: the first slot showing the marked frame or a later one
    let poster = tl.iter().find(|(_, _, i)| *i >= cast.poster).map_or(0.0, |s| s.0);

    // screens: one group per (screen, cursor), shown during its intervals
    let mut groups: Vec<(GroupKey, Vec<(f64, f64)>)> = Vec::new();
    let mut gidx: HashMap<GroupKey, usize> = HashMap::new();
    let mut caps: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
    for &(a, b, i) in &tl {
        let f: &Frame = &cast.frames[i];
        let key = (f.s, f.cursor);
        let n = *gidx.entry(key).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[n].1.push((a, b));
        match caps.iter_mut().find(|(c, _)| *c == f.caption) {
            Some((_, v)) => v.push((a, b)),
            None => caps.push((f.caption.clone(), vec![(a, b)])),
        }
    }
    let mut parsed: HashMap<usize, Screen> = HashMap::new();
    let mut body = String::new();
    for ((sid, cursor), ivs) in &groups {
        let s = parsed.entry(*sid).or_insert_with(|| cast.screen(*sid));
        let mut uses = String::new();
        for y in 0..s.rows.min(cast.rows) {
            if let Some(rid) = r.row(s.row(y)) {
                uses.push_str(&format!(r##"<use xlink:href="#{rid}" y="{}"/>"##, g(y as f64 * ch + r.base)));
            }
        }
        if let Some([cx, cy]) = cursor {
            uses.push_str(&format!(
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{}" fill="{}" opacity="0.85"/>"#,
                *cx as f64 * cw,
                *cy as f64 * ch,
                cw,
                g(ch),
                th.terminal.cursor
            ));
        }
        let (anim, still) = animate(ivs, total, poster);
        body.push_str(&format!(r#"<g opacity="{still}">{uses}{anim}</g>"#));
        body.push('\n');
    }

    let cap = &th.caption;
    let (pad, bar_h) = (th.window.pad, th.window.bar_h);
    let oy = bar_h + pad * 0.6;
    let cap_y = oy + cast.rows as f64 * ch + pad * 0.6;
    let mut cap_svg = String::new();
    let mut cap_chars = String::new();
    for (text, ivs) in &caps {
        let text = if text.is_empty() { th.brand.tagline.clone() } else { text.clone() };
        if text.is_empty() {
            continue;
        }
        cap_chars.push_str(&text);
        let (anim, still) = animate(ivs, total, poster);
        let inner = match text.split_once(" · ") {
            Some((head, rest)) => format!(
                r#"<tspan fill="{}" font-weight="700">{}</tspan><tspan fill="{}"> · </tspan><tspan>{}</tspan>"#,
                cap.accent,
                esc(head),
                cap.muted,
                esc(rest)
            ),
            None => esc(&text),
        };
        cap_svg.push_str(&format!(
            r#"<text x="{}" y="{}" fill="{}" font-size="{}px" opacity="{still}">{inner}{anim}</text>"#,
            g(pad + 4.0),
            g(cap_y + cap.height * 0.62),
            cap.fg,
            g(cap.font_size)
        ));
        cap_svg.push('\n');
    }
    let named: Vec<&str> = caps.iter().map(|(c, _)| c.as_str()).filter(|c| !c.is_empty()).collect();
    let mut desc = if th.desc.is_empty() {
        format!("{}: a recorded terminal session, {:.0} s, looping.", cast.title, total)
    } else {
        th.desc.clone()
    };
    if !named.is_empty() {
        desc.push_str(" Steps: ");
        desc.push_str(&named.join(" / "));
    }
    let f = Frame2 {
        title: &cast.title,
        desc,
        cols: cast.cols,
        rows: cast.rows,
        chrome: true,
        body,
        captions: cap_svg,
        total,
        embed_font: opts.embed_font,
        extra_chars: cap_chars,
    };
    let svg = document(&r, th, &f)?;
    Ok(Rendered { svg, total, poster, rows: r.row_defs.len() })
}

/// One screen as a static SVG (a screenshot): window and terminal only,
/// trailing blank rows trimmed.
pub fn render_screen(screen: &Screen, title: &str, th: &Theme, embed_font: bool) -> Result<String> {
    let mut r = Renderer::new(th, screen.cols);
    let rows = screen.used_rows().max(1);
    let mut body = String::new();
    for y in 0..rows {
        if let Some(rid) = r.row(screen.row(y)) {
            body.push_str(&format!(r##"<use xlink:href="#{rid}" y="{}"/>"##, g(y as f64 * r.ch + r.base)));
        }
    }
    if screen.cursor.visible && screen.cursor.y < rows {
        body.push_str(&format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{}" fill="{}" opacity="0.85"/>"#,
            screen.cursor.x as f64 * r.cw,
            screen.cursor.y as f64 * r.ch,
            r.cw,
            g(r.ch),
            th.terminal.cursor
        ));
    }
    let f = Frame2 {
        title,
        desc: screen.text(),
        cols: screen.cols,
        rows,
        chrome: false,
        body,
        captions: String::new(),
        total: 0.0,
        embed_font,
        extra_chars: String::new(),
    };
    document(&r, th, &f)
}

/// A plain-text transcript: each caption with the screen it led to, for
/// readers that cannot see the animation (screen readers, search, review).
pub fn transcript(cast: &Cast) -> String {
    let mut out = format!("{}\n{}\n", cast.title, "=".repeat(cast.title.chars().count().max(3)));
    let mut steps: Vec<(String, usize)> = Vec::new(); // caption, last frame
    for (i, f) in cast.frames.iter().enumerate() {
        match steps.last_mut() {
            Some((c, last)) if *c == f.caption => *last = i,
            _ => steps.push((f.caption.clone(), i)),
        }
    }
    if steps.iter().all(|(c, _)| c.is_empty()) && !cast.frames.is_empty() {
        steps = vec![(String::new(), cast.poster.min(cast.frames.len() - 1)), (String::new(), cast.frames.len() - 1)];
        steps.dedup_by_key(|s| s.1);
    }
    for (caption, last) in steps {
        let f = &cast.frames[last];
        out.push('\n');
        if !caption.is_empty() {
            out.push_str(&format!("## {caption}\n\n"));
        }
        out.push_str(&format!("[{:.1}s]\n", f.t));
        for line in cast.screen(f.s).text().lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Render `cast` to an animated SVG at `path` (and its transcript next to
/// it, as .txt). `theme` overrides the cast's own; none at all derives one
/// from the recording's colors. A theme of the cast's that cannot be found is
/// a warning to `notice`.
pub fn write_cast_svg(cast: &Cast, theme: Option<&str>, path: &Path, notice: &dyn Notice) -> Result<()> {
    let th = Theme::resolve(theme, cast, notice)?;
    let r = render_cast(cast, &th, &Options::default())?;
    write(path, &r.svg)?;
    write(&path.with_extension("txt"), &transcript(cast))
}

pub(crate) fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}
