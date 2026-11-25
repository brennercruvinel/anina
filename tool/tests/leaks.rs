//! What anina writes never shows this machine: a program that prints the
//! home path, the user and the host is recorded, explored and audited, and
//! `anina check` must find nothing in any of the outputs.

use std::path::{Path, PathBuf};
use std::process::Command;

fn anina() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_anina"));
    // only this test's rules, whatever config the machine has
    c.env("ANINA_CONFIG", "/dev/null");
    c
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anina-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A program that shows who and where it runs, then waits.
fn identity_script(dir: &Path) -> PathBuf {
    let p = dir.join("who.sh");
    std::fs::write(&p, "echo \"home=$HOME user=$(id -un) host=$(hostname)\"\npwd\necho ready\nread x\n").unwrap();
    p
}

fn check(out: &Path) -> std::process::Output {
    anina().arg("check").arg(out).output().unwrap()
}

#[test]
fn record_writes_nothing_to_hide() {
    let dir = scratch("record");
    let script = identity_script(&dir);
    let tape = dir.join("who.tape");
    std::fs::write(
        &tape,
        format!(
            "Set Cols 100\nSet Rows 8\nSet Cwd {}\nSpawn sh {}\nWait \"ready\"\nSnap who\nKey Enter\n",
            dir.display(),
            script.display()
        ),
    )
    .unwrap();
    let out = dir.join("out");
    let st = anina().arg("record").arg(&tape).arg("-o").arg(&out).output().unwrap();
    assert!(st.status.success(), "record failed: {}", String::from_utf8_lossy(&st.stderr));
    let shot = std::fs::read_to_string(out.join("shots/who.txt")).unwrap();
    assert!(shot.contains("home=~"), "the home path shows: {shot}");
    let c = check(&out);
    assert!(c.status.success(), "check found:\n{}", String::from_utf8_lossy(&c.stdout));
}

#[test]
fn explore_writes_nothing_to_hide() {
    let dir = scratch("explore");
    let script = identity_script(&dir);
    let out = dir.join("out");
    let st = anina()
        .args(["explore", "-q", "--max-time", "6", "--max-depth", "1", "--cwd"])
        .arg(&dir)
        .arg("-o")
        .arg(&out)
        .arg("--")
        .arg("sh")
        .arg(&script)
        .output()
        .unwrap();
    assert!(st.status.success(), "explore failed: {}", String::from_utf8_lossy(&st.stderr));
    let s0 = std::fs::read_to_string(out.join("screens/s0.txt")).unwrap();
    assert!(s0.contains("home=~"), "the home path shows: {s0}");
    let c = check(&out);
    assert!(c.status.success(), "check found:\n{}", String::from_utf8_lossy(&c.stdout));
}

/// The whole command, on a program that says where it runs in red whatever
/// the terminal announced, and leaves with 0: the report scores it, names
/// what it did wrong, and shows nothing of the machine.
#[test]
fn audit_scores_the_program_and_writes_nothing_to_hide() {
    let dir = scratch("audit");
    let script = "printf '\\033[31mhome=%s\\033[0m\\n' \"$HOME\"\necho \"user=$(id -un) host=$(hostname)\"\n";
    std::fs::write(dir.join("who.sh"), script).unwrap();
    let out = dir.join("out");
    let st = anina()
        .current_dir(&dir)
        .args(["audit", "--quick", "-o"])
        .arg(&out)
        .args(["--", "sh", "who.sh"])
        .output()
        .unwrap();
    assert!(st.status.success(), "audit failed: {}", String::from_utf8_lossy(&st.stderr));

    let text = std::fs::read_to_string(out.join("report.json")).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    let score = |category: &str| {
        let scores = report["scores"].as_array().unwrap();
        scores.iter().find(|s| s["category"] == category).map(|s| s["score"].clone()).unwrap()
    };
    // it ran in the four terminals and left each one as it found it
    let terminals = report["variants"].as_array().unwrap();
    let names: Vec<&str> = terminals.iter().filter_map(|t| t["variant"]["name"].as_str()).collect();
    assert_eq!(names, ["120x40-truecolor", "60x20-truecolor", "120x40-nocolor", "120x40-16"]);
    assert_eq!((score("stability"), score("hygiene")), (100.into(), 100.into()), "{text}");
    // red under NO_COLOR is the one check of five it fails; red is one of the 16 colors
    assert_eq!(score("compatibility"), 80, "{text}");
    assert_eq!(terminals[2]["colors_respected"], false);
    assert_eq!(terminals[3]["colors_respected"], true);
    // xterm's red on black is under 4.5:1, and the report shows where, with the home path hidden
    let worst = &terminals[0]["contrast"]["worst"][0];
    assert_eq!(
        (&worst["fg"], &worst["bg"], &worst["sample"]),
        (&"#cd0000".into(), &"#000000".into(), &"home=~".into())
    );

    // the journey it found, kept as a tape: open, settle, wait for the program to leave
    let tape = std::fs::read_to_string(out.join("journey.tape")).unwrap();
    assert!(tape.starts_with("# written by anina audit\nSpawn sh who.sh\nIdle 5\nSleep "), "{tape}");
    assert!(out.join("report.html").is_file());
    let c = check(&out);
    assert!(c.status.success(), "check found:\n{}", String::from_utf8_lossy(&c.stdout));
}

#[test]
fn check_fails_on_a_leak() {
    let dir = scratch("leak");
    let home = std::env::var("HOME").unwrap();
    std::fs::write(dir.join("a.txt"), format!("fine\nopened {home}/notes\n")).unwrap();
    let c = check(&dir);
    assert!(!c.status.success());
    let found = String::from_utf8_lossy(&c.stdout);
    assert!(found.contains("a.txt:2: home path"), "{found}");
}
