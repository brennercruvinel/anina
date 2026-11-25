//! The recorder, played against a real shell.

use super::*;
use anina_runner::redact::Redactor;

/// One sample a second, and the program draws between two of them: the
/// screen a `Wait` holds on is read off the sampling clock. It is a frame
/// of the step that waited all the same, so what a step ends with (the
/// transcript shows it) does not follow where the clock stood.
#[test]
fn the_screen_a_wait_held_on_is_a_frame_of_its_step() {
    let src = "Set FPS 1\nSpawn sh -c 'sleep 0.3; printf ready; sleep 2'\nWait \"ready\"\nCaption \"after\"\n";
    let tape = Tape::parse_str(src, "wait.tape", &HashMap::new()).unwrap();
    let out_dir = std::env::temp_dir().join(format!("anina-wait-{}", std::process::id()));
    let cast = record(&tape, Config::default(), Options { out_dir, quiet: true, ..Options::default() }).unwrap().cast;
    let waited = cast.frames.iter().filter(|f| f.caption.is_empty()).any(|f| cast.screens[f.s].contains("ready"));
    assert!(waited, "no frame of the step that waited shows what its wait held on");
}

/// A rule of `--redact` may rewrite the very text a tape waits for. The
/// screen is read redacted, so the wait holds on the text as redacted: the
/// rule changes what is written, not whether the tape plays.
#[test]
fn a_wait_holds_on_text_a_redaction_rule_rewrote() {
    let src = "Spawn sh -c 'echo hello from anina; sleep 2'\nWait \"hello from anina\"\nGone \"never here\" 1\n";
    let tape = Tape::parse_str(src, "redact.tape", &HashMap::new()).unwrap();
    let mut redactor = Redactor::default();
    redactor.add("config rule", "anina", Some("XXXXX")).unwrap();
    let out_dir = std::env::temp_dir().join(format!("anina-redact-{}", std::process::id()));
    let opts = Options { out_dir, quiet: true, redactor: Some(redactor), ..Options::default() };
    let cast = record(&tape, Config::default(), opts).unwrap().cast;
    let shown = cast.screens.concat();
    assert!(shown.contains("hello from XXXXX") && !shown.contains("anina"), "{shown}");
}
