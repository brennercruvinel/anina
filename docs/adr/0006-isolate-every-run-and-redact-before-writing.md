---
id: ADR-0006
title: Isolate every run and redact before anything is written
kind: decision
status: accepted
category: ISOLATION
date: 2026-10-06
last-updated: 2026-10-06
tags: [sandbox, privacy, home, path]
supersedes: []
superseded-by: null
related: [rust/record/src/cordon, rust/runner/src/redact.rs]
---

# ADR-0006: Isolate every run and redact before anything is written

## Context

Installers write outside any folder you give them, read your shell's environment and print your name. tuicli learned it the hard way: an active virtualenv leaked into the program under test; `/usr/bin` before Homebrew in `PATH` made an installer pick an old Python that broke later; a data-dir variable leaked a real home into a "clean" run. A published demo also showed the user's name and home path.

## Decision drivers

- A run must not touch the real machine and must not depend on it.
- Nothing personal or secret may reach a cast, a PNG or an SVG.

## Decision

- **`env` mode (default):** a fresh HOME and XDG dirs under the run's sandbox, an environment built from scratch (explicit `PATH`, `LANG`, `TERM`), the working dir outside any checkout.
- **`strict` mode:** the same inside the OS sandbox (`sandbox-exec` on macOS, `bwrap` on Linux), writes allowed only under the sandbox; an installer's every file is accounted for in a before/after report.
- **Redaction at the source:** the recorder redacts every screen it reads (home path, user and host names, git identity, token shapes, preset rules), keeping the cell grid intact.

## Consequences

### Positive

- Installers are tested as on a fresh machine and their footprint is reported.
- Demos are safe to publish by default.

### Negative or accepted trade-offs

- A program that needs the real HOME (credentials, caches) must be given what it needs explicitly.

### Follow-up

- None.

## References

- tuicli ADR-0004
