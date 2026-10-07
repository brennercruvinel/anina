---
id: ADR-0016
title: Put back only what a play changed in the sandbox
kind: decision
status: accepted
category: ISOLATION
date: 2026-10-07
last-updated: 2026-10-07
tags: [sandbox, audit, preset, restore]
supersedes: []
superseded-by: null
related: [rust/record/src/preset/mod.rs, rust/record/src/cordon/diffs.rs, ADR-0006]
---

# ADR-0016: Put back only what a play changed in the sandbox

## Context

`anina audit --preset` plays a preset's parts in up to seven terminals, and each one starts from the sandbox as prepare left it. The sandbox was copied whole after prepare (`freeze`), then deleted and copied back before every terminal (`restore`), the first one included. With `node_modules` at hundreds of MB, the audit spent most of its time copying. The copy also broke on what real installs leave behind. A socket or a FIFO aborted `freeze`, and a read-only folder (Go's module cache) made deleting the sandbox fail with EACCES.

## Decision drivers

- Every terminal starts from the same state, or the audit compares different programs.
- An audit costs about what its plays cost.
- What a real install leaves behind (sockets, read-only caches) can't stop a run.

## Considered options

### Skip the restore before the first terminal

Right, but it saves one copy out of seven.

### Clone files instead of copying them

On macOS, `std::fs::copy` already clones on APFS, and on Linux it uses `copy_file_range`. What is left is walking and recreating every entry, which cloning doesn't remove.

### Put back only what changed

A `Snapshot` (kind, size, mtime per path) taken after `freeze` and after every restore. Before the next play, the paths that differ, topmost only, are deleted and copied back from the saved tree.

## Decision

Put back only what changed, and skip the first restore. If putting back fails, the whole tree is copied again as before. Sockets, FIFOs and devices are not copied, because they belong to the process that made them. Folders keep their permissions, and deleting a tree gives every folder back its owner's write bit first.

## Consequences

### Positive

- A restore costs two walks of the sandbox plus what the play wrote.
- A sandbox with a socket, a FIFO or a read-only cache audits, and a second `anina run` can empty it.

### Negative or accepted trade-offs

- A change that keeps a file's size and mtime goes unseen. A play that rewrites a file and resets its mtime would carry it into the next terminal.
- A changed folder permission is not put back.

### Follow-up

- None.

## References

- `rust/record/src/preset/mod.rs` (`Stage::freeze`, `Stage::restore`, `copy_tree`, `remove_tree`)
- `rust/record/src/cordon/diffs.rs` (`Snapshot::differs`)
- ADR-0006
