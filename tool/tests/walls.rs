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
fn the_root_is_not_the_program_s_to_replace() {
    if !walls() {
        return;
    }
    let dir = scratch("replace");
    let strict = sandbox(&dir, Mode::Strict);
    let root = quoted(&strict.root.display().to_string());
    let target = dir.join("elsewhere");
    std::fs::create_dir_all(&target).unwrap();

    // a link in the root's place would send anina, which is not confined, wherever it points
    let _ = program(
        &strict,
        &format!("rm -rf {root}; ln -s {} {root}; mv {root} {root}.gone", quoted(&target.display().to_string())),
    );
    let md = std::fs::symlink_metadata(&strict.root).unwrap();
    assert!(md.is_dir(), "the root was replaced");

    // and a root that already is a link is not taken as one
    let link = dir.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let e = Sandbox::new(&link, Mode::Env).unwrap_err().to_string();
    assert!(e.ends_with("is a symbolic link; a sandbox root is a folder of its own"), "{e}");
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
    let cached = "[[prepare]]\nabout = \"keep a build\"\nrun = \"mkdir -p \\\"$ANINA_CACHE/build\\\" && \
                  echo built > \\\"$ANINA_CACHE/kept.txt\\\"\"\n";
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

/// How it ends when a `prepare` moves the folder that holds the root: what
/// that run says, and the next one. `sandbox-exec` refuses the move, both
/// times. Under `bwrap` the root is a mount point and the folder that holds
/// it is not: the move works, anina finds the link when `prepare` is done,
/// and the next run finds it before it starts.
#[cfg(target_os = "macos")]
const MOVED: [&str; 2] = ["the sandbox refused it"; 2];
#[cfg(target_os = "linux")]
const MOVED: [&str; 2] = FOUND;
const FOUND: [&str; 2] =
    ["prepare moved the sandbox root or put a link in its path", "a symbolic link or the name leads elsewhere"];

#[test]
fn anina_run_follows_no_link_that_prepare_left_in_the_cache() {
    if !walls() {
        return;
    }
    let elsewhere = |dir: &Path| format!("ELSEWHERE={}", dir.join("elsewhere").display());
    // the cache is prepare's to write and the root lives in it: move the
    // folder that holds the root, and a link fits in its place
    let swap = "[[prepare]]\nabout = \"swap\"\nrun = 'mv \"$ANINA_CACHE/sandbox\" \"$ANINA_CACHE/gone\" && \
                ln -s \"$ELSEWHERE\" \"$ANINA_CACHE/sandbox\"'\n";
    // under strict, then under env, where nothing refuses the move
    for (name, mode, whys) in [("moved", "strict", MOVED), ("moved-env", "env", FOUND)] {
        let dir = scratch(name);
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        let p = preset(&dir, swap, PROGRAM);
        // twice: a link that stayed is there when the next run makes its root
        for why in whys {
            let o = run(&dir, &p, &["--sandbox", mode, "--var", &elsewhere(&dir)]);
            let said = stderr(&o);
            // anina is not confined: through the link it would make its folders out there
            let made = std::fs::read_dir(dir.join("elsewhere")).unwrap().count();
            assert_eq!(made, 0, "{mode}: anina went through the link: {said}");
            assert!(!o.status.success(), "{mode}: {said}");
            assert!(said.contains(why), "{mode}: {said}");
        }
    }

    // the copy anina keeps beside the sandboxes, and removes on its way out:
    // through a link there, the folder of that name out there would go
    let dir = scratch("saved");
    let mine = dir.join("elsewhere/walls/mine.txt");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, "mine").unwrap();
    let link = "[[prepare]]\nabout = \"link\"\nrun = 'ln -s \"$ELSEWHERE\" \"$ANINA_CACHE/pristine\"'\n";
    let o = run(&dir, &preset(&dir, link, PROGRAM), &["--sandbox", "strict", "--var", &elsewhere(&dir)]);
    let said = stderr(&o);
    assert!(!o.status.success(), "{said}");
    assert!(said.contains("prepare put a link in the path of"), "{said}");
    assert!(mine.exists(), "anina removed a folder through the link: {said}");

    // and a name that leaves the cache is no sandbox's place
    for name in ["../../elsewhere/walls", &dir.join("elsewhere/walls").display().to_string(), ""] {
        let e = anina_record::cordon::place(&dir.join("cache"), name).unwrap_err().to_string();
        assert!(e.ends_with(FOUND[1]), "{name:?}: {e}");
    }
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

    // the program alone, in the terminal anina gives it: it never gets to say so
    let alone = tape.split_once('\n').unwrap().1;
    let p = preset(&dir, "", alone);
    let o = run(&dir, &p, &["--sandbox", "strict"]);
    let said = stderr(&o);
    assert!(!o.status.success(), "{said}");
    assert!(said.contains("part.tape:2:"), "{said}");

    // declared: prepare, the Exec and the program all reach the listener
    let p = preset(&dir, &format!("network = true\n{prepare}"), &tape);
    let o = run(&dir, &p, &["--sandbox", "strict"]);
    assert!(o.status.success(), "{}", stderr(&o));
}

/// A built-in preset is emptied and extracted under `CACHE/presets` on every
/// run, and a `prepare` writes the cache. Through a link it left there, the
/// folder of the preset's name out there would be emptied and filled.
#[test]
fn a_built_in_preset_is_not_extracted_through_a_link_in_the_cache() {
    let dir = scratch("presets");
    let mine = dir.join("elsewhere/shell/mine.txt");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, "mine").unwrap();
    std::fs::create_dir_all(dir.join("cache")).unwrap();
    std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("cache/presets")).unwrap();

    // from a folder with no `demo/` and a HOME with no presets: `shell` is the built-in one
    let o = Command::new(env!("CARGO_BIN_EXE_anina"))
        .args(["run", "shell", "-o"])
        .arg(dir.join("out"))
        .current_dir(&dir)
        .env("HOME", &dir)
        .env_remove("ANINA_PRESETS")
        .env("ANINA_CACHE", dir.join("cache"))
        .env("ANINA_CONFIG", "/dev/null")
        .output()
        .unwrap();
    let said = stderr(&o);
    assert!(mine.exists(), "anina emptied a folder through the link: {said}");
    assert!(!dir.join("elsewhere/shell/preset.toml").exists(), "anina extracted through the link: {said}");
    assert!(!o.status.success(), "{said}");
    assert!(said.contains(FOUND[1]), "{said}");
}

/// `explore --before` resets what the program keeps between starts, and in a
/// sandbox the program keeps it under the sandbox's HOME: that is the HOME
/// the line gets, not the one of whoever runs anina.
#[test]
fn explore_runs_before_with_the_home_of_the_sandbox() {
    let dir = scratch("before");
    let mark = dir.join("home.txt");
    let o = Command::new(env!("CARGO_BIN_EXE_anina"))
        .args(["explore", "--sandbox", "env", "--max-depth", "0", "--quiet", "-o"])
        .arg(dir.join("out"))
        .args(["--before", "printf %s \"$HOME\" > \"$MARK\"", "--", "sh", "-c", "echo ready; sleep 5"])
        .env("MARK", &mark)
        .env("ANINA_CACHE", dir.join("cache"))
        .env("ANINA_CONFIG", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", stderr(&o));
    let home = std::fs::read_to_string(&mark).unwrap();
    assert!(home.ends_with("/cache/sandbox/explore-sh/home"), "--before ran with HOME={home}");
    assert_ne!(Some(home), std::env::var("HOME").ok());
}
