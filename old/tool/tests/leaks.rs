//! What anina writes never shows this machine: a program that prints the
//! home path, the user and the host is recorded and explored, and
//! `anina check` must find nothing in either output.

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
