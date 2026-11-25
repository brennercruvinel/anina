---
id: ADR-0017
title: A reference run per system guards what anina records
kind: decision
status: accepted
category: STRUCTURE
date: 2026-11-25
last-updated: 2026-11-25
tags: [ci, compare, preset, self-check]
supersedes: []
superseded-by: null
related: [tool/tasks/selfcheck.sh, tool/tests/shell, demo/shell, rust/binary/src/versus.rs, ADR-0009]
---

# ADR-0017: A reference run per system guards what anina records

## Context

`anina compare` tells whether two runs are the same with timing taken out: the same files, the same screens as text, the same pixels, the same reports. Nothing used it on anina itself. A change in the recorder, in redaction or in a renderer changed what every run writes, and only a unit test that happened to cover the line would say so.

The `shell` preset is the smallest journey anina ships: a POSIX shell in an empty sandbox, no network, nothing to install. It runs in about 22 seconds and writes 14 files. Two runs of it on the same Mac compare equal (14 files: 5 texts, 4 PNG by pixels, 2 reports, 3 timing-bound).

What it writes depends on the system, for reasons that are the system's and not anina's. The preset spawns `sh`. Measured on macOS 27, where `/bin/sh` is bash 3.2:

- the step that lists the clean environment shows `BASH_SILENCE_DEPRECATION_WARNING`, which anina sets on macOS only, and `SHLVL` and `_`, which bash exports;
- the disk report lists `home/.bash_history`, 189 bytes, in `changes.json` and in `run.json`;
- the last screen shows the `exit` bash prints when it leaves.

On Debian and Ubuntu `sh` is dash. Linux was not run when this was written, so this part is expected, not measured: dash exports neither `SHLVL` nor `_`, keeps no history file and prints nothing when it leaves, and anina does not set the bash variable there. The environment screen (text and PNG), the transcript and both reports would then differ between the two systems. The first Linux reference will show how far that holds; one difference is enough to need two references.

## Decision drivers

- A change in what anina records fails the build, and the log shows what changed.
- A red build means anina changed, not that the runner is another system.
- A change that is meant has one place to say so, in the pull request that makes it.
- Little in git: what is kept is what the comparison reads.

## Considered options

### One reference for both systems

The preset would have to print the same on both. That means a shell that is the same on both (bash in POSIX mode is not on every Linux, dash is not on macOS) and no environment step. The environment step is what the preset is there to show: where the sandbox puts a program and what it gives it. The preset would be bent to fit its own check.

### No reference: two runs in the same job, compared

It needs nothing in git. It tells whether anina is steady, not whether it changed: both runs come from the same code.

### No reference: the run of `main` built in the same job

It catches every change with nothing in git. It builds anina twice per job, and a change that is meant has nowhere to be accepted: the check stays red until the merge.

### One reference per system, kept in the repository

`tool/tests/shell/macos` and `tool/tests/shell/linux`, each recorded on its system. The check compares with the folder of the system it runs on.

## Decision

One reference per system, in `tool/tests/shell/<system>`, and one script, `tool/tasks/selfcheck.sh`, that plays the preset and compares. `tool/tasks/fullcheck.sh` runs it as a gate, so CI and a developer run the same check.

The references sit beside the test sources and not in `demo/shell`: `demo/` is embedded in the binary, and a preset's folder is what a user copies to write their own.

A reference holds what `anina compare` reads: the texts, the PNGs, compared by pixels, and the two reports. The files compare only wants present, because their bytes follow the clock (the SVG, the cast, `index.html`), are kept empty. The macOS reference is 181 KB, of which the four PNGs are 177.

`tool/tasks/selfcheck.sh --update` records a reference again. It leaves the folder alone when the new run compares equal, so a refresh shows in git only what changed. A pull request that changes what anina records on purpose brings both references.

A system whose reference nobody recorded yet holds one file, `PENDING`. There the check plays the preset, warns that nothing was compared (an annotation in GitHub Actions) and passes. A system with no folder at all fails. Recording the reference removes the file, and from then on the check holds.

## Consequences

### Positive

- A change in the text, the pixels or the reports of the `shell` run fails `fullcheck` and `gatecheck`, with the list `anina compare` gives and a diff of each text that changed.
- A meant change is reviewed as what it is: the diff of the reference, PNGs side by side.
- No new code in anina: the check is two commands anina already had.

### Negative or accepted trade-offs

- Two references, recorded on two systems. From a Mac, the Linux one needs a container or a Linux machine.
- A new bash, dash or macOS that changes what `sh` leaves behind fails the check with no change in anina. The reference is then recorded again.
- The PNGs are compared by pixels, so the reference and the runner have to draw the same ones. The font is embedded and resvg draws without the system's help, but pixels from two processor families were not compared when this was written: a Linux reference recorded on arm64 may differ on an x86-64 runner.
- It guards one preset. The others need the network and what they install.

### Follow-up

- Record the Linux reference: `tool/tasks/selfcheck.sh --update` on Linux, then commit `tool/tests/shell/linux`.
- 2026-11-25 (#121): the first runs on GitHub's runners went red with no change in anina, and both reasons were the reference's. The environment step sorted by the host's `LANG`, which the sandbox hands down to the program: `_` came last on the Mac that recorded the reference and first on the runner. The preset now sorts with `LC_ALL=C`, so what it prints no longer follows the host's locale. And a Linux run recorded in an emulated container is no reference for the runner: its disk report lists the emulator's own cache folder (`home/.cache/rosetta`). A reference is recorded where the check runs: `gatecheck` uploads each system's run as the artifact `selfcheck-<system>`, and `tool/tasks/selfcheck.sh --keep SYSTEM RUN` keeps a downloaded one. The Linux reference is the runner's, so a Mac is enough to bring both.

## References

- `tool/tasks/selfcheck.sh`, `tool/tasks/fullcheck.sh`
- `rust/binary/src/versus.rs`: what `anina compare` reads and what it leaves out
- `rust/runner/src/engine/mod.rs`: `BASH_SILENCE_DEPRECATION_WARNING`, set on macOS only
- ADR-0009: one folder per run, with stable names
- Issue #121
