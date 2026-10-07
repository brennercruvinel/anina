//! A recording: the distinct screens (as ANSI rows) and the frames that
//! point at them in time. Renderers read it; `to_asciicast` exports it to the
//! asciinema ecosystem (player, agg, asciinema.org).

use crate::Screen;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    /// Seconds on the recording timeline (hidden time cut, holds added).
    pub t: f64,
    /// Index into `Cast::screens`.
    pub s: usize,
    /// Cursor cell when it is visible.
    pub cursor: Option<[u16; 2]>,
    #[serde(default)]
    pub caption: String,
    /// Playback speed from this frame on (2 = twice as fast).
    #[serde(default = "one")]
    pub speed: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cast {
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub theme: String,
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub end_hold: f64,
    #[serde(default)]
    pub poster: usize,
    #[serde(default)]
    pub tape: String,
    #[serde(default)]
    pub recorded: String,
    #[serde(default)]
    pub engine: String,
    pub screens: Vec<String>,
    pub frames: Vec<Frame>,
}

impl Cast {
    pub fn load(path: &Path) -> Result<Cast> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let text = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut s)?;
            s
        } else {
            String::from_utf8(bytes)?
        };
        serde_json::from_str(&text).with_context(|| format!("{} is not a cast", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string(self)?;
        let bytes = if path.extension().is_some_and(|e| e == "gz") {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            std::io::Write::write_all(&mut gz, json.as_bytes())?;
            gz.finish()?
        } else {
            json.into_bytes()
        };
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
    }

    pub fn screen(&self, i: usize) -> Screen {
        crate::escape::parse(&self.screens[i], self.cols, self.rows)
    }

    /// Recording length in seconds, the end hold included.
    pub fn duration(&self) -> f64 {
        self.frames.last().map_or(0.0, |f| f.t) + self.end_hold
    }

    /// asciicast v3 (https://docs.asciinema.org/manual/asciicast/v3/): each
    /// frame redraws the whole screen; captions become markers.
    pub fn to_asciicast(&self) -> String {
        let header = serde_json::json!({
            "version": 3,
            "term": {"cols": self.cols, "rows": self.rows, "type": "xterm-256color"},
            "title": self.title,
            "command": self.tape,
        });
        let mut out = header.to_string();
        out.push('\n');
        let mut last_t = 0.0;
        let mut last_caption = String::new();
        for f in &self.frames {
            let dt = (f.t - last_t).max(0.0);
            last_t = f.t;
            let mut data = String::from("\x1b[H\x1b[2J\x1b[?25l");
            data.push_str(&self.screens[f.s].replace('\n', "\r\n"));
            if let Some([x, y]) = f.cursor {
                data.push_str(&format!("\x1b[{};{}H\x1b[?25h", y + 1, x + 1));
            }
            out.push_str(&serde_json::json!([round3(dt), "o", data]).to_string());
            out.push('\n');
            if f.caption != last_caption && !f.caption.is_empty() {
                out.push_str(&serde_json::json!([0.0, "m", f.caption]).to_string());
                out.push('\n');
            }
            last_caption = f.caption.clone();
        }
        if self.end_hold > 0.0 {
            out.push_str(&serde_json::json!([round3(self.end_hold), "o", ""]).to_string());
            out.push('\n');
        }
        out
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Now as `YYYY-MM-DDTHH:MM:SSZ` (UTC), without a date crate.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}
