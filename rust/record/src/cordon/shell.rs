//! The shell lines of a run: a tape's `Exec`, a preset's `[[prepare]]`,
//! explore's `--before`. Each one starts behind the same prefix as the
//! program (`Wrap::argv`), so `strict` holds its writes and its network too.
//!
//! A line keeps the environment and the working directory its caller gives
//! it: the fresh HOME and the empty environment are the program's. When a
//! confined line fails, the error says what the sandbox allows, and that it
//! refused when the line's own output shows the system saying so.

use super::Wrap;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How the system words a refusal: `EPERM` under `sandbox-exec` (a write or
/// a connection), `EROFS` under `bwrap` (a write outside the bound folders).
/// Text, so a machine that words its errors in another language gets the
/// plain note instead.
const REFUSALS: [&str; 2] = ["Operation not permitted", "Read-only file system"];

/// How much of a line's stderr is kept to look for a refusal in.
const KEPT: usize = 16 * 1024;

/// One shell line, ready to run.
pub struct Line<'a> {
    wrap: &'a Wrap,
    script: &'a str,
    cmd: Command,
}

impl Wrap {
    /// `sh FLAGS -c SCRIPT`, behind the prefix.
    pub fn sh<'a>(&'a self, flags: &[&str], script: &'a str) -> Line<'a> {
        let words: Vec<&str> = ["sh"].into_iter().chain(flags.iter().copied()).chain(["-c", script]).collect();
        let argv = self.argv(&words);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        Line { wrap: self, script, cmd }
    }

    /// What to add to a failure inside the sandbox: what it allows, and that
    /// it refused when `stderr` shows it. Nothing when nothing confines.
    pub fn explain(&self, stderr: &str) -> Option<String> {
        if self.prefix.is_empty() {
            return None;
        }
        let lead =
            if REFUSALS.iter().any(|r| stderr.contains(r)) { "the sandbox refused it" } else { "it ran confined" };
        let mut under = String::new();
        for (i, dir) in self.writable.iter().enumerate() {
            let dir = dir.display();
            under.push_str(&if i == 0 { format!("the sandbox root ({dir})") } else { format!(" and {dir}") });
        }
        let network = if self.network { "on" } else { "off (a preset turns it on with network = true)" };
        Some(format!("{lead}: --sandbox strict allows writes only under {under}, and the network is {network}"))
    }
}

impl Line<'_> {
    pub fn cwd(mut self, dir: &Path) -> Self {
        self.cmd.current_dir(dir);
        self
    }

    /// Variables over the inherited environment.
    pub fn envs(mut self, vars: &HashMap<String, String>) -> Self {
        self.cmd.envs(vars);
        self
    }

    /// Run the line to its end. `what` names it in the error (`Exec`, a
    /// prepare step's label).
    pub fn run(mut self, what: &str) -> Result<()> {
        let program = self.cmd.get_program().to_string_lossy().into_owned();
        let (st, said) = self.wait().with_context(|| format!("running {program}"))?;
        if st.success() {
            return Ok(());
        }
        match self.wrap.explain(&said) {
            Some(why) => bail!("{what} failed ({st}): {}\n{why}", self.script),
            None => bail!("{what} failed ({st}): {}", self.script),
        }
    }

    /// The exit status and, when confined, the end of what the line wrote to
    /// stderr. That is shown as it comes, as without a sandbox.
    fn wait(&mut self) -> std::io::Result<(ExitStatus, String)> {
        if self.wrap.prefix.is_empty() {
            return Ok((self.cmd.status()?, String::new()));
        }
        let mut child = self.cmd.stderr(Stdio::piped()).spawn()?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        if let Some(mut pipe) = child.stderr.take() {
            // a thread: a process the line left behind may hold the pipe
            // open long after the line ended, and the wait is for the line
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
        }
        let mut kept: Vec<u8> = Vec::new();
        let mut keep = |chunk: Vec<u8>| {
            let _ = std::io::stderr().write_all(&chunk);
            kept.extend(chunk);
            if kept.len() > KEPT {
                kept.drain(..kept.len() - KEPT);
            }
        };
        let st = loop {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(chunk) => keep(chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(st) = child.try_wait()? {
                        break st;
                    }
                }
                // the pipe closed: nothing more to read, only the end to wait for
                Err(mpsc::RecvTimeoutError::Disconnected) => break child.wait()?,
            }
        };
        // what was written just before the exit
        while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            keep(chunk);
        }
        Ok((st, String::from_utf8_lossy(&kept).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn strict(writable: &[&str], network: bool) -> Wrap {
        Wrap { prefix: vec!["tool".into()], writable: writable.iter().map(PathBuf::from).collect(), network }
    }

    #[test]
    fn a_failure_says_what_the_sandbox_allows_and_whether_it_refused() {
        let off = "the network is off (a preset turns it on with network = true)";
        for (wrap, stderr, want) in [
            (Wrap::default(), "sh: /x: Operation not permitted", None),
            (
                strict(&["/c/sandbox/x"], false),
                "sh: /x: Operation not permitted\n",
                Some(format!(
                    "the sandbox refused it: --sandbox strict allows writes only under the sandbox root \
                     (/c/sandbox/x), and {off}"
                )),
            ),
            (
                strict(&["/c/sandbox/x", "/c"], true),
                "touch: cannot touch '/x': Read-only file system\n",
                Some(
                    "the sandbox refused it: --sandbox strict allows writes only under the sandbox root \
                     (/c/sandbox/x) and /c, and the network is on"
                        .to_string(),
                ),
            ),
            (
                strict(&["/c/sandbox/x"], false),
                "curl: (6) Could not resolve host: example.com\n",
                Some(format!(
                    "it ran confined: --sandbox strict allows writes only under the sandbox root (/c/sandbox/x), \
                     and {off}"
                )),
            ),
        ] {
            assert_eq!(wrap.explain(stderr), want, "{stderr:?}");
        }
    }

    #[test]
    fn a_line_goes_behind_the_prefix_with_its_flags() {
        let wrap = strict(&["/r"], false);
        let line = wrap.sh(&["-u"], "echo hi");
        let args: Vec<_> = line.cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!((line.cmd.get_program().to_str(), args.join(" ").as_str()), (Some("tool"), "sh -u -c echo hi"));
        assert_eq!(Wrap::default().sh(&[], "true").cmd.get_program().to_str(), Some("sh"));
    }

    #[test]
    fn a_line_without_a_sandbox_fails_as_it_always_did() {
        let wrap = Wrap::default();
        assert!(wrap.sh(&[], "true").run("Exec").is_ok());
        let e = wrap.sh(&["-u"], "exit 3").run("Exec").unwrap_err().to_string();
        assert_eq!(e, "Exec failed (exit status: 3): exit 3");
    }

    #[test]
    fn a_confined_line_that_fails_carries_the_note_and_does_not_wait_for_what_it_left_behind() {
        // `env` stands for the sandbox tool: the pipe, the note and the wait
        // are the same. The sleep keeps stderr open after the line ended.
        let wrap = Wrap { prefix: vec!["/usr/bin/env".into()], writable: vec![PathBuf::from("/r")], network: true };
        let t0 = std::time::Instant::now();
        let e = wrap.sh(&[], "sleep 4 & echo 'x: Operation not permitted' >&2; exit 1").run("Exec").unwrap_err();
        assert!(t0.elapsed() < Duration::from_secs(3), "waited for the background process");
        let e = e.to_string();
        assert!(e.starts_with("Exec failed (exit status: 1): sleep 4 &"), "{e}");
        assert!(
            e.ends_with(
                "\nthe sandbox refused it: --sandbox strict allows writes only under the sandbox root (/r), and the network is on"
            ),
            "{e}"
        );
    }
}
