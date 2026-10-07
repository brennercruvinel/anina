//! A screen as a PNG: drawn as a static SVG by the same renderer as the
//! animations, rasterized with resvg (no browser). The font comes from the
//! embedded JetBrains Mono, the system fonts cover what it lacks.

use super::shape::render_screen;
use super::theme::Theme;
use anina_screen::Screen;
use anyhow::{Context, Result, anyhow};
use resvg::{tiny_skia, usvg};
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

/// Rasterize an SVG document to a PNG at `scale`.
pub fn svg_to_png(svg: &str, path: &Path, scale: f32) -> Result<()> {
    let opt = usvg::Options { fontdb: fontdb(), font_family: super::fonts::SYSTEM_FAMILY.into(), ..Default::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| anyhow!("svg: {e}"))?;
    let size = tree.size().to_int_size().scale_by(scale).ok_or_else(|| anyhow!("empty image"))?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| anyhow!("image too large"))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    pixmap.save_png(path).with_context(|| format!("writing {}", path.display()))
}

/// Write `screen` as a PNG with a window titled `title`, in `theme`.
pub fn screen_png_with(screen: &Screen, title: &str, theme: &Theme, path: &Path) -> Result<()> {
    let svg = render_screen(screen, title, theme, false)?;
    svg_to_png(&svg, path, 2.0)
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
