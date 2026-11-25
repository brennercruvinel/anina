//! The default engine: microsoft/tui-test's terminal (alacritty_terminal over
//! a native pty), in process. A full screen read is a fraction of a
//! millisecond, so the recorder can sample at any rate.

use super::{Engine, Modes, Spec};
use anina_screen::{Cell, Color, Cursor, Screen, attr};
use anyhow::{Result, anyhow, bail};
use std::sync::atomic::{AtomicU32, Ordering};
use tui_test::{CellColor, KeyAction, Operation, OperationResult, RunOptions, Session};

static SEQ: AtomicU32 = AtomicU32::new(0);

#[derive(Default)]
pub struct TuiTest {
    session: Option<Session>,
    cols: u16,
    rows: u16,
}

impl TuiTest {
    fn s(&self) -> Result<&Session> {
        self.session.as_ref().ok_or_else(|| anyhow!("no program running: spawn first"))
    }
    fn op(&self, op: Operation) -> Result<OperationResult> {
        self.s()?.execute(op).map_err(|e| anyhow!("tui-test: {e}"))
    }
}

fn color(c: &CellColor) -> Color {
    match c {
        CellColor::Default => Color::Default,
        CellColor::Indexed(n) => Color::Idx(*n),
        CellColor::Rgb(r, g, b) => Color::Rgb(*r, *g, *b),
    }
}

impl Engine for TuiTest {
    fn name(&self) -> &'static str {
        "tui-test"
    }

    fn spawn(&mut self, spec: &Spec) -> Result<()> {
        if self.session.is_some() {
            bail!("a program is already running in this engine");
        }
        let name = format!("anina-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        let session = Session::new(name);
        let (program, args) = spec.argv();
        let env = if spec.clean_env { Vec::new() } else { spec.env.clone() };
        let mut opts = RunOptions {
            backend: Default::default(),
            program,
            args,
            profile: Default::default(),
            cols: spec.cols,
            rows: spec.rows,
            cwd: spec.cwd.as_ref().map(|p| p.display().to_string()),
            env,
            wait_ready: None,
            restart: false,
            timeouts: Default::default(),
            recording: Default::default(),
        };
        if !spec.clean_env && !opts.env.iter().any(|(k, _)| k == "COLORTERM") {
            opts.env.push(("COLORTERM".into(), "truecolor".into()));
        }
        session.run(opts).map_err(|e| anyhow!("could not start {}: {e}", spec.program))?;
        self.session = Some(session);
        self.cols = spec.cols;
        self.rows = spec.rows;
        Ok(())
    }

    fn write(&mut self, data: &str) -> Result<()> {
        self.op(Operation::Write { data: data.into() }).map(|_| ())
    }

    fn key(&mut self, key: &str) -> Result<()> {
        self.op(Operation::Key { keys: vec![key.into()], action: KeyAction::Press }).map(|_| ())
    }

    fn screen(&mut self) -> Result<Screen> {
        let OperationResult::Cells(cells) = self.op(Operation::Cells { x: 0, y: 0, w: self.cols, h: self.rows })?
        else {
            bail!("tui-test returned no cells");
        };
        let mut s = Screen::blank(self.cols, self.rows);
        for c in cells {
            if c.x >= self.cols || c.y >= self.rows {
                continue;
            }
            let mut a = 0;
            for (on, flag) in [
                (c.bold, attr::BOLD),
                (c.dim, attr::DIM),
                (c.italic, attr::ITALIC),
                (c.underline, attr::UNDERLINE),
                (c.inverse, attr::INVERSE),
                (c.strike, attr::STRIKE),
                (c.invisible, attr::INVISIBLE),
                (c.blink, attr::BLINK),
            ] {
                if on {
                    a |= flag;
                }
            }
            *s.cell_mut(c.x, c.y) = Cell { ch: c.char, fg: color(&c.fg), bg: color(&c.bg), attrs: a };
        }
        if let OperationResult::Cursor(cur) = self.op(Operation::GetCursor)? {
            s.cursor = Cursor { x: cur.x, y: cur.y, visible: cur.visible };
        }
        Ok(s)
    }

    fn modes(&mut self) -> Result<Modes> {
        match self.op(Operation::GetModes)? {
            OperationResult::Modes(m) => Ok(m),
            _ => bail!("tui-test returned no modes"),
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.op(Operation::Resize { cols, rows })?;
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }

    fn exit_code(&mut self) -> Result<Option<i32>> {
        // GetExitCode is the last shell command's status (shell integration);
        // the process itself is in State::exited
        match self.op(Operation::State)? {
            OperationResult::State(st) => Ok(st.exited),
            _ => Ok(None),
        }
    }

    fn close(&mut self) -> Result<()> {
        if let Some(s) = self.session.take() {
            s.close().map_err(|e| anyhow!("tui-test: {e}"))?;
        }
        Ok(())
    }
}

impl Drop for TuiTest {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
