---
id: ADR-0001
title: Wait for what the screen shows, not for the clock
kind: lesson
status: accepted
category: TIMING
date: 2026-10-06
last-updated: 2026-10-06
tags: [wait, idle, flakiness]
supersedes: []
superseded-by: null
related: [rust/engine/mod.rs, rust/record/mod.rs, rust/teach.rs]
---

# ADR-0001: Wait for what the screen shows, not for the clock

## Context

anina's predecessor (tuicli, a set of shell scripts that drove one TUI through tmux) started with `sleep` between steps. Every timing that worked on one machine failed on another: a splash animation, a toast over a table, a file picker that animated while opening and was read mid-animation (the script then walked into the wrong folder, in another project).

## Decision drivers

- Runs must give the same result on a fast and a slow machine.
- A failed step must say why, not just "timeout".

## Decision

Every step that depends on the program waits on the screen:

- **`Wait TEXT|/re/`** until it shows, **`Gone`** until it leaves, **`Idle`** until the screen stops changing (cells known to change by themselves are ignored, ADR-0008).
- **One fixed text per state is enough** (`press enter to begin`, `steps ─`, `hits for`). Toasts hide what is under them: wait for them to go.
- **Act on stable screens.** A reading is trusted only when two consecutive reads agree; the engine makes reads cheap enough for that (ADR-0002).
- **Check machine-dependent state before acting** (`When "[ ] python env" Key Space`): a box may already be ticked.
- **A timeout is a finding.** The error shows the screen it saw, suggests the closest text on it (`did you mean …?`) and saves the screen to disk (`rust/teach.rs`). In tuicli one such timeout exposed a real bug in the program under test.

`Sleep` remains only to let an animation finish before a screenshot, or to pace a demo.

## Consequences

### Positive

- Tapes run unchanged on any machine; failures explain themselves.

### Negative or accepted trade-offs

- A tape author must pick a stable text per state; `anina init` and `anina explore --to` pick it for them.

### Follow-up

- None.

## References

- tuicli ADR-0002 and ADR-0003 (the shell-era lessons this one generalizes)
