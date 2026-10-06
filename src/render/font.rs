//! The embedded monospace font (JetBrains Mono, SIL OFL 1.1, see
//! assets/fonts/OFL.txt), and subsets of it for SVGs: only the glyphs a file
//! draws, as a web font in a data URI, so an animation looks the same on
//! every machine and inside an <img> on GitHub, where no outside font loads.
//!
//! `subsetter` cuts the glyphs but writes a PDF-only font (no `cmap`, no
//! `OS/2`); browsers refuse that, so the subset gets both back here: a format
//! 4 `cmap` for the kept characters and the original `OS/2`.

use anyhow::{Result, anyhow};
use base64::Engine as _;
use std::collections::BTreeSet;

pub const REGULAR: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
pub const BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");

/// The family name SVGs use for the embedded subset, ahead of the theme's.
pub const FAMILY: &str = "AninaMono";
/// The family name of the full font, as resvg finds it in its font database.
pub const SYSTEM_FAMILY: &str = "JetBrains Mono";

/// A web-font subset of `font` with the characters of `chars` it covers
/// (the rest falls back to the viewer's fonts). `None` when it covers none.
pub fn subset(font: &[u8], chars: &BTreeSet<char>) -> Result<Option<Vec<u8>>> {
    let face = ttf_parser::Face::parse(font, 0).map_err(|e| anyhow!("font: {e}"))?;
    let mut mapper = subsetter::GlyphRemapper::new();
    let mut map: Vec<(u32, u16)> = Vec::new();
    for &c in chars {
        if (c as u32) > 0xFFFF {
            continue; // format 4 cmap covers the BMP; the rest falls back
        }
        if let Some(g) = face.glyph_index(c) {
            map.push((c as u32, mapper.remap(g.0)));
        }
    }
    if map.is_empty() {
        return Ok(None);
    }
    let cut = subsetter::subset(font, 0, &mapper).map_err(|e| anyhow!("subsetting: {e:?}"))?;
    let mut tables = read_tables(&cut)?;
    tables.retain(|(t, _)| t != b"cmap" && t != b"OS/2");
    tables.push((*b"cmap", cmap4(&map)));
    let os2 = read_tables(font)?.into_iter().find(|(t, _)| t == b"OS/2").map(|(_, d)| d);
    if let Some(os2) = os2 {
        tables.push((*b"OS/2", os2));
    }
    Ok(Some(write_sfnt(tables)))
}

/// `@font-face` rules for the regular and bold subsets of `regular`/`bold`
/// characters, as base64 data URIs.
pub fn face_css(regular: &BTreeSet<char>, bold: &BTreeSet<char>) -> Result<String> {
    let mut css = String::new();
    for (font, chars, weight) in [(REGULAR, regular, 400), (BOLD, bold, 700)] {
        if let Some(data) = subset(font, chars)? {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            css.push_str(&format!(
                "@font-face{{font-family:{FAMILY};font-weight:{weight};src:url(data:font/ttf;base64,{b64}) format(\"truetype\")}}"
            ));
        }
    }
    Ok(css)
}

fn be16(d: &[u8], at: usize) -> Result<u16> {
    d.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or_else(|| anyhow!("font is truncated"))
}

fn be32(d: &[u8], at: usize) -> Result<u32> {
    d.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| anyhow!("font is truncated"))
}

/// The tables of an sfnt, by tag.
fn read_tables(d: &[u8]) -> Result<Vec<([u8; 4], Vec<u8>)>> {
    let n = be16(d, 4)? as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let rec = 12 + 16 * i;
        let tag: [u8; 4] = d.get(rec..rec + 4).ok_or_else(|| anyhow!("font is truncated"))?.try_into()?;
        let (off, len) = (be32(d, rec + 8)? as usize, be32(d, rec + 12)? as usize);
        out.push((tag, d.get(off..off + len).ok_or_else(|| anyhow!("font table out of range"))?.to_vec()));
    }
    Ok(out)
}

fn checksum(d: &[u8]) -> u32 {
    d.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// Assemble an sfnt (TrueType outlines) from tables, with checksums and
/// `head.checkSumAdjustment`.
fn write_sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|a| a.0);
    let n = tables.len() as u16;
    let sel = 15 - n.leading_zeros() as u16; // floor(log2 n)
    let range = 16u16 << sel;
    let mut out = Vec::new();
    out.extend_from_slice(&0x00010000u32.to_be_bytes());
    for v in [n, range, sel, n * 16 - range] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut head_at = None;
    for (tag, data) in &mut tables {
        if tag == b"head" && data.len() >= 12 {
            data[8..12].copy_from_slice(&[0; 4]);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = Some(offset);
        }
        offset += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    if let Some(h) = head_at {
        let adj = 0xB1B0AFBAu32.wrapping_sub(checksum(&out));
        out[h + 8..h + 12].copy_from_slice(&adj.to_be_bytes());
    }
    out
}

/// A `cmap` with one format 4 subtable (Windows, Unicode BMP) for `map`
/// (code point, glyph id), sorted by code point.
fn cmap4(map: &[(u32, u16)]) -> Vec<u8> {
    // segments of consecutive code points whose glyph ids are consecutive too
    let mut segs: Vec<(u16, u16, i32)> = Vec::new(); // start, end, delta
    for &(c, g) in map {
        let (c, delta) = (c as u16, g as i32 - c as i32);
        match segs.last_mut() {
            Some(s) if s.1 as u32 + 1 == c as u32 && s.2 == delta => s.1 = c,
            _ => segs.push((c, c, delta)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 1));
    let seg_x2 = (segs.len() * 2) as u16;
    let sel = 15 - (segs.len() as u16).leading_zeros() as u16;
    let range = 2u16 << sel;
    let mut sub = Vec::new();
    let len = 16 + 8 * segs.len();
    for v in [4u16, len as u16, 0, seg_x2, range, sel, seg_x2 - range] {
        sub.extend_from_slice(&v.to_be_bytes());
    }
    segs.iter().for_each(|s| sub.extend_from_slice(&s.1.to_be_bytes()));
    sub.extend_from_slice(&0u16.to_be_bytes());
    segs.iter().for_each(|s| sub.extend_from_slice(&s.0.to_be_bytes()));
    segs.iter().for_each(|s| sub.extend_from_slice(&(s.2 as i16 as u16).to_be_bytes()));
    segs.iter().for_each(|_| sub.extend_from_slice(&0u16.to_be_bytes()));
    let mut out = Vec::new();
    for v in [0u16, 1, 3, 1] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend_from_slice(&sub);
    out
}
