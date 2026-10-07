//! The audit's outputs: report.json (every number), report.html (scores,
//! explanations, a contact sheet of every terminal), a terminal summary.

use super::contrast;
use super::measure::{Outcome, Status};
use super::score::{Score, status_line};
use crate::screen::{Screen, attr};
use anyhow::Result;
use serde::Serialize;
use std::fmt::Write as _;
use std::path::Path;

#[derive(Serialize)]
pub struct Report<'a> {
    pub tool: &'static str,
    pub version: &'static str,
    pub generated: String,
    pub target: String,
    pub engine: String,
    pub scores: &'a [Score],
    pub variants: &'a [Outcome],
}

pub fn write_json(r: &Report, path: &Path) -> Result<()> {
    std::fs::write(path, crate::privacy::Redactor::shared().to_json(r)?)?;
    Ok(())
}

/// Redacted (a failure names the tape's path, an error the user's files)
/// and HTML-escaped.
fn esc(s: &str) -> String {
    crate::privacy::Redactor::shared()
        .text(s)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn grade(score: Option<u8>) -> &'static str {
    match score {
        Some(s) if s >= 90 => "good",
        Some(s) if s >= 50 => "fair",
        Some(_) => "poor",
        None => "na",
    }
}

/// A screen as colored HTML, for when there is no PNG.
pub fn screen_html(s: &Screen) -> String {
    let mut out = String::new();
    for y in 0..s.used_rows().max(1) {
        let mut x = 0;
        let row = s.row(y);
        while x < row.len() {
            let c = &row[x];
            if c.is_spacer() {
                x += 1;
                continue;
            }
            let (fg, bg) = contrast::seen(c);
            let mut text = String::new();
            let style_of = |cx: &crate::screen::Cell| {
                (contrast::seen(cx), cx.attrs & (attr::BOLD | attr::ITALIC | attr::UNDERLINE))
            };
            let style = style_of(c);
            while x < row.len() && (row[x].is_spacer() || style_of(&row[x]) == style) {
                if !row[x].is_spacer() {
                    text.push_str(&row[x].ch);
                }
                x += 1;
            }
            let mut css = format!("color:{}", contrast::hex(fg));
            if bg != contrast::DEFAULT_BG {
                let _ = write!(css, ";background:{}", contrast::hex(bg));
            }
            if c.has(attr::BOLD) {
                css.push_str(";font-weight:700");
            }
            if c.has(attr::ITALIC) {
                css.push_str(";font-style:italic");
            }
            if c.has(attr::UNDERLINE) {
                css.push_str(";text-decoration:underline");
            }
            let _ = write!(out, "<span style=\"{css}\">{}</span>", esc(&text));
        }
        out.push('\n');
    }
    out
}

pub fn write_html(r: &Report, path: &Path) -> Result<()> {
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>anina audit · {target}</title>
<style>
:root{{--bg:#0f1418;--panel:#161d22;--line:#26323a;--ink:#dfe7ea;--dim:#8a9aa2;--good:#4cc38a;--fair:#e6a23c;--poor:#e5534b;--na:#6b7b83}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--ink);font:14px/1.5 ui-sans-serif,system-ui,-apple-system,Segoe UI,sans-serif}}
main{{max-width:1180px;margin:0 auto;padding:28px 20px 60px}}
h1{{font-size:22px;margin:0 0 4px}}h2{{font-size:16px;margin:36px 0 12px;color:var(--dim);font-weight:600;text-transform:uppercase;letter-spacing:.06em}}
.meta{{color:var(--dim);font-size:13px}}code,pre{{font-family:ui-monospace,'JetBrains Mono',Menlo,Consolas,monospace}}
.scores{{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:14px;margin-top:22px}}
.score{{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:16px}}
.gauge{{width:84px;height:84px;border-radius:50%;display:grid;place-items:center;font-size:26px;font-weight:700;margin-bottom:10px;
  background:conic-gradient(var(--c) calc(var(--v)*1%),var(--line) 0)}}
.gauge span{{width:68px;height:68px;border-radius:50%;background:var(--panel);display:grid;place-items:center;color:var(--c)}}
.good{{--c:var(--good)}}.fair{{--c:var(--fair)}}.poor{{--c:var(--poor)}}.na{{--c:var(--na)}}
.score h3{{margin:0 0 6px;font-size:15px;text-transform:capitalize}}.score ul{{margin:6px 0 0;padding-left:18px;color:var(--ink);font-size:13px;overflow-wrap:anywhere}}
.rule{{color:var(--dim);font-size:12px}}
table{{width:100%;border-collapse:collapse;font-size:13px}}th,td{{text-align:left;padding:7px 8px;border-bottom:1px solid var(--line);vertical-align:top}}
th{{color:var(--dim);font-weight:600}}.chip{{display:inline-block;padding:1px 8px;border-radius:99px;font-size:12px;font-weight:600;background:var(--line)}}
.chip.ok{{color:var(--good)}}.chip.broke{{color:var(--fair)}}.chip.crashed{{color:var(--poor)}}
.sheet{{display:grid;grid-template-columns:repeat(auto-fill,minmax(360px,1fr));gap:16px}}
.card{{background:var(--panel);border:1px solid var(--line);border-radius:12px;overflow:hidden}}
.card header{{padding:10px 12px;display:flex;justify-content:space-between;gap:8px;align-items:center;border-bottom:1px solid var(--line)}}
.card img{{display:block;width:100%}}.term{{margin:0;padding:10px;background:#000;font-size:9px;line-height:1.25;overflow:auto;max-height:420px;white-space:pre}}
.card footer{{padding:8px 12px;color:var(--dim);font-size:12px}}
</style><main>
<h1>anina audit</h1><div class="meta"><code>{target}</code> · {n} terminals · engine {engine} · {gen} · anina {ver}</div>
<div class="scores">"#,
        target = esc(&r.target),
        n = r.variants.len(),
        engine = esc(&r.engine),
        gen = esc(&r.generated),
        ver = r.version
    );
    for s in r.scores {
        let g = grade(s.score);
        let v = s.score.map_or("–".to_string(), |v| v.to_string());
        let _ = write!(
            h,
            r#"<section class="score {g}"><div class="gauge" style="--v:{pct}"><span>{v}</span></div><h3>{cat}</h3><div class="rule">{rule}</div><ul>{items}</ul></section>"#,
            pct = s.score.unwrap_or(0),
            cat = s.category,
            rule = esc(s.rule),
            items = s.explain.iter().map(|e| format!("<li>{}</li>", esc(e))).collect::<String>()
        );
    }
    h.push_str("</div><h2>Terminals</h2><table><tr><th>terminal</th><th>status</th><th>first screen</th><th>latency p50 / p95</th><th>text below AA</th><th>colors</th><th>exit</th><th>notes</th></tr>");
    for o in r.variants {
        let colors = match o.color_respected() {
            None => format!("{} truecolor cells", o.truecolor_cells),
            Some((ok, d)) => format!("{} {}", if ok { "ok:" } else { "✗" }, d),
        };
        let mut notes = Vec::new();
        if !matches!(o.status, Status::Ok) {
            notes.push(status_line(o));
        }
        if o.says_too_small {
            notes.push("says too small".to_string());
        }
        if let Some(c) = &o.crash_text {
            notes.push(c.clone());
        }
        notes.extend(o.error_lines.iter().take(2).cloned());
        if !o.latency.unanswered.is_empty() {
            notes.push(format!("no change after {}", o.latency.unanswered.join(", ")));
        }
        let _ = write!(
            h,
            "<tr><td><code>{}</code>{}</td><td><span class=\"chip {}\">{}</span></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&o.variant.name),
            if o.variant.baseline { " <span class=\"chip\">baseline</span>" } else { "" },
            o.status.label(),
            o.status.label(),
            o.first_screen_s.map_or("–".into(), |t| format!("{:.0} ms", t * 1000.0)),
            match (o.latency.p50_ms, o.latency.p95_ms) {
                (Some(a), Some(b)) => format!("{a:.0} / {b:.0} ms"),
                _ => "–".into(),
            },
            if o.contrast.text_cells == 0 {
                "–".into()
            } else {
                format!("{:.1}%", 100.0 * (1.0 - o.contrast.text_pass_ratio()))
            },
            esc(&colors),
            match (o.exit_code, &o.status) {
                (Some(c), _) => c.to_string(),
                (None, Status::Ok) => "running".into(),
                (None, _) => "–".into(),
            },
            esc(&notes.join(" · "))
        );
    }
    h.push_str("</table><h2>Contact sheet</h2><div class=\"sheet\">");
    for o in r.variants {
        let body = match (&o.shot_png, &o.shot) {
            (Some(p), _) => format!("<img src=\"{}\" alt=\"{}\">", esc(p), esc(&o.variant.name)),
            (None, Some(s)) => format!("<pre class=\"term\">{}</pre>", screen_html(s)),
            (None, None) => "<pre class=\"term\">(no screen)</pre>".into(),
        };
        let _ = write!(
            h,
            "<article class=\"card\"><header><code>{}</code><span class=\"chip {}\">{}</span></header>{}<footer>{} frames · {} screens · content {}x{}</footer></article>",
            esc(&o.variant.name),
            o.status.label(),
            o.status.label(),
            body,
            o.frames,
            o.screens,
            o.used_cols,
            o.used_rows
        );
    }
    h.push_str("</div></main>\n");
    std::fs::write(path, h)?;
    Ok(())
}

/// The summary printed at the end of a run.
pub fn summary(r: &Report) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "\n  {:<15} {:>5}", "category", "score");
    for sc in r.scores {
        let v = sc.score.map_or("–".into(), |v| v.to_string());
        let first = sc.explain.first().cloned().unwrap_or_default();
        let _ = writeln!(s, "  {:<15} {:>5}   {}", sc.category, v, clip(&first, 90));
    }
    let _ = writeln!(
        s,
        "\n  {:<22} {:<8} {:>8} {:>15} {:>9}  colors",
        "terminal", "status", "first", "p50/p95 ms", "text<AA"
    );
    for o in r.variants {
        let lat = match (o.latency.p50_ms, o.latency.p95_ms) {
            (Some(a), Some(b)) => format!("{a:.0}/{b:.0}"),
            _ => "–".into(),
        };
        let aa = if o.contrast.text_cells == 0 {
            "–".into()
        } else {
            format!("{:.1}%", 100.0 * (1.0 - o.contrast.text_pass_ratio()))
        };
        let colors = match o.color_respected() {
            None => String::new(),
            Some((true, _)) => "ok".into(),
            Some((false, d)) => format!("✗ {d}"),
        };
        let _ = writeln!(
            s,
            "  {:<22} {:<8} {:>8} {:>15} {:>9}  {}",
            o.variant.name,
            o.status.label(),
            o.first_screen_s.map_or("–".into(), |t| format!("{:.0}ms", t * 1000.0)),
            lat,
            aa,
            clip(&colors, 60)
        );
        if let Status::Broke { error } | Status::Crashed { reason: error } = &o.status {
            let _ = writeln!(s, "  {:<22} {}", "", clip(error, 100));
        }
    }
    s
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}
