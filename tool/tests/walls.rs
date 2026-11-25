//! The walls of `--sandbox strict`, tried from inside: a tape's `Exec`, a
//! preset's `prepare` and the program itself write outside the root and
//! reach for the network.
//!
//! The same tests run on macOS (`sandbox-exec`) and on Linux (`bwrap`); the
//! two things that differ are named below. Nothing here leaves the machine:
//! the network is a listener this test opens on the loopback.
//!
//! Every folder is under cargo's target dir. The temp dir would hide a
//! missing wall: macOS's is writable inside the sandbox, and Linux's /tmp is
//! replaced by an empty one, where a write works and is thrown away.
#![cfg(any(target_os = "macos", target_os = "linux"))]

use anina_record::cordon::{Mode, Sandbox};
use anina_record::{Config, Options, record};
use anina_runner::script::{Line, Step, Tape};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(target_os = "macos")]
const TOOL: &str = "sandbox-exec";
#[cfg(target_os = "linux")]
const TOOL: &str = "bwrap";

/// How a failed connection reads. `sandbox-exec` answers `EPERM`, which
/// anina reports as a refusal. In `bwrap`'s own network namespace nothing
/// listens, which looks like any failed connection: the plain note.
#[cfg(target_os = "macos")]
const NO_NETWORK: &str = "the sandbox refused it";
#[cfg(target_os = "linux")]
const NO_NETWORK: &str = "it ran confined";

/// Whether the OS sandbox is here to be tried. Without it a test says so and
/// passes on a developer's machine; in CI it fails, where a wall nobody
/// tried would read as a wall that held.
fn walls() -> bool {
    if anina_record::which(TOOL).is_some() || Path::new("/usr/bin").join(TOOL).exists() {
        return true;
    }
    assert!(std::env::var_os("CI").is_none(), "{TOOL} is missing: the strict sandbox was not tested");
    eprintln!("skipped: no {TOOL} here, the strict sandbox was not tested");
    false
}

/// An empty folder for one test, as the system names it.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("walls-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(d).unwrap()
}

/// A sandbox under `dir`, as a preset that declares nothing gets it.
fn sandbox(dir: &Path, mode: Mode) -> Sandbox {
    Sandbox::new(&dir.join("root"), mode).unwrap()
}

/// `text` between single quotes, for `sh`.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `sh` writing one line to `file`.
fn write_to(file: &Path) -> String {
    format!("echo written > {}", quoted(&file.display().to_string()))
}

/// `sh` opening a connection to this machine's `port`. bash does it by
/// itself, and both systems have it.
fn connect_to(port: u16) -> String {
    format!("bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port}'")
}

/// Play a tape of `Exec` lines the way a part of `anina run` is played.
fn exec(sb: &Sandbox, script: &str) -> anyhow::Result<()> {
    let file = PathBuf::from("walls.tape");
    let lines = vec![Line { file: file.clone(), n: 1, step: Step::Exec(script.to_string()) }];
    let tape = Tape { path: file, lines, warnings: Vec::new() };
    let cfg = Config { wrap: sb.wrap(), ..Config::default() };
    let opts = Options { out_dir: sb.root.join("out"), quiet: true, ..Options::default() };
    record(&tape, cfg, opts).map(|_| ())
}

/// Start `sh -c script` the way a tape's `Spawn` starts the program: behind
/// the same prefix.
fn program(sb: &Sandbox, script: &str) -> bool {
    let argv = sb.wrap().argv(&["sh", "-c", script]);
    Command::new(&argv[0]).args(&argv[1..]).output().unwrap().status.success()
}

#[test]
fn an_exec_writes_inside_the_root_and_nowhere_else() {
    if !walls() {
        return;
    }
    let dir = scratch("exec");
    let (inside, outside) = (dir.join("root/work/in.txt"), dir.join("out.txt"));

    let strict = sandbox(&dir, Mode::Strict);
    exec(&strict, &write_to(&inside)).unwrap();
    assert!(inside.exists());
    let e = exec(&strict, &write_to(&outside)).unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains("Exec failed"), "{said}");
    assert!(
        said.contains(&format!(
            "the sandbox refused it: --sandbox strict allows writes only under the sandbox root ({})",
            strict.root.display()
        )),
        "{said}"
    );
    assert!(!outside.exists(), "the write went through");

    // the same line without the OS sandbox: what strict stopped was a write that works
    exec(&sandbox(&dir, Mode::Env), &write_to(&outside)).unwrap();
    assert!(outside.exists());
}

#[test]
fn the_program_writes_inside_the_root_and_nowhere_else() {
    if !walls() {
        return;
    }
    let dir = scratch("program");
    let (inside, outside) = (dir.join("root/home/in.txt"), dir.join("out.txt"));

    let strict = sandbox(&dir, Mode::Strict);
    assert!(program(&strict, &write_to(&inside)));
    assert!(inside.exists());
    assert!(!program(&strict, &write_to(&outside)));
    assert!(!outside.exists(), "the write went through");

    assert!(program(&sandbox(&dir, Mode::Env), &write_to(&outside)));
    assert!(outside.exists());
}

#[test]
fn no_network_under_strict_unless_it_is_declared() {
    if !walls() {
        return;
    }
    let dir = scratch("network");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connect = connect_to(listener.local_addr().unwrap().port());

    // the listener answers: without the OS sandbox the connection opens
    assert!(program(&sandbox(&dir, Mode::Env), &connect));

    let closed = sandbox(&dir, Mode::Strict);
    assert!(!program(&closed, &connect), "the program reached the network");
    let e = exec(&closed, &connect).unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains(NO_NETWORK), "{said}");
    assert!(said.contains("the network is off (a preset turns it on with network = true)"), "{said}");

    let open = Sandbox { network: true, ..sandbox(&dir, Mode::Strict) };
    assert!(program(&open, &connect), "network = true did not open the network");
    exec(&open, &connect).unwrap();
}

#[test]
fn a_root_with_a_quote_and_a_space_in_its_name_holds_the_same() {
    if !walls() {
        return;
    }
    let dir = scratch("names").join("it's a \"root\" (1)");
    std::fs::create_dir_all(&dir).unwrap();
    let (inside, outside) = (dir.join("root/work/in.txt"), dir.join("out.txt"));

    let strict = sandbox(&dir, Mode::Strict);
    assert!(program(&strict, &write_to(&inside)));
    assert!(inside.exists());
    assert!(!program(&strict, &write_to(&outside)));
    assert!(!outside.exists(), "the write went through");
}

/// A preset on disk: `manifest` lines after the name, one part, its tape.
fn preset(dir: &Path, manifest: &str, tape: &str) -> PathBuf {
    let p = dir.join("preset");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("preset.toml"),
        format!("name = \"walls\"\n{manifest}\n[[parts]]\nname = \"part\"\ntape = \"part.tape\"\n"),
    )
    .unwrap();
    std::fs::write(p.join("part.tape"), tape).unwrap();
    p
}

/// `anina run PRESET ARGS`, with its cache and its output under `dir`.
fn run(dir: &Path, preset: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anina"))
        .arg("run")
        .arg(preset)
        .args(args)
        .arg("-o")
        .arg(dir.join("out"))
        .env("ANINA_CACHE", dir.join("cache"))
        // only this test's rules, whatever config the machine has
        .env("ANINA_CONFIG", "/dev/null")
        .output()
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A program that draws one line and stays a moment.
const PROGRAM: &str = "Spawn sh -c 'echo ready; sleep 1'\nWait \"ready\"\n";

#[test]
fn anina_run_says_why_an_exec_failed_under_strict() {
    if !walls() {
        return;
    }
    let dir = scratch("run");
    let outside = dir.join("out.txt");
    let p = preset(&dir, "", &format!("Exec {}\n{PROGRAM}", write_to(&outside)));

    let o = run(&dir, &p, &["--sandbox", "strict"]);
    let said = stderr(&o);
    assert!(!o.status.success(), "{said}");
    assert!(said.contains("part.tape:1: Exec failed"), "{said}");
    // the root reads as anina shows it everywhere: /sandbox
    assert!(
        said.contains("the sandbox refused it: --sandbox strict allows writes only under the sandbox root (/sandbox)"),
        "{said}"
    );
    assert!(!outside.exists(), "the write went through");

    // the preset's own mode (env, by default): the same tape passes
    let o = run(&dir, &p, &[]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(outside.exists());
}

#[test]
fn anina_run_keeps_prepare_inside_the_root_and_the_cache() {
    if !walls() {
        return;
    }
    let dir = scratch("prepare");
    let outside = dir.join("out.txt");
    let kept = dir.join("cache/kept.txt");
    // in the cache, where a preset keeps its clones and builds
    let cached = "[[prepare]]\nabout = \"keep a build\"\nrun = \"echo built > \\\"$ANINA_CACHE/kept.txt\\\"\"\n";
    let escape = "[[prepare]]\nabout = \"write elsewhere\"\nrun = \"echo x > \\\"$ELSEWHERE\\\"\"\n";

    let p = preset(&dir, &format!("{cached}{escape}"), PROGRAM);
    let o = run(&dir, &p, &["--sandbox", "strict", "--var", &format!("ELSEWHERE={}", outside.display())]);
    let said = stderr(&o);
    assert!(!o.status.success(), "{said}");
    assert!(kept.exists(), "prepare could not write the cache: {said}");
    assert!(said.contains("write elsewhere failed"), "{said}");
    assert!(
        said.contains("the sandbox refused it: --sandbox strict allows writes only under the sandbox root ("),
        "{said}"
    );
    assert!(!outside.exists(), "the write went through");

    // the cache is prepare's: the tape's Exec does not write it
    let stolen = dir.join("cache/stolen.txt");
    let p = preset(&dir, cached, &format!("Exec {}\n{PROGRAM}", write_to(&stolen)));
    let o = run(&dir, &p, &["--sandbox", "strict"]);
    assert!(!o.status.success(), "{}", stderr(&o));
    assert!(!stolen.exists(), "an Exec wrote the cache");
}

#[test]
fn anina_run_opens_the_network_for_a_preset_that_declares_it() {
    if !walls() {
        return;
    }
    let dir = scratch("declared");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let prepare = format!("[[prepare]]\nabout = \"fetch\"\nrun = \"{}\"\n", connect_to(port));
    // the program reaches it too, and says so on its screen
    let tape = format!(
        "Exec {}\nSpawn bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port} && echo reached; sleep 1'\nWait \"reached\"\n",
        connect_to(port)
    );

    // no `network = true`: the first thing to reach out is refused
    let p = preset(&dir, &prepare, &tape);
    let o = run(&dir, &p, &["--sandbox", "strict"]);
    let said = stderr(&o);
    assert!(!o.status.success(), "{said}");
    assert!(said.contains("fetch failed"), "{said}");
    assert!(said.contains(NO_NETWORK), "{said}");
    assert!(said.contains("the network is off (a preset turns it on with network = true)"), "{said}");

    // declared: prepare, the Exec and the program all reach the listener
    let p = preset(&dir, &format!("network = true\n{prepare}"), &tape);
    let o = run(&dir, &p, &["--sandbox", "strict"]);
    assert!(o.status.success(), "{}", stderr(&o));
}
