//! How a command ends, as the code a pipeline reads. Only the binary knows
//! these: the crates below return values and plain errors, and the command
//! that called them says which ending a failure is.
//!
//! | code | ending |
//! | --- | --- |
//! | 0 | the command did what it was asked |
//! | 1 | it failed some other way (a file missing, a preset unknown) |
//! | 2 | the command line was not understood |
//! | 3 | the journey failed: a wait did not hold, a part did not play |
//! | 4 | there is something to hide in what was written |
//! | 5 | the two runs differ |
//! | 6 | a score is under the limit `--min-score` gave |
//!
//! Clap ends a command line it cannot read with 2 by itself.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Failed = 1,
    Usage = 2,
    Journey = 3,
    Hidden = 4,
    Differ = 5,
    Score = 6,
}

/// An error that knows its ending. It reads as its message and nothing else,
/// so what a command prints does not change with the code it exits with.
#[derive(Debug)]
struct Ended {
    ending: Ending,
    message: String,
}

impl fmt::Display for Ended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Ended {}

/// A failure of this `ending`, with the message to show.
pub fn ended(ending: Ending, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Ended { ending, message: message.into() })
}

/// An error from a crate below, read as this `ending`: its text is kept.
pub fn read_as(ending: Ending, e: anyhow::Error) -> anyhow::Error {
    ended(ending, format!("{e:#}"))
}

/// The ending of one failure: what it was marked as, else `Failed`.
fn of(e: &anyhow::Error) -> Ending {
    e.chain().find_map(|c| c.downcast_ref::<Ended>()).map_or(Ending::Failed, |x| x.ending)
}

/// The exit code for the failures of one command, in the order they came.
/// Something to hide comes before anything else: a failed wait that also
/// wrote a secret is a leak first.
pub fn code(failed: &[anyhow::Error]) -> i32 {
    let all: Vec<Ending> = failed.iter().map(of).collect();
    let first = if all.contains(&Ending::Hidden) { Some(Ending::Hidden) } else { all.first().copied() };
    first.map_or(0, |e| e as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ending_has_its_code_and_an_unmarked_failure_is_one() {
        for (e, want) in [
            (Ending::Failed, 1),
            (Ending::Usage, 2),
            (Ending::Journey, 3),
            (Ending::Hidden, 4),
            (Ending::Differ, 5),
            (Ending::Score, 6),
        ] {
            assert_eq!(code(&[ended(e, "x")]), want);
        }
        assert_eq!(code(&[]), 0);
        assert_eq!(code(&[anyhow::anyhow!("a file is missing")]), 1);
    }

    #[test]
    fn a_marked_failure_reads_as_its_message_alone() {
        let e = ended(Ending::Differ, "2 differences (14 files)");
        assert_eq!(format!("{e:#}"), "2 differences (14 files)");
        let below = anyhow::anyhow!("basics.tape:37: \"x\" did not appear").context("playing basics");
        let text = format!("{below:#}");
        assert_eq!(format!("{:#}", read_as(Ending::Journey, below)), text);
    }

    #[test]
    fn a_context_added_above_keeps_the_ending() {
        let e = ended(Ending::Journey, "1 part(s) failed: basics").context("anina run");
        assert_eq!(code(&[e]), 3);
    }

    #[test]
    fn something_to_hide_comes_before_the_failure_that_came_first() {
        let journey = ended(Ending::Journey, "1 part(s) failed: basics");
        let leak = ended(Ending::Hidden, "1 thing to hide in 1 file");
        assert_eq!(code(&[journey, leak]), 4);
        assert_eq!(code(&[ended(Ending::Journey, "x"), anyhow::anyhow!("y")]), 3);
    }
}
