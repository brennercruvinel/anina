//! The HTML index of a run: every screenshot as a card, plus links to the
//! map, animations and reports.

use super::shape::esc;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct Item {
    pub name: String,
    pub png: PathBuf,
    pub caption: String,
}

/// `rel`, a file under `dir`, as an `<img src>` value, escaped: the path
/// itself, or with `embed` the file inlined as a data URI, so the page is one
/// file to share. A file that can't be read stays a path.
pub fn img_src(dir: &Path, rel: &str, embed: bool) -> String {
    use base64::Engine as _;
    if embed && let Ok(bytes) = std::fs::read(dir.join(rel)) {
        let mime = match rel.rsplit('.').next() {
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            _ => "application/octet-stream",
        };
        return format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    }
    esc(rel)
}

fn href(dir: &Path, p: &Path) -> String {
    match p.strip_prefix(dir) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => format!("file://{}", std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).display()),
    }
}

/// Write `dir/index.html`: a contact grid of `items` with `links` (label,
/// href) on top. Returns its path.
pub fn write_gallery(dir: &Path, title: &str, items: &[Item], links: &[(String, String)]) -> Result<PathBuf> {
    let nav: Vec<String> = links.iter().map(|(l, h)| format!(r#"<a href="{}">{}</a>"#, esc(h), esc(l))).collect();
    let cards: String = items
        .iter()
        .map(|it| {
            let src = esc(&href(dir, &it.png));
            format!(
                r#"<figure id="{id}"><a href="{src}"><img loading="lazy" src="{src}" alt="{name}"></a><figcaption><b>{name}</b>{cap}</figcaption></figure>"#,
                id = esc(&it.name),
                name = esc(&it.name),
                cap = if it.caption.is_empty() { String::new() } else { format!("<span>{}</span>", esc(&it.caption)) },
            )
        })
        .collect();
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{t}</title>
<style>
:root{{color-scheme:dark;--bg:#0d1117;--card:#161b22;--line:#30363d;--fg:#e6edf3;--muted:#8b949e;--accent:#7DE0D3}}
@media (prefers-color-scheme:light){{:root{{color-scheme:light;--bg:#f6f8fa;--card:#fff;--line:#d0d7de;--fg:#1f2328;--muted:#57606a;--accent:#0a7d6f}}}}
body{{margin:0;padding:24px max(16px,4vw);background:var(--bg);color:var(--fg);font:14px/1.5 ui-sans-serif,system-ui,sans-serif}}
h1{{margin:0 0 4px;font-size:22px}} nav{{display:flex;flex-wrap:wrap;gap:12px;margin:0 0 20px;color:var(--muted)}} a{{color:var(--accent)}}
main{{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,420px),1fr));gap:16px}}
figure{{margin:0;background:var(--card);border:1px solid var(--line);border-radius:10px;overflow:hidden}}
img{{display:block;width:100%;height:auto;background:#000}} figcaption{{padding:8px 12px;display:flex;flex-direction:column;gap:2px}} figcaption span{{color:var(--muted)}}
</style>
<h1>{t}</h1><nav>{n}<span>{count} screens</span></nav><main>{cards}</main>
"#,
        t = esc(title),
        n = nav.join(""),
        count = items.len(),
    );
    std::fs::create_dir_all(dir)?;
    let out = dir.join("index.html");
    std::fs::write(&out, html)?;
    Ok(out)
}
