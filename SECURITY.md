# Security

anina runs programs and writes down what they show. Two things stand between a run and you: redaction, which keeps your name out of what is written, and the sandbox, which keeps a run away from your files. This page says what each one holds, what it does not, and where the code is. It promises nothing the code does not do.

## What redaction holds

A screen is redacted before anything is made from it: a cast, a screenshot, an SVG, a wait in a tape. `run`, `record`, `audit`, `explore` and `init` all treat the screens they read that way. The strings of `run.json`, `report.json` and `graph.json` are redacted as they are written, and so is the error a command ends with.

What is replaced, found when anina starts:

| what | from | becomes |
| --- | --- | --- |
| home path | `$HOME` | `~` |
| your own temp folder | `$TMPDIR`, unless it is the shared `/tmp` or `/var/tmp` (on macOS it is `/var/folders/<id>/T`) | `$TMPDIR` |
| user name | `$USER`, `$LOGNAME`, `id -un` | `user` |
| host name | the system's, whole and up to the first dot | `host` |
| git identity | `git config --global` `user.name` and `user.email` | `Your Name`, `you@example.com` |
| token shapes | GitHub (`ghp_`, `github_pat_`...), `sk-` keys, AWS key ids (`AKIA`, `ASIA`), Slack (`xox?-`), JWTs, and `token=`, `secret:`, `password=`, `api_key=` and the like followed by 12 or more characters | the prefix and `••••••••` |
| the sandbox's paths | the root and the HOME under it | `/sandbox`, `~` |
| your rules | `[redact]` in `./anina.toml` and `~/.config/anina/config.toml`; a preset's `[[redact]]`; `anina run --redact` | what the rule says |

A replacement keeps the grid: the columns after it stay where the program drew them.

`run`, `record`, `audit` and `explore` end with `anina check` on the files they wrote, whether they worked or failed, and the command fails if it finds something. The check reads every text file, casts inside their gzip, and a PNG through the text of the screen anina stores in it.

Code: `rust/runner/src/redact.rs`, `rust/binary/src/checks.rs`.

## What the sandbox holds

There are three modes. `anina run` and `anina audit --preset` use the preset's, `env` when it names none, and `--sandbox` wins over it. `anina explore` uses `off` unless `--sandbox` says otherwise. `record`, `init` and `audit` without `--preset` have no sandbox: they run as `off`.

| mode | the program | `Exec`, `prepare`, `explore --before` |
| --- | --- | --- |
| `off` | your environment | your environment |
| `env` | a HOME, temp and XDG folders of its own under the sandbox root, an environment built from scratch, an empty working folder | your environment |
| `strict` | `env`, inside the OS sandbox | your environment, inside the OS sandbox |

`env` gives a program no reason to touch your files. It does not stop one: nothing refuses a write. The environment built from scratch still carries your `PATH` after the sandbox's own, so the program finds the tools your machine has.

The sandbox root lives in anina's cache and starts empty on every run, unless `anina run --keep`. `anina run` lists what its parts created, changed and deleted under the root. What `prepare` wrote, and anything outside the root, is not in that list.

### `strict`, by system

Under `strict`, everything a run starts goes through the OS sandbox: the program, a tape's `Exec`, a preset's `prepare`, `explore --before`. When one of those lines fails, the error says what `strict` allows, and that the sandbox refused when the line's own output shows it.

| | macOS, `sandbox-exec` | Linux, `bwrap` |
| --- | --- | --- |
| writes | refused, except under the root, `/dev` and `/private/var/folders` (your real per-user temp and cache folders, not only the sandbox's), and on the entry `/private/tmp` itself, not on what is in it | the whole file system is read-only, except the root, a new `/dev` and an empty `/tmp` of its own |
| the root itself | cannot be removed or renamed from inside | is a mount point |
| `prepare` | may also write anina's cache | may also write anina's cache |
| network | refused, the loopback and name lookups included; a socket file under the root still works | a network namespace of its own: the machine's loopback and every other address are out of reach |
| reads | everything you can read | everything you can read |
| processes | not isolated | it sees only its own, and dies with anina |

The network is off unless the preset says `network = true`. `explore` has no preset, so `explore --sandbox strict` has no network; explore a program that needs one under `env`.

On macOS the profile starts from `(allow default)` and takes away two things, writes and the network. Everything else stays as it is for any program you run. On Linux the argv is the one in `rust/record/src/cordon/linux.rs`, no more: no system call filter, no IPC namespace of its own, and socket files that exist on disk (a session bus, a Docker socket) are not hidden.

Code: `rust/record/src/cordon/` (`mod.rs` the modes and the environment, `macos.rs` the profile, `linux.rs` the argv, `shell.rs` the shell lines), `Stage::new` in `rust/record/src/preset/mod.rs`.

## What neither does

- **A tape is code, and so is a preset.** `Exec` runs a shell line, `Spawn` runs any program, `prepare` runs shell before the parts, and a tape can set its own environment and working folder. A tape from a stranger is a script from a stranger: read it before you run it. Under `off` and `env` nothing holds it.
- **`strict` is not a jail for a program that wants out.** It stops a program that writes where it should not, an installer above all. It does not hide your files: under `strict` with `network = true`, a run can read what you can read and send it. `Exec`, `prepare` and `--before` also see your environment, tokens in variables included, and share your terminal.
- **`strict` needs the system's tool.** That is `sandbox-exec`, which macOS documents as deprecated, or `bwrap`, which has to be installed. Without it `--sandbox strict` stops with an error; it never falls back to `env`. Windows is untested.
- **Redaction matches patterns.** It hides what it knows: the names above and those token shapes. A secret of another shape, a name in another case, a project name, an address, another person's e-mail pass through unless a rule of yours names them. Only the global git identity is known, not a repository's own.
- **Not every file passes through redaction.** `changes.json` (the paths a run changed in the sandbox) and `journey.tape` (the command you gave `audit`) are written as they are. The check at the end reads them like the rest.
- **The check reads text.** It skips binary files, PNGs that carry no text and embedded images (`data:` URIs), and counts the first two in its summary. It looks for what the built-in rules and `[redact]` hide; a preset's `[[redact]]` and `--redact` apply while recording and are not checked again. `init` has no check at the end.
- **`anina init` writes down what you type.** A password typed while it records is in the tape.
- **What `Exec` and `prepare` print goes to your terminal as it is.** It is not recorded, and it is not redacted.

## Reporting a problem

If anina wrote something it should have hidden, or a run under `strict` did something this page says it cannot, write to [brenner@hoffresearch.com](mailto:brenner@hoffresearch.com), the maintainer's address in [`docs/CODE_OF_CONDUCT.md`](docs/CODE_OF_CONDUCT.md). Please do not open a public issue for it. Say what you ran, on which system, and what you expected to be hidden or refused.
