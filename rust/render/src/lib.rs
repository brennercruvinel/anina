//! Renderers: the animated SVG of a cast, PNG screenshots of a screen, the
//! HTML gallery of a run. Themes give the window, brand and palette.

pub mod brand;
pub mod fonts;
pub mod index;
pub mod pixel;
pub mod shape;
pub mod theme;

use anina_screen::frames::Cast;
use anyhow::Result;
use std::path::{Path, PathBuf};
use theme::Theme;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The cast to render (.cast.json or .cast.json.gz)
    pub cast: PathBuf,
    /// Output file (default: next to the cast, .svg)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Theme: a built-in name, a path to a theme JSON (anina or vhs), or auto
    #[arg(long)]
    pub theme: Option<String>,
    /// Longest a still screen stays, in seconds (default: the theme's)
    #[arg(long)]
    pub max_hold: Option<f64>,
    /// Play everything X times faster
    #[arg(long, default_value_t = 1.0)]
    pub speed: f64,
    /// Playback frame rate cap (default: the theme's)
    #[arg(long)]
    pub max_fps: Option<f64>,
    /// Dark window chrome (the theme's own); with --light writes both
    #[arg(long)]
    pub dark: bool,
    /// Light window chrome around the unchanged terminal
    #[arg(long)]
    pub light: bool,
    /// Do not embed the font subset (smaller, but looks different per machine)
    #[arg(long)]
    pub no_font: bool,
    /// Also write the poster frame as a PNG
    #[arg(long)]
    pub poster: bool,
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let stem = p.file_stem().map_or("out".into(), |s| s.to_string_lossy().into_owned());
    p.with_file_name(format!("{stem}{suffix}.svg"))
}

pub fn run(a: Args) -> Result<()> {
    let cast = Cast::load(&a.cast)?;
    let th = Theme::resolve(a.theme.as_deref(), &cast)?;
    let opts = shape::Options { max_hold: a.max_hold, speed: a.speed, max_fps: a.max_fps, embed_font: !a.no_font };
    let out = a.out.clone().unwrap_or_else(|| {
        let s = a.cast.to_string_lossy();
        PathBuf::from(format!("{}.svg", s.trim_end_matches(".gz").trim_end_matches(".json").trim_end_matches(".cast")))
    });
    let variants: Vec<(PathBuf, Theme)> = match (a.dark, a.light) {
        (true, true) => {
            vec![(with_suffix(&out, "-dark"), th.clone()), (with_suffix(&out, "-light"), th.light_chrome())]
        }
        (false, true) => vec![(out.clone(), th.light_chrome())],
        _ => vec![(out.clone(), th.clone())],
    };
    for (path, t) in &variants {
        let r = shape::render_cast(&cast, t, &opts)?;
        shape::write(path, &r.svg)?;
        let gz = gzip_len(r.svg.as_bytes());
        eprintln!(
            "anina: {} frames, {} rows, {:.1}s, poster at {:.1}s, {} KB ({} KB gzip) -> {}",
            cast.frames.len(),
            r.rows,
            r.total,
            r.poster,
            r.svg.len() / 1024,
            gz / 1024,
            path.display()
        );
    }
    shape::write(&out.with_extension("txt"), &shape::transcript(&cast))?;
    if variants.len() == 2 {
        let name = |p: &Path| p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let snippet = format!(
            "<picture>\n  <source media=\"(prefers-color-scheme: light)\" srcset=\"{}\">\n  <img src=\"{}\" alt=\"{}\">\n</picture>\n",
            name(&variants[1].0),
            name(&variants[0].0),
            shape::esc(&cast.title)
        );
        let p = out.with_extension("picture.html");
        std::fs::write(&p, &snippet)?;
        eprintln!("anina: README snippet -> {}\n{snippet}", p.display());
    }
    if a.poster && !cast.frames.is_empty() {
        let f = &cast.frames[cast.poster.min(cast.frames.len() - 1)];
        let p = out.with_file_name(format!(
            "{}-poster.png",
            out.file_stem().map_or("out".into(), |s| s.to_string_lossy().into_owned())
        ));
        pixel::screen_png_with(&cast.screen(f.s), &cast.title, &th, &p)?;
        eprintln!("anina: poster -> {}", p.display());
    }
    Ok(())
}

/// The gzip size of `data`, what a web server sends (via gzip(1) when
/// present, else the raw size).
fn gzip_len(data: &[u8]) -> usize {
    use std::io::Write;
    let child = std::process::Command::new("gzip")
        .arg("-9c")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return data.len() };
    // feed stdin from a thread: gzip's output could fill the pipe meanwhile
    let writer = child.stdin.take().map(|mut stdin| {
        let data = data.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        })
    });
    let out = child.wait_with_output();
    if let Some(w) = writer {
        let _ = w.join();
    }
    out.map_or(data.len(), |o| o.stdout.len())
}
