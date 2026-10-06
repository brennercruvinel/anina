---
id: ADR-0002
title: Run programs in an in-process terminal emulator (tui-test), with tmux as a fallback
kind: decision
status: accepted
category: ENGINE
date: 2026-10-06
last-updated: 2026-10-06
tags: [engine, tui-test, tmux, pty]
supersedes: []
superseded-by: null
related: [src/engine/mod.rs, src/engine/tuitest.rs, src/engine/tmux.rs]
---

# ADR-0002: Run programs in an in-process terminal emulator (tui-test), with tmux as a fallback

## Context

tuicli drove programs through a private tmux server: `send-keys` to type, `capture-pane -e -N -p` to read. It worked, but every screen read spawned a process (tens of milliseconds), timing came from polling, and tmux sets `TERM` and colors its own way.

In 2026-10, microsoft/tui-test 0.1.0 shipped its core as a Rust crate (`tui-test-rs`, MIT): alacritty_terminal over a native pty, in process, with cells (char, colors, attributes, OSC 8 link), cursor, terminal modes, keys and resize.

## Decision drivers

- Fidelity: truecolor, wide characters, braille, alternate screen, cursor, modes.
- Cheap reads, so the recorder can sample often and waits can check stability.
- One install, macOS and Linux (and Windows later).
- Not locked to one backend.

## Considered options

### Keep tmux only

Proven on 67 screens of one TUI, but slow reads, an external dependency and no terminal modes.

### Write our own emulator binding (alacritty_terminal, libghostty-vt, charm x/vt)

Most control, most work; tui-test already did it and tests it against several emulators.

### tui-test in process behind an `Engine` trait, tmux as a second backend

## Decision

The `Engine` trait (`spawn`, `write`, `key`, `screen`, `modes`, `resize`, `exit_code`, `close`) is the only way the rest of anina touches a terminal. `tui-test` is the default; `tmux` stays as an alternative (a human can `tmux attach` to watch).

A spike on three different programs before the decision: urna (ratatui, fullscreen, braille, truecolor), create-astro (inline prompts, Node) and burn's training dashboard (ratatui, braille plots, popups). All three drove and rendered identically to tmux; a full screen read took about 0.15 ms.

## Consequences

### Positive

- Sampling and stability checks are nearly free; latency can be measured (`anina audit`).
- Terminal modes are available, so exit hygiene can be checked.

### Negative or accepted trade-offs

- tui-test is young (0.1.0). The version is pinned and the trait keeps the exit open.
- tui-test always inherits the parent environment; a clean environment is made by wrapping the program in `env -i` (`Spec::argv`), the same for every backend.

### Follow-up

- Windows through tui-test's ConPTY, once tested.

## References

- https://github.com/microsoft/tui-test, crate `tui-test-rs`
- tuicli ADR-0001 (the tmux capture this one supersedes)
