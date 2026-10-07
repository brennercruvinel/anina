//! Runs a terminal program for anina and keeps the run stable and safe.
//!
//! - `engine`: runs a program in a pseudo terminal (tui-test in process, or tmux)
//! - `script`: the tape language (anina's dialect and vhs's)
//! - `masker`: finds the cells that change by themselves (clocks, spinners)
//! - `redact`: hides what identifies the machine before anything is written

pub mod engine;
pub mod masker;
pub mod redact;
pub mod script;
