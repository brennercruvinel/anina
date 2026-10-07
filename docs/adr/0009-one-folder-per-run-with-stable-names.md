---
id: ADR-0009
title: One output folder per run, with stable screen names
kind: decision
status: accepted
category: STRUCTURE
date: 2026-10-06
last-updated: 2026-10-06
tags: [outputs, runs]
supersedes: []
superseded-by: null
related: [rust/record/src/preset, rust/record/src/lib.rs]
---

# ADR-0009: One output folder per run, with stable screen names

## Context

tuicli once overwrote a run with a half-finished one (a shell glob that matched nothing aborted a cleanup), and comparing two runs meant guessing which file was which.

## Decision

Every run writes to its own folder (`anina-out/<name>/…` by default, `--out` to choose). Screenshots keep the names the tape gives them (`Snap NN-name`), so two runs compare file by file: the `.txt` for layout, the PNG for color. Each run folder holds the cast, the SVG, the shots, `run.json` and an `index.html`.

## Consequences

### Positive

- Runs never clobber each other; diffs between runs are plain `diff -r`.

### Negative or accepted trade-offs

- Old runs pile up; they are disposable and ignored by git.

### Follow-up

- None.

## References

- tuicli ADR-0008
