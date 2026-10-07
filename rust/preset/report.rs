//! What a preset run leaves behind: `run.json` and an `index.html` with the
//! demo and the screenshots of each part, grouped by part.

use super::manifest::Part;
use crate::record::Recording;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct Run {
    pub preset: String,
    pub description: String,
    pub homepage: String,
    pub recorded: String,
    pub seconds: f64,
    pub engine: String,
    pub sandbox: String,
    pub parts: Vec<PartResult>,
    pub changes: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartResult {
    pub name: String,
    pub title: String,
    /// ok | failed | skipped
    pub status: String,
    pub error: Option<String>,
    pub seconds: f64,
    pub frames: usize,
    pub screens: usize,
    pub shots: Vec<String>,
    pub svg: Option<String>,
    pub cast: Option<String>,
    pub exit_code: Option<i32>,
    pub first_screen: Option<f64>,
    pub warnings: Vec<String>,
}

impl PartResult {
    fn base(p: &Part, title: &str, status: &str) -> Self {
        PartResult {
            name: p.name.clone(),
            title: title.into(),
            status: status.into(),
            error: None,
            seconds: 0.0,
            frames: 0,
            screens: 0,
            shots: Vec::new(),
            svg: None,
            cast: None,
            exit_code: None,
            first_screen: None,
            warnings: Vec::new(),
        }
    }

    pub fn ok(p: &Part, title: &str, r: &Recording, secs: f64, svg: bool) -> Self {
        PartResult {
            seconds: round(secs),
            frames: r.cast.frames.len(),
            screens: r.cast.screens.len(),
            shots: r.shots.iter().map(|s| format!("{}/shots/{}", p.name, s.name)).collect(),
            svg: svg.then(|| format!("{0}/{0}.svg", p.name)),
            cast: Some(format!("{0}/{0}.cast.json.gz", p.name)),
            exit_code: r.exit_code,
            first_screen: r.first_screen.map(round),
            warnings: r.warnings.clone(),
            ..Self::base(p, title, "ok")
        }
    }

    pub fn failed(p: &Part, title: &str, error: String, secs: f64) -> Self {
        PartResult { error: Some(error), seconds: round(secs), ..Self::base(p, title, "failed") }
    }

    pub fn skipped(p: &Part, title: &str, why: String) -> Self {
        PartResult { error: Some(why), ..Self::base(p, title, "skipped") }
    }
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Redacted and HTML-escaped.
fn esc(s: &str) -> String {
    crate::privacy::Redactor::shared()
        .text(s)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `index.html` next to `run.json`: per part, the demo and the screenshots
/// (PNG when rendered, the text otherwise).
pub fn write_index(out: &Path, run: &Run) -> anyhow::Result<()> {
    let mut body = String::new();
    for p in &run.parts {
        body.push_str(&format!(
            "<section><h2>{} <small>{} · {}{}</small></h2>",
            esc(&p.title),
            esc(&p.name),
            esc(&p.status),
            if p.seconds > 0.0 { format!(" · {:.1}s", p.seconds) } else { String::new() }
        ));
        if let Some(e) = &p.error {
            body.push_str(&format!("<pre class=err>{}</pre>", esc(e)));
        }
        if let Some(svg) = &p.svg {
            body.push_str(&format!("<p><img class=demo src=\"{}\" alt=\"{} demo\"></p>", esc(svg), esc(&p.title)));
        }
        if !p.shots.is_empty() {
            body.push_str("<div class=grid>");
            for s in &p.shots {
                let name = s.rsplit('/').next().unwrap_or(s);
                let fig = if out.join(format!("{s}.png")).is_file() {
                    format!("<img src=\"{}.png\" alt=\"{}\">", esc(s), esc(name))
                } else {
                    let text = std::fs::read_to_string(out.join(format!("{s}.txt"))).unwrap_or_default();
                    format!("<pre>{}</pre>", esc(&text))
                };
                body.push_str(&format!("<figure>{fig}<figcaption>{}</figcaption></figure>", esc(name)));
            }
            body.push_str("</div>");
        }
        body.push_str("</section>");
    }
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>{name} · anina</title>\
<style>body{{font:14px system-ui,sans-serif;margin:2rem auto;max-width:1200px;padding:0 1rem;color:#1d2731;background:#f6f7f8}}\
h1{{margin-bottom:.2rem}}h2 small{{font-weight:400;color:#68737d;font-size:.8em}}\
.demo{{max-width:100%;border-radius:10px}}.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(340px,1fr));gap:1rem}}\
figure{{margin:0}}figure img{{width:100%;border-radius:6px}}figcaption{{color:#68737d;font-size:12px}}\
pre{{background:#0d1b24;color:#e6eef0;padding:.6rem;border-radius:6px;overflow:auto;font-size:11px}}pre.err{{background:#3a1416}}\
footer{{margin-top:2rem;color:#68737d;font-size:12px}}</style>\
<h1>{name}</h1><p>{desc}{home}</p>{body}\
<footer>recorded {rec} · {secs:.0}s · engine {eng} · sandbox {sbx}<br>disk: {chg}</footer>",
        name = esc(&run.preset),
        desc = esc(&run.description),
        home = if run.homepage.is_empty() {
            String::new()
        } else {
            format!(" · <a href=\"{0}\">{0}</a>", esc(&run.homepage))
        },
        rec = esc(&run.recorded),
        secs = run.seconds,
        eng = esc(&run.engine),
        sbx = esc(&run.sandbox),
        chg = esc(&run.changes).replace('\n', "<br>"),
    );
    std::fs::write(out.join("index.html"), html)?;
    Ok(())
}
