---
id: ADR-0013
title: Split anina into a workspace of layered crates
kind: decision
status: accepted
category: STRUCTURE
date: 2026-10-07
last-updated: 2026-10-07
tags: [rust, workspace, crates]
supersedes: []
superseded-by: null
related: [Cargo.toml, rust/, release.toml, .github/workflows/gatecheck.yml]
---

# ADR-0013: Split anina into a workspace of layered crates

## Context

anina was one crate (ADR-0003): a library of 17 modules under `rust/` and a binary on top. The layers were real (the screen model at the bottom, engines and the tape language above it, then rendering and recording, then the commands) but nothing enforced them: any module could reach any other through `crate::`. The empty modules waiting for work (`codecs`, `design`, `motion`, `recode`) had the same reach.

## Decision drivers

- The compiler, not review, should keep a lower layer from reaching up.
- A layer should build and test alone.
- Still one binary and one `cargo install anina` (ADR-0003).
- The CLI does not change: same commands, same flags, same output.

## Considered options

### Keep one crate

No work. Layering stays a convention, and every change rebuilds everything.

### A workspace of layered crates

One crate per layer under `rust/`, each one depending only on the layers below. Costs a manifest per crate and a pass over every `crate::` path.

## Decision

A virtual workspace (`members = ["rust/*"]`) of nine crates. Shared metadata and dependency versions live in the root `Cargo.toml`; each crate states what it inherits and what it uses.

| crate | package | depends on |
| --- | --- | --- |
| `rust/screen` | `anina-screen` | none |
| `rust/runner` | `anina-runner` | screen |
| `rust/render` | `anina-render` | screen |
| `rust/record` | `anina-record` | screen, runner, render |
| `rust/binary` | `anina` | screen, runner, render, record |
| `rust/codecs`, `rust/design`, `rust/motion`, `rust/recode` | `anina-codecs`, `anina-design`, `anina-motion`, `anina-recode` | none yet |

Modules took names of equal length on the way (see ADR-0010): `ansi`, `cast` and `teach` became `screen::escape`, `frames` and `lesson`; `tape`, `volatile` and `privacy` became `runner::script`, `masker` and `redact`; `sandbox` became `record::cordon`; `audit`, `explore`, `init` and `check` became the binary's `audits`, `mapper`, `taping` and `checks`. `docs/CHANGELOG` has the full list.

`data/theme` and `demo/` sit outside the crates that embed them, and `cargo package` only ships what is inside a crate. Two symlinks bring them in: `rust/render/assets -> ../../data/theme` and `rust/record/demo -> ../../demo`. The tree stays as it is and `cargo package --workspace` passes.

## Consequences

### Positive

- A wrong-way dependency fails to compile.
- `cargo check -p anina-screen` builds the bottom layer alone.
- The empty crates have a place and no reach until someone depends on them.

### Negative or accepted trade-offs

- A clone on Windows needs `git config core.symlinks true` to build.
- The `leaks` test lives in `tool/tests/`, outside `rust/binary`: it runs in the repo and in CI but is not in the published package.
- `cargo package -p anina` alone fails until the `anina-*` crates are on crates.io; validate with `cargo package --workspace`.
- cargo-release sees nine crates: `tag-name = "v{{version}}"` at the root and `tag = false` in the eight inner crates keep one `v0.1.0` tag.

### Follow-up

- `anina-codecs`, `anina-design`, `anina-motion` and `anina-recode` join `[workspace.dependencies]` when a crate first depends on them.

## References

- ADR-0003, ADR-0010
- Issue #1 (the migration plan)
