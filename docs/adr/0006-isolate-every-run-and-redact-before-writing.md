---
id: ADR-0006
title: Isolate every run and redact before anything is written
kind: decision
status: accepted
category: ISOLATION
date: 2026-10-06
last-updated: 2026-11-25
tags: [sandbox, privacy, home, path]
supersedes: []
superseded-by: null
related: [rust/record/src/cordon, rust/runner/src/redact.rs, SECURITY.md]
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

- 2026-11-25 (#120): `strict` holds for everything a run starts, not only for the program. A tape's `Exec`, a preset's `[[prepare]]` and `explore --before` start behind the same prefix as `Spawn` (`Wrap::argv` in `rust/record/src/cordon`). Until then only the program was inside, and the shell lines of the same run wrote wherever they could. When a confined line fails, the error says what `strict` allows, and that the sandbox refused when the line's own output shows it.
- 2026-11-25 (#120): `prepare` runs inside the sandbox, and for `prepare` alone anina's cache (`${ANINA_CACHE}`) is a second writable folder. The other choice was to leave `prepare` outside and say so. What the shipped presets do decided it. Two of the seven have a `prepare`. `urna` downloads a release, or runs an installer script, into the sandbox's own temp and home: that is the installer this ADR is about, and left outside it would run unconfined in the one mode that promises to hold it. `burn` clones and builds under `${ANINA_CACHE}/burn`: that is why the cache is writable. The program and the `Exec` lines get the root only, so they cannot touch the copies and the other sandboxes kept in the cache.
- 2026-11-25 (#120): no toolchain's own cache is made writable (`~/.cargo`, Go's module cache): ADR-0007 keeps programs out of the core, and every folder added is one more place a run can write. So under `strict` a `prepare` that fills such a cache is refused and says so: `burn`'s build is one, cargo keeps the crates it downloads in its own home. `burn` declares `env`, where nothing changed. A step whose `creates` exists is skipped, so after one run under `env` the clone and the build are there, and a run under `strict` has nothing left to prepare.
- 2026-11-25 (#120): under `strict` the network is off unless the preset says `network = true`, for the program and for the shell lines alike. The field was printed by `anina presets` and read nowhere else. `explore` has no preset to say it, so `explore --sandbox strict` has no network; a program that needs it is explored under `env`. `record` and `audit` without `--preset` have no sandbox mode: they run as `off`.
- 2026-11-25 (#120): the fresh HOME and the empty environment are still the program's alone. `Exec`, `prepare` and `--before` keep the environment anina was started with; `strict` holds what they write and reach, not what they see. The guarantees and the limits, by mode and by system, are in [`SECURITY.md`](../../SECURITY.md).

## References

- tuicli ADR-0004
