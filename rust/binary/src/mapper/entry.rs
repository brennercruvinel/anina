//! `anina explore`: the command's arguments, the sandbox the program runs
//! in, and the files the map is written to.

use super::*;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Output directory (default: anina-out/explore-<program>)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Terminal engine: tui-test (default) or tmux
    #[arg(long, default_value = "tui-test")]
    pub engine: String,
    #[arg(long, default_value_t = 100)]
    pub cols: u16,
    #[arg(long, default_value_t = 30)]
    pub rows: u16,
    /// Working directory of the program
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Environment for the program, K=V (repeatable)
    #[arg(long = "env", value_name = "K=V")]
    pub env: Vec<String>,
    /// Start from an empty environment (PATH kept) plus --env
    #[arg(long)]
    pub clean_env: bool,
    /// Stop after this many screens
    #[arg(long, default_value_t = 60)]
    pub max_states: usize,
    /// Do not go deeper than this many keys from the start
    #[arg(long, default_value_t = 6)]
    pub max_depth: usize,
    /// Stop after this many seconds
    #[arg(long, default_value_t = 300)]
    pub max_time: u64,
    /// More keys to try on every screen (comma separated)
    #[arg(long, value_delimiter = ',')]
    pub keys: Vec<String>,
    /// Keys never to press (comma separated)
    #[arg(long, value_delimiter = ',')]
    pub avoid: Vec<String>,
    /// Text to type when a screen shows a cursor (an input)
    #[arg(long = "type")]
    pub type_text: Option<String>,
    /// Also press keys whose label sounds destructive (install, delete, save...)
    #[arg(long)]
    pub allow_risky: bool,
    /// A shell command run before every start of the program (reset the
    /// state it keeps between runs, e.g. `rm -rf $HOME/.cache/app`). In a
    /// sandbox, HOME is the sandbox's
    #[arg(long, value_name = "CMD")]
    pub before: Option<String>,
    /// Write path.tape: the shortest way to the first screen showing TEXT
    #[arg(long, value_name = "TEXT")]
    pub to: Option<String>,
    /// Sandbox mode: off (default, your environment), env (a fresh HOME and
    /// environment), strict (env inside the OS sandbox)
    #[arg(long, default_value = "off")]
    pub sandbox: String,
    /// Less output
    #[arg(short, long)]
    pub quiet: bool,
    /// The program and its arguments
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

pub fn run(a: Args, scope: &Scope) -> Result<()> {
    let program = a.command[0].clone();
    let base =
        std::path::Path::new(&program).file_name().map_or("program".into(), |s| s.to_string_lossy().into_owned());
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("anina-out").join(format!("explore-{base}")));
    scope.writes_to(&out);
    let mode: Mode = a.sandbox.parse()?;
    // like `anina run`: in the cache, emptied for every exploration
    let sb = match mode {
        Mode::Off => None,
        _ => {
            let root = anina_record::cordon::place(&anina_record::preset::cache_dir(), &format!("explore-{base}"))?;
            if root.exists() {
                std::fs::remove_dir_all(&root)?;
            }
            Some(Sandbox::new(&root, mode)?)
        }
    };
    let mut env = sb.as_ref().map(|s| s.env()).unwrap_or_default();
    for kv in &a.env {
        let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("--env takes K=V, got {kv:?}"))?;
        env.retain(|(n, _)| n != k);
        env.push((k.to_string(), v.to_string()));
    }
    let clean_env = a.clean_env || sb.is_some();
    let cwd = a.cwd.clone().or_else(|| sb.as_ref().map(|s| s.work.clone()));
    if clean_env && !env.iter().any(|(k, _)| k == "PATH") {
        env.push(("PATH".into(), std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into())));
    }
    let launch = Launch {
        program: program.clone(),
        args: a.command[1..].to_vec(),
        cwd: cwd.clone(),
        env: env.clone(),
        clean_env,
        cols: a.cols,
        rows: a.rows,
        before: a.before.clone(),
    };
    // no preset here to declare a network: under strict there is none
    let wrap = sb.as_ref().map(|s| s.wrap()).unwrap_or_default();
    let argv = wrap.argv(&a.command);
    let mut spec = Spec::new(argv[0].clone(), argv[1..].to_vec());
    spec.cols = a.cols;
    spec.rows = a.rows;
    spec.cwd = cwd;
    for (k, v) in env {
        spec.set_env(k, v);
    }
    spec.clean_env = clean_env;
    let engine_name = a.engine.clone();
    let before = a.before.clone();
    // `--before` resets what the program keeps between starts, and in a
    // sandbox the program keeps it there: the line gets the sandbox's HOME
    // and variables, and starts where the program does
    let in_sandbox: Option<(std::collections::HashMap<String, String>, PathBuf)> =
        sb.as_ref().map(|s| (s.env().into_iter().collect(), s.work.clone()));
    // every screen is redacted as it is read: titles, the graph, the map and
    // the tape to a screen all see what may be written
    let mut red = scope.redactor().blank();
    for (p, w) in sb.as_ref().map(|s| s.redactions()).unwrap_or_default() {
        red.literal(&p, &w);
    }
    red.rules.extend(scope.redactor().rules.iter().cloned());
    let make: Factory = Box::new(move || {
        if let Some(cmd) = &before {
            let line = wrap.sh(&[], cmd);
            match &in_sandbox {
                Some((vars, work)) => line.envs(vars).cwd(work).run("--before")?,
                None => line.run("--before")?,
            }
        }
        let mut e: Box<dyn Engine> = Box::new(Redacted::new(engine::by_name(&engine_name)?, red.clone()));
        e.spawn(&spec)?;
        Ok(e)
    });
    let set = Settings {
        max_states: a.max_states,
        max_depth: a.max_depth,
        max_time: Duration::from_secs(a.max_time),
        extra_keys: a.keys.clone(),
        avoid: a.avoid.clone(),
        type_text: a.type_text.clone(),
        allow_risky: a.allow_risky,
        verbose: !a.quiet,
        ..Default::default()
    };
    let g = Explorer::new(make, launch, set).run()?;
    let pngs = g.write(&out, scope.redactor())?;
    let exits = g.edges.iter().filter(|e| e.to.is_none()).count();
    eprintln!(
        "anina explore: {} screens, {} transitions, {} ways out, {} skipped in {:.0} s ({} restarts){}",
        g.states.len(),
        g.edges.len() - exits,
        exits,
        g.skipped.len(),
        g.seconds,
        g.restarts,
        if g.complete { "" } else { "; stopped by a budget" }
    );
    if let Some(r) = scope.redactor().summary() {
        eprintln!("anina explore: redacted {r}");
    }
    eprintln!("anina explore: map -> {}", anina_screen::lesson::shown(&out.join("map.md")));
    if pngs == 0 {
        eprintln!("anina explore: no PNG renderer here; screens saved as .txt and .ansi");
    }
    if let Some(want) = &a.to {
        let target = g.find(want).ok_or_else(|| anyhow!("no screen found showing {want:?}"))?;
        let tape = g
            .tape_to(target, want)
            .ok_or_else(|| anyhow!("s{target} shows {want:?} but no recorded way leads there"))?;
        let p = out.join("path.tape");
        std::fs::write(&p, tape)?;
        eprintln!("anina explore: s{target} shows {want:?}; the way there -> {}", anina_screen::lesson::shown(&p));
    }
    Ok(())
}
