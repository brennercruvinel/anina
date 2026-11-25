//! The audit's outputs: report.json (every number), report.html (scores,
//! explanations, a contact sheet of every terminal), a terminal summary.

use super::meter::{Outcome, Status};
use super::ratio;
use super::score::{Score, status_line};
use anina_runner::redact::Redactor;
use anina_screen::{Screen, attr};
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

/// Write every number at `path`, its strings redacted by `red`.
pub fn write_json(r: &Report, path: &Path, red: &Redactor) -> Result<()> {
    std::fs::write(path, red.to_json(r)?)?;
    Ok(())
}

/// Redacted by `red` (a failure names the tape's path, an error the user's
/// files) and HTML-escaped.
fn esc(red: &Redactor, s: &str) -> String {
    red.text(s).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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
pub fn screen_html(s: &Screen, red: &Redactor) -> String {
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
            let (fg, bg) = ratio::seen(c);
            let mut text = String::new();
            let style_of =
                |cx: &anina_screen::Cell| (ratio::seen(cx), cx.attrs & (attr::BOLD | attr::ITALIC | attr::UNDERLINE));
            let style = style_of(c);
            while x < row.len() && (row[x].is_spacer() || style_of(&row[x]) == style) {
                if !row[x].is_spacer() {
                    text.push_str(&row[x].ch);
                }
                x += 1;
            }
            let mut css = format!("color:{}", ratio::hex(fg));
            if bg != ratio::DEFAULT_BG {
                let _ = write!(css, ";background:{}", ratio::hex(bg));
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
            let _ = write!(out, "<span style=\"{css}\">{}</span>", esc(red, &text));
        }
        out.push('\n');
    }
    out
}

/// Write the HTML report at `path`; with `embed`, the screenshots go inside
/// it. Every text it shows goes through `red` first.
pub fn write_html(r: &Report, path: &Path, embed: bool, red: &Redactor) -> Result<()> {
    let esc = |s: &str| esc(red, s);
    let dir = path.parent().unwrap_or(Path::new("."));
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
            (Some(p), _) => format!(
                "<img src=\"{}\" alt=\"{}\">",
                anina_render::index::img_src(dir, p, embed),
                esc(&o.variant.name)
            ),
            (None, Some(s)) => format!("<pre class=\"term\">{}</pre>", screen_html(s, red)),
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

#[cfg(test)]
mod tests {
    use super::super::meter::tests::{matrix, ran};
    use super::super::score::all_scores;
    use super::*;
    use anina_screen::escape::parse;
    use std::path::PathBuf;

    /// The default matrix: the baseline answered fast, 60x20 broke on a wait
    /// (the error names a tape under HOME), the NO_COLOR terminal got colors.
    fn outcomes() -> Vec<Outcome> {
        let mut all: Vec<Outcome> = matrix().iter().map(|v| ran(v, "ready")).collect();
        all[0].first_screen_s = Some(0.18);
        (all[0].latency.p50_ms, all[0].latency.p95_ms) = (Some(20.0), Some(40.0));
        let tape = std::env::var("HOME").map_or("tapes/app.tape".to_string(), |h| format!("{h}/tapes/app.tape"));
        all[2].status = Status::Broke { error: format!("{tape}:3: Wait \"Files\" never came") };
        all[5] = ran(&all[5].variant, "\x1b[31mready\x1b[0m");
        all
    }

    fn report<'a>(scores: &'a [Score], variants: &'a [Outcome]) -> Report<'a> {
        Report {
            tool: "anina audit",
            version: "0.0.0",
            generated: "2026-01-01T00:00:00Z".into(),
            target: "demo <app> & co".into(),
            engine: "tui-test".into(),
            scores,
            variants,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("anina-print-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The home folder, when redaction has one to hide (as `Redactor::identity` reads it).
    fn home() -> Option<String> {
        std::env::var("HOME").ok().filter(|h| h.len() > 1)
    }

    /// The fields of the summary's row that starts with `name`.
    fn row<'a>(summary: &'a str, name: &str) -> Vec<&'a str> {
        let line = summary.lines().find(|l| l.split_whitespace().next() == Some(name));
        line.unwrap_or_else(|| panic!("no row {name} in:\n{summary}")).split_whitespace().collect()
    }

    #[test]
    fn the_summary_has_a_row_per_score_and_per_terminal() {
        let all = outcomes();
        let scores = all_scores(&all);
        let s = summary(&report(&scores, &all));
        // (6 + 0.5) / 7 terminals ran; 7 of 9 checks passed elsewhere
        for (category, score) in [
            ("stability", "93"),
            ("responsiveness", "100"),
            ("readability", "100"),
            ("compatibility", "78"),
            ("hygiene", "100"),
        ] {
            assert_eq!(row(&s, category)[..2], [category, score], "{s}");
        }
        assert_eq!(row(&s, "120x40-truecolor"), ["120x40-truecolor", "ok", "180ms", "20/40", "0.0%"]);
        assert_eq!(row(&s, "120x40-256"), ["120x40-256", "ok", "–", "–", "0.0%", "ok"]);
        assert_eq!(row(&s, "60x20-truecolor")[..2], ["60x20-truecolor", "broke"]);
        assert!(s.contains("app.tape:3: Wait \"Files\" never came"), "the failure is not told:\n{s}");
        assert!(s.contains("✗ 5 of 5 visible cells colored with NO_COLOR=1"), "{s}");

        // a score nothing could measure shows a dash, not a zero
        let quiet = [ran(&matrix()[0], "")];
        let scores = all_scores(&quiet);
        let s = summary(&report(&scores, &quiet));
        assert_eq!(row(&s, "responsiveness")[1], "–", "{s}");
        assert_eq!(row(&s, "readability")[1], "–", "{s}");
        assert_eq!(row(&s, "120x40-truecolor"), ["120x40-truecolor", "ok", "–", "–", "–"]);
    }

    #[test]
    fn report_json_holds_every_score_and_terminal_and_nothing_of_the_machine() {
        let all = outcomes();
        let scores = all_scores(&all);
        let dir = scratch("json");
        write_json(&report(&scores, &all), &dir.join("report.json"), &Redactor::identity()).unwrap();
        let text = std::fs::read_to_string(dir.join("report.json")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!((v["tool"].as_str(), v["target"].as_str()), (Some("anina audit"), Some("demo <app> & co")));
        let cats: Vec<&str> = v["scores"].as_array().unwrap().iter().filter_map(|s| s["category"].as_str()).collect();
        assert_eq!(cats, ["stability", "responsiveness", "readability", "compatibility", "hygiene"]);
        assert_eq!(v["scores"][0]["score"], 93);
        let terminals = v["variants"].as_array().unwrap();
        assert_eq!(terminals.len(), 7);
        let base = &terminals[0];
        assert_eq!(base["variant"]["name"], "120x40-truecolor");
        assert_eq!((&base["variant"]["baseline"], &base["variant"]["colors"]), (&true.into(), &"truecolor".into()));
        assert_eq!((&base["status"]["kind"], &base["exit_code"]), (&"ok".into(), &0.into()));
        assert_eq!(terminals[2]["status"]["kind"], "broke");
        assert_eq!(terminals[5]["colors_respected"], false);
        // the screenshot is a file beside the report, not a field of it
        assert!(base.get("shot").is_none(), "{base}");
        if let Some(home) = home() {
            assert!(!text.contains(&home), "the report names the home folder");
            assert_eq!(terminals[2]["status"]["error"], "~/tapes/app.tape:3: Wait \"Files\" never came");
        }
    }

    #[test]
    fn report_html_escapes_what_it_shows_and_lists_every_terminal() {
        let mut all = outcomes();
        all[1].shot_png = Some("terminals/80x24-truecolor/screen.png".into());
        all[6].shot = None;
        let scores = all_scores(&all);
        let dir = scratch("html");
        std::fs::create_dir_all(dir.join("terminals/80x24-truecolor")).unwrap();
        std::fs::write(dir.join("terminals/80x24-truecolor/screen.png"), b"PNG!").unwrap();
        let red = Redactor::identity();
        let page = |embed: bool| {
            write_html(&report(&scores, &all), &dir.join("report.html"), embed, &red).unwrap();
            std::fs::read_to_string(dir.join("report.html")).unwrap()
        };
        let (html, embedded) = (page(false), page(true));
        let _ = std::fs::remove_dir_all(&dir);

        assert!(html.contains("<title>anina audit · demo &lt;app&gt; &amp; co</title>"), "the target is not escaped");
        assert!(!html.contains("<app>"));
        for o in &all {
            assert!(html.contains(&format!("<td><code>{}</code>", o.variant.name)), "{} has no row", o.variant.name);
            assert!(
                html.contains(&format!("<header><code>{}</code>", o.variant.name)),
                "{} has no card",
                o.variant.name
            );
        }
        assert_eq!(html.matches("<span class=\"chip\">baseline</span>").count(), 1);
        assert_eq!(html.matches("<td><span class=\"chip broke\">broke</span></td>").count(), 1);
        assert!(html.contains("✗ 5 of 5 visible cells colored with NO_COLOR=1"));
        // a terminal shows its PNG, or its screen as text, or says it has none
        assert!(html.contains("<img src=\"terminals/80x24-truecolor/screen.png\" alt=\"80x24-truecolor\">"));
        assert!(html.contains("<pre class=\"term\"><span style=\"color:#e5e5e5\">ready"));
        assert_eq!(html.matches("<pre class=\"term\">(no screen)</pre>").count(), 1);
        // --embed puts the image inside the page
        assert!(embedded.contains("<img src=\"data:image/png;base64,UE5HIQ==\" alt=\"80x24-truecolor\">"));
        if let Some(home) = home() {
            assert!(!html.contains(&home), "the page names the home folder");
            assert!(html.contains("~/tapes/app.tape:3: Wait &quot;Files&quot; never came"));
        }
    }

    #[test]
    fn a_score_is_good_from_90_fair_from_50_and_poor_below() {
        let mark = |score: Option<u8>| Score { category: "stability", score, rule: "r", explain: Vec::new() };
        let scores: Vec<Score> = [Some(100), Some(90), Some(89), Some(50), Some(49), Some(0), None].map(mark).into();
        let dir = scratch("bands");
        write_html(&report(&scores, &[]), &dir.join("report.html"), false, &Redactor::default()).unwrap();
        let html = std::fs::read_to_string(dir.join("report.html")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        for (band, shown, filled) in [
            ("good", "100", 100),
            ("good", "90", 90),
            ("fair", "89", 89),
            ("fair", "50", 50),
            ("poor", "49", 49),
            ("poor", "0", 0),
            // nothing measured: a dash and an empty gauge
            ("na", "–", 0),
        ] {
            let gauge = format!(
                "<section class=\"score {band}\"><div class=\"gauge\" style=\"--v:{filled}\"><span>{shown}</span>"
            );
            assert_eq!(html.matches(&gauge).count(), 1, "{band} {shown}");
        }
    }

    #[test]
    fn a_screen_becomes_spans_in_the_colors_it_is_seen_in() {
        let s = parse("\x1b[1;31mhi\x1b[0m x\n\x1b[44m<b>\x1b[0m\n\x1b[3;4;7mi\x1b[0m", 6, 4);
        let want = concat!(
            "<span style=\"color:#cd0000;font-weight:700\">hi</span><span style=\"color:#e5e5e5\"> x  </span>\n",
            "<span style=\"color:#e5e5e5;background:#0000ee\">&lt;b&gt;</span><span style=\"color:#e5e5e5\">   </span>\n",
            "<span style=\"color:#000000;background:#e5e5e5;font-style:italic;text-decoration:underline\">i</span>",
            "<span style=\"color:#e5e5e5\">     </span>\n",
        );
        // the fourth row is empty and left out
        let red = Redactor::default();
        assert_eq!(screen_html(&s, &red), want);
        assert_eq!(screen_html(&parse("", 4, 2), &red), "<span style=\"color:#e5e5e5\">    </span>\n");
    }
}
