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
| [0010](0010-equal-length-siblings-and-a-shared-lexicon.md) | decision | Equal-length siblings and a shared lexicon |
| [0011](0011-a-design-system-read-from-the-screen.md) | decision | A design system read from the screen |
| [0012](0012-one-timeline-many-writers.md) | decision | One timeline, many writers |
| [0013](0013-a-workspace-of-layered-crates.md) | decision | Split anina into a workspace of layered crates |
| [0014](0014-code-and-cli-speak-to-different-readers.md) | decision | The code and the CLI speak to different readers |
| [0015](0015-waits-that-hold-until-the-program-answered.md) | lesson | A written wait holds until the program answered, in every terminal |
| [0016](0016-put-back-only-what-a-play-changed.md) | decision | Put back only what a play changed in the sandbox |

A new ADR takes the next number, follows `TEMPLATE.md` and joins this table.
