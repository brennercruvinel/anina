# Architecture decision records

Why anina is built the way it is. `kind: decision` records a choice between options; `kind: lesson` records something a run taught by failing. Most of them come from tuicli, the shell-script predecessor that drove one TUI end to end.

| ADR | Kind | Title |
| --- | --- | --- |
| [0001](0001-read-the-screen-not-the-clock.md) | lesson | Wait for what the screen shows, not for the clock |
| [0002](0002-an-in-process-terminal-with-tmux-as-fallback.md) | decision | Run programs in an in-process terminal emulator (tui-test), with tmux as a fallback |
| [0003](0003-a-single-rust-binary.md) | decision | Ship anina as a single Rust binary |
| [0004](0004-one-tape-language-that-reads-vhs.md) | decision | One tape language that also reads vhs tapes |
| [0005](0005-animated-svg-with-an-embedded-font.md) | decision | Animate as a self-contained SVG with an embedded font subset |
| [0006](0006-isolate-every-run-and-redact-before-writing.md) | decision | Isolate every run and redact before anything is written |
| [0007](0007-nothing-about-one-program-in-the-core.md) | decision | Nothing about any one program in the core; programs live in presets |
| [0008](0008-mask-what-changes-by-itself.md) | decision | Mask the cells that change by themselves |
| [0009](0009-one-folder-per-run-with-stable-names.md) | decision | One output folder per run, with stable screen names |

A new ADR takes the next number, follows `TEMPLATE.md` and joins this table.
