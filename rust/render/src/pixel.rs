//! A screen as a PNG: drawn as a static SVG by the same renderer as the
//! animations, rasterized with resvg (no browser). The font comes from the
//! embedded JetBrains Mono, the system fonts cover what it lacks.
//!
//! The PNG also carries the screen's text in an `iTXt` chunk (keyword
//! `anina-screen`), so `anina check` can read what the pixels show. Writing
//! that chunk and reading a PNG's chunks back both live here.

use super::shape::render_screen;
use super::theme::Theme;
use anina_screen::Screen;
use anyhow::{Context, Result, anyhow};
use resvg::{tiny_skia, usvg};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(super::fonts::REGULAR.to_vec());
        db.load_font_data(super::fonts::BOLD.to_vec());
        db.load_system_fonts();
        db.set_monospace_family(super::fonts::SYSTEM_FAMILY);
        Arc::new(db)
    })
    .clone()
}

/// The `iTXt` keyword the screen's text is stored under.
pub const TEXT_KEY: &str = "anina-screen";

/// Rasterize an SVG document to a PNG at `scale`.
pub fn svg_to_png(svg: &str, path: &Path, scale: f32) -> Result<()> {
    svg_to_png_with_text(svg, path, scale, None)
}

/// Rasterize an SVG document to a PNG at `scale`, with `text` (what the
/// image shows) in an `iTXt` chunk.
pub fn svg_to_png_with_text(svg: &str, path: &Path, scale: f32, text: Option<&str>) -> Result<()> {
    let opt = usvg::Options { fontdb: fontdb(), font_family: super::fonts::SYSTEM_FAMILY.into(), ..Default::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| anyhow!("svg: {e}"))?;
    let size = tree.size().to_int_size().scale_by(scale).ok_or_else(|| anyhow!("empty image"))?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| anyhow!("image too large"))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    let mut png = pixmap.encode_png().map_err(|e| anyhow!("png: {e}"))?;
    if let Some(t) = text {
        png = with_itxt(&png, TEXT_KEY, t)?;
    }
    std::fs::write(path, png).with_context(|| format!("writing {}", path.display()))
}

/// `png` with an uncompressed `iTXt` chunk `key` = `text` before `IEND`.
fn with_itxt(png: &[u8], key: &str, text: &str) -> Result<Vec<u8>> {
    let iend =
        png.len().checked_sub(12).filter(|&i| &png[i + 4..i + 8] == b"IEND").ok_or_else(|| anyhow!("png: no IEND"))?;
    // keyword, NUL, compression flag 0, method 0, empty language, NUL, empty translated keyword, NUL, text
    let mut data = Vec::with_capacity(key.len() + text.len() + 5);
    data.extend_from_slice(key.as_bytes());
    data.extend_from_slice(&[0, 0, 0, 0, 0]);
    data.extend_from_slice(text.as_bytes());
    let mut chunk = Vec::with_capacity(data.len() + 12);
    chunk.extend_from_slice(&u32::try_from(data.len())?.to_be_bytes());
    chunk.extend_from_slice(b"iTXt");
    chunk.extend_from_slice(&data);
    chunk.extend_from_slice(&crc32fast::hash(&chunk[4..]).to_be_bytes());
    let mut out = png[..iend].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[iend..]);
    Ok(out)
}

pub const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A PNG's chunks as (kind, data), in order, up to IEND or the first one
/// that runs past the end. Empty when `png` is not a PNG.
pub fn png_chunks(png: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    if !png.starts_with(PNG_SIGNATURE) {
        return out;
    }
    let mut at = PNG_SIGNATURE.len();
    while at + 12 <= png.len() {
        let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
        let kind = &png[at + 4..at + 8];
        let Some(data) = png.get(at + 8..at + 8 + len) else { break };
        out.push((kind, data));
        if kind == b"IEND" {
            break;
        }
        at += 12 + len;
    }
    out
}

/// The text of every tEXt, zTXt and iTXt chunk of a PNG, joined by lines.
/// A chunk that does not parse is left out; the rest still counts.
pub fn png_text(png: &[u8]) -> String {
    let text: Vec<String> = png_chunks(png)
        .into_iter()
        .filter_map(|(kind, data)| match kind {
            b"tEXt" => data.splitn(2, |b| *b == 0).nth(1).map(latin1),
            b"zTXt" => {
                data.splitn(2, |b| *b == 0).nth(1).and_then(|r| r.get(1..)).and_then(inflate).map(|b| latin1(&b))
            }
            b"iTXt" => itxt(data),
            _ => None,
        })
        .collect();
    text.join("\n")
}

/// keyword NUL flag method language NUL translated NUL text
fn itxt(data: &[u8]) -> Option<String> {
    let mut parts = data.splitn(2, |b| *b == 0);
    parts.next()?;
    let rest = parts.next()?;
    let (flag, rest) = (*rest.first()?, rest.get(2..)?);
    let mut fields = rest.splitn(3, |b| *b == 0);
    let (_lang, _translated, text) = (fields.next()?, fields.next()?, fields.next()?);
    let text = if flag == 1 { inflate(text)? } else { text.to_vec() };
    String::from_utf8(text).ok()
}

fn inflate(z: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(z).read_to_end(&mut out).ok()?;
    Some(out)
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// Write `screen` as a PNG with a window titled `title`, in `theme`.
pub fn screen_png_with(screen: &Screen, title: &str, theme: &Theme, path: &Path) -> Result<()> {
    let svg = render_screen(screen, title, theme, false)?;
    svg_to_png_with_text(&svg, path, 2.0, Some(&format!("{title}\n{}", screen.text())))
}

/// Write `screen` as a PNG with a window titled `title`, in `theme` (a name
/// or path; empty or "auto" = colors derived from the screen itself).
pub fn screen_png(screen: &Screen, title: &str, theme: &str, path: &Path) -> Result<()> {
    let th = match theme {
        "" | "auto" => crate::brand::derive_screens(std::slice::from_ref(screen), title),
        t => Theme::load(t)?,
    };
    screen_png_with(screen, title, &th, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_zlib() {
        assert_eq!(crc32fast::hash(b"IEND"), 0xae42_6082);
        assert_eq!(crc32fast::hash(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn text_lands_before_iend() {
        let dir = std::env::temp_dir().join(format!("anina-pixel-{}", std::process::id()));
        let path = dir.join("t.png");
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#123"/></svg>"##;
        svg_to_png_with_text(svg, &path, 1.0, Some("title\nhello")).unwrap();
        let png = std::fs::read(&path).unwrap();
        let at = png.windows(4).position(|w| w == b"iTXt").unwrap();
        assert!(png[at..].windows(11).any(|w| w == b"title\nhello"));
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        // and it reads back, every chunk's CRC whole
        assert_eq!(png_text(&png), "title\nhello");
        let kinds: Vec<&[u8]> = png_chunks(&png).into_iter().map(|c| c.0).collect();
        assert_eq!(kinds.last(), Some(&&b"IEND"[..]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
