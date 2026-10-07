//! The live session: the user's terminal in raw mode on the alternate
//! screen, every input byte forwarded to the program, the program's screen
//! mirrored back (rows that changed, ~30 fps), and a log of what was typed
//! with the screen just before it.

use super::input::Decoder;
use super::print::{Input, Session};
use anina_runner::engine::{self, Engine, Modes, Spec};
use anina_screen::{Cell, Mask, Screen, sgr};
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::time::{Duration, Instant};

/// What `run` needs besides the program.
pub struct Options {
    pub spec: Spec,
    /// The single byte that ends the session (not forwarded).
    pub stop: u8,
    /// Follow the user's terminal size (when no size was forced).
    pub follow_size: bool,
}

/// Puts the terminal back however the session ends.
struct TermGuard;

const RESTORE: &str = "\x1b[0m\x1b[?25h\x1b[?1l\x1b>\x1b[?2004l\x1b[?1004l\x1b[?1049l";

impl TermGuard {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode().context("anina init needs an interactive terminal")?;
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = std::io::stdout().write_all(RESTORE.as_bytes());
            let _ = std::io::stdout().flush();
            prev(info);
        }));
        let mut out = std::io::stdout();
        out.write_all(b"\x1b[?1049h\x1b[H\x1b[2J")?;
        out.flush()?;
        Ok(TermGuard)
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        let mut out = std::io::stdout();
        let _ = out.write_all(RESTORE.as_bytes());
        let _ = out.flush();
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = std::panic::take_hook(); // back to the default hook
    }
}

/// The byte for a `ctrl+x` style stop key.
pub fn stop_byte(key: &str) -> Result<u8> {
    let k = engine::normalize_key(key);
    let Some(rest) = k.strip_prefix("ctrl+") else {
        bail!("the stop key must be a ctrl key (ctrl+], ctrl+g, ...), got {key:?}");
    };
    let b = match rest {
        "space" | "@" => 0,
        "\\" => 0x1c,
        "]" => 0x1d,
        "^" => 0x1e,
        "_" => 0x1f,
        r if r.len() == 1 && r.as_bytes()[0].is_ascii_lowercase() => r.as_bytes()[0] - b'a' + 1,
        _ => bail!("unsupported stop key {key:?}"),
    };
    Ok(b)
}

/// One terminal row as escapes: styles as they change, the rest of the row
/// cleared when it ends early (never at the last column, where an erase
/// would eat the pending-wrap cell).
fn row_ansi(s: &Screen, y: u16) -> String {
    let row = s.row(y);
    let end = row.iter().rposition(|c| *c != Cell::default()).map_or(0, |i| i + 1);
    let mut out = String::new();
    let mut last = None;
    for c in &row[..end] {
        if c.is_spacer() {
            continue;
        }
        let style = (c.fg, c.bg, c.attrs);
        if last != Some(style) {
            out.push_str(&sgr(c.fg, c.bg, c.attrs));
            last = Some(style);
        }
        out.push_str(&c.ch);
    }
    out.push_str("\x1b[0m");
    if end < s.cols as usize {
        out.push_str("\x1b[K");
    }
    out
}

/// Escapes that bring the user's terminal from `prev` to `s`.
fn repaint(prev: Option<&Screen>, s: &Screen) -> String {
    let full = prev.is_none_or(|p| p.cols != s.cols || p.rows != s.rows);
    let mut out = String::from("\x1b[?25l");
    if full {
        out.push_str("\x1b[0m\x1b[H\x1b[2J");
    }
    for y in 0..s.rows {
        if full || prev.is_some_and(|p| p.row(y) != s.row(y)) {
            out.push_str(&format!("\x1b[{};1H", y + 1));
            out.push_str(&row_ansi(s, y));
        }
    }
    out.push_str(&format!("\x1b[{};{}H", s.cursor.y + 1, s.cursor.x + 1));
    if s.cursor.visible {
        out.push_str("\x1b[?25h");
    }
    out
}

/// Input modes the program turned on, mirrored so the user's terminal sends
/// what the program expects (arrow keys in application mode, bracketed paste).
fn mode_changes(prev: &Modes, now: &Modes) -> String {
    let mut out = String::new();
    for (name, on, off) in [
        ("application_cursor_keys", "\x1b[?1h", "\x1b[?1l"),
        ("application_keypad", "\x1b=", "\x1b>"),
        ("bracketed_paste", "\x1b[?2004h", "\x1b[?2004l"),
        ("focus_events", "\x1b[?1004h", "\x1b[?1004l"),
    ] {
        let (a, b) = (prev.get(name).copied().unwrap_or(false), now.get(name).copied().unwrap_or(false));
        if a != b {
            out.push_str(if b { on } else { off });
        }
    }
    out
}

/// An input ends a stretch: count where it was volatile, start over.
fn end_stretch(quiet: &mut Mask, changes: &mut [u8], stretches: &mut [u8], volatile: &mut Mask) {
    for (i, on) in quiet.bits.iter_mut().enumerate() {
        if *on {
            stretches[i] = stretches[i].saturating_add(1);
            if stretches[i] >= 2 {
                volatile.bits[i] = true;
            }
        }
        *on = false;
    }
    changes.iter_mut().for_each(|c| *c = 0);
}

/// Wait up to `ms` for stdin to be readable; read what is there.
fn read_stdin(buf: &mut [u8], ms: i32) -> Result<Option<usize>> {
    let mut fds = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd for the duration of the call
    let n = unsafe { libc::poll(&mut fds, 1, ms) };
    if n < 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted {
            return Ok(Some(0));
        }
        return Err(e.into());
    }
    if n == 0 || fds.revents & (libc::POLLIN | libc::POLLHUP) == 0 {
        return Ok(Some(0));
    }
    // SAFETY: buf is valid for buf.len() bytes
    let r = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
    if r <= 0 {
        return Ok(None); // stdin closed
    }
    Ok(Some(r as usize))
}

/// Run the program under the user's hands until the stop key or its exit.
pub fn run(opts: Options) -> Result<Session> {
    let mut eng = engine::by_name("tui-test")?;
    let spec = opts.spec.clone();
    let guard = TermGuard::enter()?;
    eng.spawn(&spec)?;
    let result = session(eng.as_mut(), &opts);
    let _ = eng.close();
    drop(guard);
    result
}

fn session(eng: &mut dyn Engine, opts: &Options) -> Result<Session> {
    let start = Instant::now();
    let now = || start.elapsed().as_secs_f64();
    let (mut cols, mut rows) = (opts.spec.cols, opts.spec.rows);
    let mut out = std::io::stdout();
    let mut dec = Decoder::default();
    let mut inputs: Vec<Input> = Vec::new();
    let mut resizes = Vec::new();
    let mut latest = eng.screen()?;
    let mut drawn: Option<Screen> = None;
    let mut modes = Modes::new();
    // per stretch between inputs: how often each cell changed once the
    // screen had a moment to settle; twice makes it volatile for that stretch
    let cells = cols as usize * rows as usize;
    let mut quiet_changes = vec![0u8; cells];
    let mut quiet = Mask::new(cols, rows);
    // in how many stretches each cell was volatile: two makes it volatile everywhere
    let mut stretches = vec![0u8; cells];
    let mut volatile = Mask::new(cols, rows);
    let mut last_input = 0.0f64;
    let mut pending_since: Option<Instant> = None;
    let mut last_frame = Instant::now() - Duration::from_secs(1);
    let mut last_modes = last_frame;
    let mut last_size = last_frame;
    let mut buf = [0u8; 8192];
    let frame = Duration::from_millis(33);

    loop {
        let wait = frame.saturating_sub(last_frame.elapsed()).as_millis().clamp(1, 33) as i32;
        let mut stop = false;
        match read_stdin(&mut buf, wait)? {
            None => stop = true,
            Some(0) => {}
            Some(n) => {
                let mut bytes = &buf[..n];
                if let Some(p) = bytes.iter().position(|b| *b == opts.stop) {
                    bytes = &bytes[..p];
                    stop = true;
                }
                let t = now();
                for ev in dec.feed(bytes) {
                    inputs.push(Input { t, ev, before: latest.clone(), quiet: quiet.clone() });
                }
                if !bytes.is_empty() {
                    eng.write(&String::from_utf8_lossy(bytes))?;
                    last_input = t;
                    end_stretch(&mut quiet, &mut quiet_changes, &mut stretches, &mut volatile);
                }
                pending_since = dec.has_pending().then(Instant::now);
            }
        }
        if stop || pending_since.is_some_and(|p| p.elapsed() > Duration::from_millis(40)) {
            let t = now();
            for ev in dec.flush() {
                inputs.push(Input { t, ev, before: latest.clone(), quiet: quiet.clone() });
            }
            pending_since = None;
        }
        if stop {
            break;
        }
        if last_frame.elapsed() < frame {
            continue;
        }
        last_frame = Instant::now();
        let s = eng.screen()?;
        if s.cells != latest.cells && s.cols == quiet.cols && s.rows == quiet.rows && now() - last_input > 0.5 {
            // a cell that changes once (a toast closing, a result arriving) is
            // content; one that keeps changing is an animation, a clock, a spinner
            let changed = anina_runner::masker::from_screens(&[latest.clone(), s.clone()]);
            for (i, c) in changed.bits.iter().enumerate() {
                if *c {
                    quiet_changes[i] = quiet_changes[i].saturating_add(1);
                    if quiet_changes[i] >= 2 {
                        quiet.bits[i] = true;
                    }
                }
            }
        }
        latest = s;
        if drawn.as_ref() != Some(&latest) {
            out.write_all(repaint(drawn.as_ref(), &latest).as_bytes())?;
            out.flush()?;
            drawn = Some(latest.clone());
        }
        if last_modes.elapsed() > Duration::from_millis(150) {
            last_modes = Instant::now();
            if let Ok(m) = eng.modes() {
                let change = mode_changes(&modes, &m);
                if !change.is_empty() {
                    out.write_all(change.as_bytes())?;
                    out.flush()?;
                }
                modes = m;
            }
            if eng.exit_code()?.is_some() {
                latest = eng.screen()?;
                break;
            }
        }
        if opts.follow_size && last_size.elapsed() > Duration::from_millis(250) {
            last_size = Instant::now();
            if let Ok((c, r)) = crossterm::terminal::size()
                && (c, r) != (cols, rows)
                && c > 0
                && r > 0
            {
                eng.resize(c, r)?;
                (cols, rows) = (c, r);
                resizes.push((now(), c, r));
                drawn = None;
            }
        }
    }
    Ok(Session {
        cols: opts.spec.cols,
        rows: opts.spec.rows,
        argv: std::iter::once(opts.spec.program.clone()).chain(opts.spec.args.iter().cloned()).collect(),
        cwd: opts.spec.cwd.as_ref().map(|p| p.display().to_string()),
        env: opts.spec.env.clone(),
        clean_env: opts.spec.clean_env,
        inputs,
        resizes,
        last: latest,
        end: now(),
        last_quiet: quiet,
        volatile,
    })
}
