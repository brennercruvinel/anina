//! anina: drive, map, audit and animate any terminal program. No AI needed.
//!
//! The layers, bottom up:
//! - `screen`, `ansi`: the cell grid every other module reads
//! - `engine`: runs a program in a pseudo terminal (tui-test in process, or tmux)
//! - `tape`: the script language (anina's dialect and vhs's)
//! - `record`: plays a tape and records a `cast`
//! - `render`: animated SVG, PNG screenshots, galleries
//! - `volatile`, `privacy`, `sandbox`, `teach`: what keeps runs stable, safe and debuggable
//! - `explore`, `init`, `audit`, `brand`, `preset`, `check`: the products built on top

pub mod ansi;
pub mod audit;
pub mod brand;
pub mod cast;
pub mod check;
pub mod engine;
pub mod explore;
pub mod init;
pub mod preset;
pub mod privacy;
pub mod record;
pub mod render;
pub mod sandbox;
pub mod screen;
pub mod tape;
pub mod teach;
pub mod volatile;
