//! The tmux engine: a private tmux server per process. Slower than tui-test
//! (a process per screen read) but lets a human attach and watch:
//! `tmux -L anina-<pid> attach`.

use super::{Engine, Modes, Spec};
use anina_screen::{Cursor, Screen};
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

/// Text as one `send-keys -l` argument. tmux ends a command at an argument
/// that ends in `;`, so a typed `;` was dropped; `\;` is how it takes one.
/// (`--` before it keeps a typed `-` from reading as a flag.)
fn literal(data: &str) -> String {
    match data.strip_suffix(';') {
        Some(head) => format!("{head}\\;"),
        None => data.to_string(),
    }
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
        self.tmux(&["send-keys", "-t", SESSION, "-l", "--", &literal(data)]).map(|_| ())
    }

    fn key(&mut self, key: &str) -> Result<()> {
        let k = tmux_key(key);
        self.tmux(&["send-keys", "-t", SESSION, &k]).map(|_| ())
    }

    fn screen(&mut self) -> Result<Screen> {
        let raw = self.tmux(&["capture-pane", "-t", SESSION, "-e", "-N", "-p"])?;
        let mut s = anina_screen::escape::parse(raw.trim_end_matches('\n'), self.cols, self.rows);
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

#[cfg(test)]
mod tests {
    use super::super::{normalize_key, wait_for};
    use super::*;
    use anina_screen::Pattern;
    use std::time::{Duration, Instant};

    #[test]
    fn a_key_goes_to_tmux_under_the_name_tmux_gives_it() {
        for (key, tmux) in [
            ("enter", "Enter"),
            ("esc", "Escape"),
            ("tab", "Tab"),
            ("shift+tab", "BTab"),
            ("backspace", "BSpace"),
            ("space", "Space"),
            ("up", "Up"),
            ("down", "Down"),
            ("left", "Left"),
            ("right", "Right"),
            ("home", "Home"),
            ("end", "End"),
            ("pageup", "PPage"),
            ("pagedown", "NPage"),
            ("delete", "DC"),
            ("insert", "IC"),
            ("f1", "F1"),
            ("f12", "F12"),
            // a letter is itself, case kept, `f` included
            ("a", "a"),
            ("G", "G"),
            ("f", "f"),
            ("?", "?"),
            ("ctrl+c", "C-c"),
            ("alt+x", "M-x"),
            ("ctrl+alt+x", "C-M-x"),
            ("shift+up", "S-Up"),
            ("ctrl+right", "C-Right"),
            ("ctrl+shift+left", "C-S-Left"),
            ("alt+enter", "M-Enter"),
            ("ctrl+space", "C-Space"),
            ("shift+f5", "S-F5"),
        ] {
            assert_eq!(tmux_key(key), tmux, "{key}");
            // and tmux's name reads back as the key
            assert_eq!(normalize_key(tmux), key, "{tmux}");
        }
    }

    /// Whether tmux is here to be tried. Without it the test passes on a
    /// developer's machine, untried; in CI it fails, where it is installed.
    /// One tmux server per process and one session name in it: the tests that
    /// start a program take turns.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tmux_here() -> bool {
        if Command::new("tmux").arg("-V").output().is_ok_and(|o| o.status.success()) {
            return true;
        }
        assert!(std::env::var_os("CI").is_none(), "tmux is missing: the tmux engine was not tested");
        false
    }

    /// tmux reads `;` at the end of an argument as the end of its command, and
    /// `-` at its start as a flag: both are text when a tape types them.
    #[test]
    fn a_semicolon_and_a_dash_are_typed_as_written() {
        assert_eq!(
            (literal("a;"), literal(";"), literal("a;b"), literal("-l")),
            ("a\\;".into(), "\\;".into(), "a;b".into(), "-l".into())
        );
        if !tmux_here() {
            return;
        }
        let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let mut spec = Spec::new("/bin/sh", vec!["-c".into(), "read l; echo \"got[$l]\"; sleep 2".into()]);
        (spec.cols, spec.rows) = (40, 5);
        let mut e = Tmux::default();
        e.spawn(&spec).unwrap();
        for typed in [";", "-a", "b;", " -l"] {
            e.write(typed).unwrap();
        }
        e.key("enter").unwrap();
        let (seen, s) =
            wait_for(&mut e, &Pattern::Text("got[;-ab; -l]".into()), false, Duration::from_secs(10)).unwrap();
        e.close().unwrap();
        assert!(seen, "the screen was:\n{}", s.text());
    }

    #[test]
    fn a_program_in_tmux_is_typed_to_resized_and_seen_to_exit() {
        if !tmux_here() {
            return;
        }
        let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let script = "printf 'name? '; read n; echo \"hello $n\"; read x; stty size; read y; exit 3";
        let mut spec = Spec::new("/bin/sh", vec!["-c".into(), script.into()]);
        (spec.cols, spec.rows) = (40, 5);
        let mut e = Tmux::default();
        e.spawn(&spec).unwrap();
        let shows = |e: &mut Tmux, text: &str| {
            let (seen, s) = wait_for(e, &Pattern::Text(text.into()), false, Duration::from_secs(10)).unwrap();
            assert!(seen, "{text:?} never showed; the screen was:\n{}", s.text());
            s
        };
        let s = shows(&mut e, "name?");
        assert_eq!((s.cols, s.rows, e.exit_code().unwrap()), (40, 5, None));
        e.write("anina").unwrap();
        e.key("enter").unwrap();
        shows(&mut e, "hello anina");
        assert_eq!(e.modes().unwrap().get("alt_screen"), Some(&false));
        // the program sees the new size, not only anina
        e.resize(60, 8).unwrap();
        e.key("enter").unwrap();
        let s = shows(&mut e, "8 60");
        assert_eq!((s.cols, s.rows), (60, 8));
        e.key("enter").unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let code = loop {
            if let Some(code) = e.exit_code().unwrap() {
                break code;
            }
            assert!(Instant::now() < end, "the program never exited");
            std::thread::sleep(Duration::from_millis(25));
        };
        assert_eq!(code, 3);
        e.close().unwrap();
    }
}
