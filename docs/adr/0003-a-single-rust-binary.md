---
id: ADR-0003
title: Ship anina as a single Rust binary
kind: decision
status: accepted
category: STRUCTURE
date: 2026-10-06
last-updated: 2026-10-06
tags: [rust, distribution]
supersedes: []
superseded-by: null
related: [Cargo.toml, .github/workflows/gatecheck.yml]
---

# ADR-0003: Ship anina as a single Rust binary

## Context

tuicli was POSIX sh plus Python stdlib, tmux and a headless Chromium for PNGs. That is three runtimes for a user to install before the first screenshot.

## Decision drivers

- One command to install, no runtime to manage.
- Use the engine (ADR-0002) as a library, not a subprocess.
- No browser for screenshots.

## Considered options

### Python package (PyPI, `uv tool install`)

Least rewriting; still needs tmux and a browser; tui-test only reachable through its bindings.

### Go binary (like vhs)

Good distribution; a full rewrite and no direct use of the Rust engine.

### Rust binary

A full rewrite, but the engine links in directly and PNGs rasterize with resvg.

## Decision

Rust, one crate, one module per responsibility (`screen`, `engine`, `tape`, `record`, `cast`, `render`, `volatile`, `privacy`, `sandbox`, `teach`, `explore`, `init`, `audit`, `brand`, `preset`). `cargo install anina` works; release binaries follow.

## Consequences

### Positive

- One binary does everything; tmux and the OS sandboxes are optional extras that `anina doctor` reports.

### Negative or accepted trade-offs

- The Python renderers of tuicli were ported, not reused.

### Follow-up

- Release automation (cargo-dist) and a Homebrew tap.

## References

- ADR-0002
