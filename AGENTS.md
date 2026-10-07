# AGENTS.md

anina drives, maps, audits and animates terminal programs. One binary, built from a workspace of layered crates (docs/adr/0013).

## Layout

| path | package | what it holds |
| --- | --- | --- |
| `rust/screen` | `anina-screen` | the cell grid, escape parsing, recorded frames, wait-failure lessons |
| `rust/runner` | `anina-runner` | engines (tui-test, tmux), the tape language, masking, redaction |
| `rust/render` | `anina-render` | animated SVG, PNG, galleries, themes, brand |
| `rust/record` | `anina-record` | recording, presets, the sandbox (`cordon`) |
| `rust/binary` | `anina` | the CLI: `audits`, `mapper`, `taping`, `checks` |
| `rust/codecs`, `design`, `motion`, `recode` | `anina-*` | empty, reserved |
| `data/theme` | | default theme and the embedded font |
| `demo/` | | built-in presets, embedded in `anina-record` |
| `docs/adr` | | why things are the way they are |

## Rules

- A crate depends only on the layers below it: screen, then runner and render, then record, then the binary. Never the other way.
- Paths inside a crate use `crate::`; across crates, `anina_<name>::`.
- `rust/render/assets` and `rust/record/demo` are symlinks to `data/theme` and `demo/`. Keep them: `cargo package` needs the files inside the crate.
- A dependency goes in `[workspace.dependencies]` first, then `name.workspace = true` in the crate that uses it.
- Nothing about one program goes in the core; programs live in presets (ADR-0007).

## Before a pull request

`tool/tasks/fullcheck.sh` runs every gate below, in order:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p anina -- doctor
cargo deny check
cargo package --workspace
```
