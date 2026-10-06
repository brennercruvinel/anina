# Contributing to anina

Thanks for helping. anina drives real programs in a real terminal, so changes
are checked by running real programs: the presets are the test suite.

## Build and check

```sh
cargo build
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo run -- doctor        # can anina drive a terminal here?
cargo run -- run shell     # a whole journey, no network, a few seconds
```

CI runs the same on macOS and Linux.

## Where things live

| Module | Responsibility |
| --- | --- |
| `src/screen.rs`, `src/ansi.rs` | The cell grid every other module reads; ANSI in and out |
| `src/engine/` | Running a program in a pseudo terminal (`tui-test` in process, `tmux`) |
| `src/tape/` | The tape language (anina's dialect and vhs's) |
| `src/record/` | Playing a tape and recording a cast |
| `src/cast.rs` | The recording format; asciicast v3 export |
| `src/render/` | Animated SVG, PNG screenshots, themes, galleries |
| `src/volatile.rs`, `src/privacy.rs`, `src/sandbox/`, `src/teach.rs` | Stable, safe and self-explaining runs |
| `src/explore/`, `src/init/`, `src/audit/`, `src/brand.rs`, `src/preset/` | The commands built on top |
| `presets/` | Journeys for specific programs: the only place a program is named |
| `docs/adr/` | Why things are the way they are |

## Rules of thumb

- The core never names a program. If you need program knowledge, it belongs
  in a preset (ADR-0007).
- Wait on the screen, never on the clock (ADR-0001).
- A new decision or a lesson learned the hard way gets an ADR
  (`docs/adr/TEMPLATE.md`).
- Do not commit generated runs (`anina-out/`).

## Adding a preset

1. `presets/<name>/preset.toml` with the requirements, variables, sandbox mode
   and parts.
2. One tape per part. Write it by using the program: `anina init -o part.tape -- <program>`.
3. Run it end to end: `anina run presets/<name>`, and look at the screenshots.
