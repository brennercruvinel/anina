//! What a crate below the command line has to say while it works. It prints
//! nothing: each line goes to the `Notice` its caller passed, and the caller
//! decides where the line shows and how it starts. `anina` writes them to
//! stderr, behind `anina: `.

/// Where a status line and a warning go.
pub trait Notice {
    /// A line of progress or of result, as the text to show.
    fn status(&self, line: &str);
    /// Something that was not as asked; the work went on.
    fn warning(&self, line: &str);
}
