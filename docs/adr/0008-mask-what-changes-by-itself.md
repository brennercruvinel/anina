---
id: ADR-0008
title: Mask the cells that change by themselves
kind: decision
status: accepted
category: TIMING
date: 2026-10-06
last-updated: 2026-10-06
tags: [volatile, determinism, explore]
supersedes: []
superseded-by: null
related: [rust/runner/src/masker.rs, rust/binary/src/mapper]
---

# ADR-0008: Mask the cells that change by themselves

## Context

Many screens never stand still: anina's hash rain and breathing logo, spinners, clocks, elapsed times, create-astro's random project name, burn's live plots. "Has the screen changed?" and "is this the same screen?" give wrong answers on them, which breaks idle waits, screen maps and comparisons between runs.

## Decision drivers

- Generic: no list of known animations.
- Cheap enough to run on every state.

## Decision

Watch a screen while nobody types; the cells that change form its volatile mask (`rust/runner/src/masker.rs`). Hashes, idle waits and state identity skip masked cells. The mask is learned per state and grows across visits.

## Consequences

### Positive

- `explore` maps animated TUIs into a finite graph; `Idle` settles on animated screens.

### Negative or accepted trade-offs

- A cell that changes only rarely may be missed by a short observation; masks grow when it happens again.

### Follow-up

- None.

## References

- ADR-0001
