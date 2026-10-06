//! The tmux engine: a private tmux server per process. Slower than tui-test
//! (a process per screen read) but lets a human attach and watch:
//! `tmux -L anina-<pid> attach`.

use super::{Engine, Modes, Spec};
use crate::screen::{Cursor, Screen};
use anyhow::{Context, Result, bail};
use std::process::Command;

pub struct Tmux {
    socket: String,
    running: bool,
    cols: u16,
    rows: u16,
}

impl Default for Tmux {
    fn default() -> Self {
        Tmux { socket: format!("anina-{}", std::process::id()), running: false, cols: 100, rows: 30 }
    }
}

const SESSION: &str = "anina";

impl Tmux {
    fn tmux(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("tmux")
            .args(["-L", &self.socket, "-f", "/dev/null"])
            .args(args)
            .output()
            .context("tmux is not installed (brew install tmux / apt install tmux)")?;
        if !out.status.success() {
            bail!("tmux {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// tmux key names for the normalized `mod+key` form.
fn tmux_key(k: &str) -> String {
    let mut parts: Vec<&str> = k.split('+').collect();
    let key = parts.pop().unwrap_or("");
    let base = match key {
        "enter" => "Enter".to_string(),
        "esc" => "Escape".into(),
        "tab" if parts.contains(&"shift") => return "BTab".into(),
        "tab" => "Tab".into(),
        "backspace" => "BSpace".into(),
        "space" => "Space".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "PPage".into(),
        "pagedown" => "NPage".into(),
        "delete" => "DC".into(),
        "insert" => "IC".into(),
        f if f.starts_with('f') && f.len() > 1 && f[1..].chars().all(|c| c.is_ascii_digit()) => f.to_uppercase(),
        other => other.to_string(),
    };
    let mut prefix = String::new();
    for m in parts {
        prefix.push_str(match m {
            "ctrl" => "C-",
            "alt" => "M-",
            "shift" => "S-",
            _ => "",
        });
    }
    prefix + &base
}

impl Engine for Tmux {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn spawn(&mut self, spec: &Spec) -> Result<()> {
        let (program, args) = spec.argv();
        let mut argv: Vec<String> = Vec::new();
        if !spec.clean_env {
            // tmux passes its own environment; set ours through env(1)
            argv.push("/usr/bin/env".into());
            if !spec.env.iter().any(|(k, _)| k == "COLORTERM") {
                argv.push("COLORTERM=truecolor".into());
            }
            argv.extend(spec.env.iter().map(|(k, v)| format!("{k}={v}")));
        }
        argv.push(program);
        argv.extend(args);
        let cwd = spec.cwd.clone().unwrap_or(std::env::current_dir()?);
        let (cols, rows) = (spec.cols.to_string(), spec.rows.to_string());
        let mut a: Vec<&str> = vec!["new-session", "-d", "-s", SESSION, "-x", &cols, "-y", &rows, "-c"];
        let cwd_s = cwd.display().to_string();
        a.push(&cwd_s);
        a.extend(argv.iter().map(|s| s.as_str()));
        self.tmux(&a)?;
        for opt in [["remain-on-exit", "on"], ["status", "off"]] {
            self.tmux(&["set-option", "-t", SESSION, opt[0], opt[1]])?;
        }
        self.tmux(&["set-option", "-g", "default-terminal", "tmux-256color"])?;
        self.tmux(&["set-option", "-ga", "terminal-overrides", ",*:RGB"])?;
        self.running = true;
        self.cols = spec.cols;
        self.rows = spec.rows;
        Ok(())
    }

    fn write(&mut self, data: &str) -> Result<()> {
        self.tmux(&["send-keys", "-t", SESSION, "-l", data]).map(|_| ())
    }

    fn key(&mut self, key: &str) -> Result<()> {
        let k = tmux_key(key);
        self.tmux(&["send-keys", "-t", SESSION, &k]).map(|_| ())
    }

    fn screen(&mut self) -> Result<Screen> {
        let raw = self.tmux(&["capture-pane", "-t", SESSION, "-e", "-N", "-p"])?;
        let mut s = crate::ansi::parse(raw.trim_end_matches('\n'), self.cols, self.rows);
        let cur = self.tmux(&["display-message", "-t", SESSION, "-p", "#{cursor_x} #{cursor_y} #{cursor_flag}"])?;
        let v: Vec<u16> = cur.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        if v.len() == 3 {
            s.cursor = Cursor { x: v[0], y: v[1], visible: v[2] == 1 };
        }
        Ok(s)
    }

    fn modes(&mut self) -> Result<Modes> {
        let f = "#{alternate_on} #{cursor_flag} #{mouse_any_flag} #{bracketed_paste}";
        let out = self.tmux(&["display-message", "-t", SESSION, "-p", f])?;
        let v: Vec<bool> = out.split_whitespace().map(|n| n == "1").collect();
        let mut m = Modes::new();
        for (name, on) in ["alt_screen", "cursor_visible", "mouse", "bracketed_paste"].iter().zip(v) {
            m.insert(name.to_string(), on);
        }
        Ok(m)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.tmux(&["resize-window", "-t", SESSION, "-x", &cols.to_string(), "-y", &rows.to_string()])?;
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }

    fn exit_code(&mut self) -> Result<Option<i32>> {
        let out = self.tmux(&["display-message", "-t", SESSION, "-p", "#{pane_dead} #{pane_dead_status}"])?;
        let mut it = out.split_whitespace();
        if it.next() == Some("1") {
            return Ok(Some(it.next().and_then(|c| c.parse().ok()).unwrap_or(0)));
        }
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        if self.running {
            let _ = self.tmux(&["kill-server"]);
            self.running = false;
        }
        Ok(())
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
