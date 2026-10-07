---
id: ADR-0014
title: The code and the CLI speak to different readers
kind: decision
status: accepted
category: STRUCTURE
date: 2026-10-07
last-updated: 2026-10-07
tags: [naming, cli, lexicon]
supersedes: []
superseded-by: null
related: [docs/TERMS.toml, rust/binary/src/main.rs]
---

# ADR-0014: The code and the CLI speak to different readers

## Context

The workspace split (ADR-0013) renamed modules to a shared lexicon (ADR-0010). Some of them now differ from what users type: `mapper` is `anina explore`, `taping` is `anina init`, `cordon` is `--sandbox`, the `inner` and `outer` engines are `--engine tui-test` and `--engine tmux`. Before v0.1.0 ships, renaming the CLI is cheap; after, it is a breaking change.

## Decision drivers

- Users read commands, flags, file formats and output names. They should say what a thing does.
- Contributors read the code. It should follow the lexicon.
- One name for one thing inside each vocabulary.

## Decision

Keep both. The CLI, the file formats (`preset.toml`, tape variables, `run.json`) and the output names (`anina-out/`) are the public vocabulary and stay as they are. The code keeps its names. `docs/TERMS.toml` holds the bridge: each term with a public name has a `cli` field.

## Consequences

### Positive

- No breaking change for users; the code keeps one lexicon.

### Negative or accepted trade-offs

- Two names for some things. A contributor looks one up in `docs/TERMS.toml`.

### Follow-up

- A new term with a public name gets its `cli` field in the same change.

## References

- ADR-0010, ADR-0013
- `docs/TERMS.toml`
