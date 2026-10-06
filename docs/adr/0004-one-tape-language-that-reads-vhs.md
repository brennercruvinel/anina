---
id: ADR-0004
title: One tape language that also reads vhs tapes
kind: decision
status: accepted
category: FORMAT
date: 2026-10-06
last-updated: 2026-10-06
tags: [tape, vhs, dsl]
supersedes: []
superseded-by: null
related: [src/tape/mod.rs]
---

# ADR-0004: One tape language that also reads vhs tapes

## Context

charmbracelet/vhs made `.tape` files the common way to script terminal demos. tuicli had its own similar language with what vhs lacks for fullscreen TUIs: `Gone`, `Idle`, `When`, `Caption`, `Speed`, `Poster`, a clean environment.

## Decision drivers

- People with vhs tapes should not rewrite them.
- Keep the steps that make fullscreen TUIs reliable.

## Decision

One parser reads both dialects (`src/tape/mod.rs`): `Type@50ms "…"`, `Enter 3`, `Ctrl+C`, `Sleep 500ms`, `Wait+Screen@10s /re/`, `Screenshot`, `Output`, `Require`, `Env`, `Source`, next to anina's own steps. Quoting is POSIX-like. vhs pixel and style settings (`FontSize`, `Width`, `Padding`…) become warnings: anina sizes by columns and rows and styles by themes. Without a `Spawn`, a tape types into a shell, as in vhs, after the prompt settles.

## Consequences

### Positive

- vhs tapes run as is; `Output x.cast` also exports asciicast v3.

### Negative or accepted trade-offs

- `Copy`/`Paste` and scroll steps are skipped with a warning for now.

### Follow-up

- None.

## References

- https://github.com/charmbracelet/vhs
