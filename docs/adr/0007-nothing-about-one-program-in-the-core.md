---
id: ADR-0007
title: Nothing about any one program in the core; programs live in presets
kind: decision
status: accepted
category: STRUCTURE
date: 2026-10-06
last-updated: 2026-10-06
tags: [presets, generic]
supersedes: []
superseded-by: null
related: [rust/preset, presets/]
---

# ADR-0007: Nothing about any one program in the core; programs live in presets

## Context

tuicli was written for one program (anina): its paths, its binary, its corpus and its gallery titles were in the core scripts. Pointing it at another TUI meant editing it.

## Decision drivers

- Any terminal program, from a one-line prompt to a fullscreen dashboard.
- Program knowledge must be data a user can write, not code.

## Decision

The core (`screen`, `engine`, `tape`, `record`, `render`, `explore`, `audit`, `init`) never names a program. Everything specific lives in a preset: a `preset.toml` (requirements with install hints, variables with defaults from the environment, sandbox mode, cached prepare steps, redaction rules, theme) and its tapes, one per part of the journey. `anina run NAME [--part P]` runs the whole journey or some parts. Presets for a shell, create-astro, burn and anina ship as examples and as the test suite.

A lesson kept from tuicli for presets of an installer: test the installer with the released binary users get, and the rest with the build under development; a version that reports the same number with a different layout fails in between.

## Consequences

### Positive

- A new program is a new folder, not a code change.

### Negative or accepted trade-offs

- Presets that need network or long builds (astro, burn) are slower to run; prepare steps are cached.

### Follow-up

- Community presets from a git URL.

## References

- tuicli ADR-0005 and ADR-0006
