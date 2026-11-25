//! How a command ends, read the way a pipeline reads it: by its exit code.
//! Each kind of failure has its own (the table is in `rust/binary/src/ending.rs`
//! and in the README), and what the command prints stays what it was.

use std::path::PathBuf;
use std::process::Command;

fn anina() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_anina"));
    // only this test's rules, whatever config the machine has
    c.env("ANINA_CONFIG", "/dev/null");
    c
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anina-test-codes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The exit code and the last line of stderr.
fn ends(c: &mut Command) -> (Option<i32>, String) {
    let o = c.output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    (o.status.code(), err.lines().last().unwrap_or_default().to_string())
}

#[test]
fn a_command_line_that_is_not_understood_ends_with_2() {
    let dir = scratch("usage");
    // clap refuses both: a flag it does not know, and an audit of nothing
    assert_eq!(ends(anina().current_dir(&dir).arg("--no-such-flag")).0, Some(2));
    assert_eq!(ends(anina().current_dir(&dir).arg("audit")).0, Some(2));
    // a speed of zero would be an animation that never ends
    for speed in ["0", "-1", "fast"] {
        let (code, said) = ends(anina().current_dir(&dir).args(["render", "x.cast.json", "--speed", speed]));
        assert_eq!(code, Some(2), "--speed {speed}: {said}");
    }
    // a part or a sandbox belongs to a preset: with a command neither is silently dropped
    for flag in [["--part", "one"], ["--sandbox", "strict"]] {
        let (code, said) = ends(anina().current_dir(&dir).arg("audit").args(flag).args(["--", "true"]));
        assert_eq!(
            (code, said.as_str()),
            (Some(2), "anina: --part and --sandbox go with --preset: a tape or a command has neither")
        );
    }
}

#[test]
fn a_failure_of_no_other_kind_ends_with_1() {
    let dir = scratch("other");
    let (code, said) = ends(anina().current_dir(&dir).args(["run", "no-such-preset"]));
    assert_eq!(code, Some(1), "{said}");
    assert!(said.starts_with("anina: no preset \"no-such-preset\"; known: "), "{said}");
}

#[test]
fn a_journey_that_fails_ends_with_3() {
    let dir = scratch("journey");
    std::fs::write(dir.join("bad.tape"), "Exec false\nSpawn sh -c 'echo ready; sleep 1'\nWait \"ready\"\n").unwrap();
    let (code, said) = ends(anina().current_dir(&dir).args(["record", "bad.tape", "-o", "out"]));
    assert_eq!((code, said.as_str()), (Some(3), "anina: bad.tape:1: Exec failed (exit status: 1): false"));

    // a preset whose part fails: the run is written, and it ends the same way
    let preset = dir.join("stops");
    std::fs::create_dir_all(&preset).unwrap();
    std::fs::write(preset.join("preset.toml"), "name = \"stops\"\n[[parts]]\nname = \"one\"\ntape = \"bad.tape\"\n")
        .unwrap();
    std::fs::copy(dir.join("bad.tape"), preset.join("bad.tape")).unwrap();
    let cache = dir.join("cache");
    let (code, said) = ends(anina().current_dir(&dir).env("ANINA_CACHE", &cache).args(["run", "./stops", "-o", "ran"]));
    assert_eq!((code, said.as_str()), (Some(3), "anina: 1 part(s) failed: one"));
    assert!(dir.join("ran/run.json").is_file());

    // an audit whose journey breaks in the baseline terminal scored nothing: it says how
    // many terminals it left, writes its report, and ends as a journey that failed
    std::fs::write(dir.join("never.tape"), "Spawn sh -c 'echo ready; sleep 3'\nWait \"never\" 1\n").unwrap();
    let (code, said) = ends(anina().current_dir(&dir).args(["audit", "never.tape", "--quick", "-o", "audited"]));
    assert_eq!(code, Some(3), "{said}");
    assert!(said.ends_with("3 of 4 terminals were not run (--keep-going runs them)"), "{said}");
    assert!(dir.join("audited/report.json").is_file());
}

#[test]
fn something_to_hide_ends_with_4() {
    let dir = scratch("hidden");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::write(dir.join("out/note.txt"), "token=ghp_abcdefghijklmnopqrstuvwxyz0123456789\n").unwrap();
    let (code, said) = ends(anina().current_dir(&dir).args(["check", "out"]));
    assert_eq!(code, Some(4), "{said}");
    assert!(said.starts_with("anina: 1 thing to hide in 1 file"), "{said}");
}

#[test]
fn two_runs_that_differ_end_with_5_and_two_alike_with_0() {
    let dir = scratch("differ");
    for (folder, text) in [("a", "one\n"), ("b", "two\n"), ("c", "one\n")] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
        std::fs::write(dir.join(folder).join("screen.txt"), text).unwrap();
        // what marks a folder as one anina wrote
        std::fs::write(dir.join(folder).join("run.json"), "{}").unwrap();
    }
    let (code, said) = ends(anina().current_dir(&dir).args(["compare", "a", "b"]));
    assert_eq!(code, Some(5), "{said}");
    assert!(said.starts_with("anina: 1 difference ("), "{said}");
    assert_eq!(ends(anina().current_dir(&dir).args(["compare", "a", "c"])).0, Some(0));
    // a folder anina did not write is an argument that is wrong, not a run that differs
    std::fs::create_dir_all(dir.join("other")).unwrap();
    std::fs::write(dir.join("other/screen.txt"), "one\n").unwrap();
    let (code, said) = ends(anina().current_dir(&dir).args(["compare", "other", "a"]));
    assert_eq!(code, Some(2), "{said}");
    assert!(said.starts_with("anina: other is not a folder anina wrote"), "{said}");
}

/// The same program `leaks` audits: red whatever the terminal announced, so
/// compatibility is 80 and the red on black is under the contrast it needs.
#[test]
fn a_score_under_the_limit_ends_with_6_and_without_the_limit_with_0() {
    let dir = scratch("score");
    let script = "printf '\\033[31mhome=%s\\033[0m\\n' \"$HOME\"\necho ready\n";
    std::fs::write(dir.join("who.sh"), script).unwrap();
    let audit = |more: &[&str], out: &str| {
        let mut c = anina();
        c.current_dir(&dir).args(["audit", "--quick", "-o", out]).args(more).args(["--", "sh", "who.sh"]);
        ends(&mut c)
    };
    let (code, said) = audit(&[], "plain");
    assert_eq!(code, Some(0), "{said}");

    let (code, said) = audit(&["--min-score", "90"], "held");
    assert_eq!(code, Some(6), "{said}");
    assert!(said.starts_with("anina: ") && said.contains(" under 90: ") && said.contains("compatibility 80"), "{said}");
    // the report is written before the limit is held
    assert!(dir.join("held/report.json").is_file());

    // a limit every score reaches changes nothing
    let (code, said) = audit(&["--min-score", "0"], "zero");
    assert_eq!(code, Some(0), "{said}");
    // and a limit that is not a score is a command line not understood
    assert_eq!(audit(&["--min-score", "101"], "over").0, Some(2));
}
