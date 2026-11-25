# Contributing

How a change gets into anina. [`ARCHITECTURE.md`](ARCHITECTURE.md) shows how the parts fit; the reasons are in [`docs/adr/`](docs/adr).

## Who writes the commits

People and AI agents. Each author, human or not, commits under an account of its own and signs its commits; nobody commits as someone else. The history says who wrote each line, and that is all the distinction makes: an agent's commit passes the same gates and the same review as a person's.

## One task, one branch, one pull request

- A task starts as an issue that says what will be true when it is done, and where that is seen working.
- Everything for the task goes in one branch and one pull request, whoever writes it. Work is not split by author.
- Review happens in the pull request. Authors review each other's, people and agents alike.
- The maintainer merges into `main`.
- A release is tagged from a `release-*` branch (`release.toml`).

## The gates

One script holds them, and CI runs the same script:

```sh
tool/tasks/fullcheck.sh
```

| gate | what it holds |
| --- | --- |
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `cargo test --workspace` |
| `doctor` | `anina doctor`: the engines drive a terminal here |
| `selfcheck` | anina on its own output: the `shell` preset against the run kept in `tool/tests/shell/<system>` |
| `deny` | `cargo deny check`: advisories, licenses, sources |
| `package` | `cargo package --workspace --locked` |

A gate is added in `tool/tasks/fullcheck.sh` and nowhere else. CI adds what a developer's machine does not run every time: the oldest Rust the manifest promises (`rust-version` in `Cargo.toml`), the release profile, and the coverage summary.

`cargo deny` comes from `cargo install cargo-deny`. The strict sandbox tests need `sandbox-exec` (it ships with macOS) or `bwrap` (the `bubblewrap` package); without the tool they say so and pass on your machine, and fail in CI.

## What a pull request brings

- **What changes**, in one or two lines, and one place where it is seen working.
- **An ADR** when it makes or changes a decision: the next number, `docs/adr/TEMPLATE.md`, a row in the table of `docs/adr/README.md`. A decision that moves gets a dated follow-up in its ADR, with the original text left as it was.
- **A line in `docs/CHANGELOG`** when a user sees the change.
- **The reference run**, when what anina records changes on purpose, for both systems in the same pull request: `tool/tasks/selfcheck.sh --update` on the system you have, and for the other the run `gatecheck` uploads as the artifact `selfcheck-<system>`, kept with `tool/tasks/selfcheck.sh --keep SYSTEM RUN`.
- **A line saying why**, for a new dependency.

## The code

- A source file stays at or under 639 lines. One that outgrows it gives a part of itself to a module of its own.
- A crate depends only on the layers below it, and only the binary parses flags or prints: a library takes a plain `Request` and says its lines to a `Notice` ([ADR-0013](docs/adr/0013-a-workspace-of-layered-crates.md)).
- Names follow the lexicon: a name anina coins is one word of its siblings' length; a name the world gave stays as given ([ADR-0010](docs/adr/0010-equal-length-siblings-and-a-shared-lexicon.md)). A coined name a user knows by another word gets its entry in `docs/TERMS.toml`.
- Nothing about one program goes in the crates; it goes in the program's preset in `demo/` ([ADR-0007](docs/adr/0007-nothing-about-one-program-in-the-core.md)).
- A test asserts what the code should do, from its contract. A commit message is one sentence saying what is now true.

## Security

A problem with redaction or with the sandbox is reported as [`SECURITY.md`](SECURITY.md) says, not in a public issue.
