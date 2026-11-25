---
id: ADR-0010
title: Sibling names share a length; what the world named keeps its name
kind: decision
status: accepted
category: STRUCTURE
date: 2026-11-25
last-updated: 2026-11-25
tags: [naming, lexicon, modules]
supersedes: []
superseded-by: null
related: [docs/TERMS.toml, rust/, ADR-0013, ADR-0014]
---

# ADR-0010: Sibling names share a length; what the world named keeps its name

## Context

This record is written after the decision. The number and the file name were reserved on 2026-10-07, when the folders were laid out for the workspace split, and the file stayed empty. The split (ADR-0013) renamed the modules that same day, and `docs/TERMS.toml` and ADR-0014 cited this number. Two days later the empty file went with the other placeholders. For seven weeks three documents pointed at an ADR that was not there.

The record holds the rule, not its reasons. A terms file carried over from another project (`docs/TERMS.md`, left out of the split) stated it in one sentence: siblings of the same kind have the same length, and each abbreviation has one meaning. Issue #1 lists every rename and gives no motive. So this ADR says what the rule is, measured in the tree, and what it gives and costs as that can be read today. It does not claim to know why it was first chosen.

The rule as it held on 2026-11-25, before this ADR changed four names:

| where | names | letters |
| --- | --- | --- |
| folders at the root | `data`, `demo`, `docs`, `rust`, `tool` | 4 |
| crates in `rust/` | `binary`, `record`, `render`, `runner`, `screen` | 6 |
| modules of `screen` | `escape`, `frames`, `lesson` | 6 |
| modules of `runner` | `engine`, `masker`, `redact`, `script` | 6 |
| modules of `record` | `cordon`, `preset` | 6 |
| modules of `binary` | `audits`, `checks`, `mapper`, `taping`, `versus` | 6 |
| modules of `render` | `brand`, `fonts`, `index`, `pixel`, `shape`, `theme` | 5 |
| `engine/` | `inner`, `outer` | 5 |
| `cordon/` | `diffs`, `linux`, `macos` | 5 |
| `preset/` | `print`, `specs` | 5 |
| `audits/` | `cases`, `meter`, `print`, `ratio`, `score` | 5 |
| `mapper/` | `graph`, `hints` | 5 |
| `taping/` | `input`, `print`, `relay` | 5 |
| `tool/` | `tasks`, `tests` | 5 |
| `docs/` | `adr`, `img` | 3 |

Six letters for a crate and for a module at the top of a crate, five for a module inside another. `render` is the one crate whose modules have five: they sat inside a `render` module before the split and kept their length when it became a crate.

Some names were never under the rule. `lib.rs`, `mod.rs` and `main.rs` are Rust's, `.github` and `.zed` belong to their tools. A folder in `demo/` carries the name of the program it records, the one a user types (`anina run glow`, `helix`, `harlequin`). `linux` and `macos` are the systems' own names, five letters by coincidence. Types kept their names through the split: `TuiTest`, `Tmux`, `Sandbox`, `Manifest`. Nothing checks the rule but review. An empty `tool/tasks/namecheck.py` was reserved alongside this ADR and removed with it; whatever it was meant to check, it never ran.

To reach those lengths the split renamed 27 modules (`docs/CHANGELOG` has the list). A few kept their word (`audit` became `audits`, `font` became `fonts`); most took a new one (`tape` became `script`, `sandbox` became `cordon`). Four of those replaced a name the whole terminal world uses with a word only anina uses: the tui-test engine became `inner`, the tmux engine `outer`, the SVG writer `shape` and the PNG writer `pixel`.

## Decision drivers

- Three documents cite this ADR; it has to exist and match the code.
- Someone who knows terminals and has never read anina should find the code of an engine or of a file format by its name, with `ls` or a search by file name.
- Keep what the rule gives where it costs little.
- One principle, so the next module needs no ruling. A list of exceptions does not say what to call the third engine.
- ADR-0014 stands: the code and the CLI are two vocabularies, and `docs/TERMS.toml` is the bridge.

## Considered options

### Keep every name

No work. `inner`, `outer`, `shape` and `pixel` keep hiding the four things in the tree a newcomer already knows by name, and `TERMS.toml` keeps two entries whose only job is to undo a rename.

### Drop the rule and name each module after its public word

`tape`, `sandbox`, `explore`, `init`, `compare`. It undoes ADR-0014 seven weeks after it was accepted, and 27 names move again. It also gives up what the rule gives (below) to fix a problem that only four names have.

### Rename the five names the issue lists, and say no more

Issue #119 names `inner`, `outer`, `shape`, `pixel` and `versus`. Renaming them by list fixes today and leaves no rule: `versus` would become `compare` while `mapper` stays for `explore`, and nobody could say why.

### Draw a line between the names anina coins and the names the world gave

The lexicon keeps the first kind. The second kind is not anina's to rename.

## Decision

The lexicon stays, with a line through it.

**A name anina coins follows the lexicon.** A layer, a command, a step of the pipeline: anina picks one word for it, of the length its siblings have. When users know the same thing by another word, `docs/TERMS.toml` holds both (ADR-0014).

**A name the world gave stays as the world gave it.** When a module's whole job is one thing that exists outside anina under a proper name (an engine, a file format, an operating system, a program in `demo/`), the module takes that name, whatever its length. The test is whether the word is a proper name: tmux, SVG and Linux are; explore, compare and tape are common words anina chose among.

Read against every module in the tree:

| module | holds | kind | name |
| --- | --- | --- | --- |
| `engine/inner` | the tui-test engine | given | becomes `engine/tuitest` |
| `engine/outer` | the tmux engine | given | becomes `engine/tmux` |
| `render/shape` | the SVG writer | given | becomes `render/svg` |
| `render/pixel` | the PNG writer and reader | given | becomes `render/png` |
| `cordon/linux`, `cordon/macos` | the sandbox of each system | given | stay; they already were |
| `demo/*` | one program each | given | stay; they already were |
| `binary/versus` | `anina compare` | coined | stays |
| `mapper`, `taping`, `audits`, `checks` | `explore`, `init`, `audit`, `check` | coined | stay |
| `cordon`, `script`, `frames` | the sandbox, the tape language, a cast | coined | stay |
| every other module | a step of anina's own | coined | stays |

`tuitest` is one word because a Rust module takes no hyphen, `tui_test` is the name of the crate it wraps, and a file called `tui_test.rs` reads as a test. It is also the name the module had before the split, and its type is `TuiTest`.

`versus` stays. `compare` is not a name the world gave to a thing; it is the verb anina chose for a command. That makes `versus` and `compare` the same kind of pair as `mapper` and `explore` or `taping` and `init`, and ADR-0014 already decided those: the code keeps its word, the CLI keeps its own, `TERMS.toml` joins them. Renaming `versus` alone would be the exception this ADR set out not to make.

The tape language is the closest call. vhs used the word first, but a tape here is anina's own language that also reads vhs's, and `script` is not a wrapper around vhs. It stays coined. It had no entry in `TERMS.toml`, and neither had `frames`: both get one, with the file a user knows them by (`*.tape`, `*.cast.json`).

What the rule gives, as the tree shows it:

- Names of one kind line up. `ls rust/`, a block of `mod` lines and a `use` list read as a column.
- A fixed length leaves room for one word. No compounds (`tape_parser`), no prefixes, so one name for one thing (ADR-0014) is easy to keep.
- A name that breaks the pattern shows at a glance, and now it means something: the thing is not anina's.

## Consequences

### Positive

- `ls rust/runner/src/engine` lists the engines by the names `--engine` takes, and `ls rust/render/src` shows `svg.rs` and `png.rs`.
- The next engine, format or system has its name before the file is created.
- `TERMS.toml` loses the `outer` entry, and `tuitest` differs from its public name by a hyphen.
- No reference to a missing ADR.

### Negative or accepted trade-offs

- Siblings no longer line up where a given name sits: `engine/` holds `tmux` and `tuitest`, and `render/` holds four names of five letters next to `png` and `svg`.
- The coined names still hide what they hold. A search for sandbox, tape, cast, explore, init or compare finds text inside files, not `cordon`, `script`, `frames`, `mapper`, `taping` or `versus`. A contributor needs `TERMS.toml`, and for a module with no entry there (`ratio` for contrast, `relay`, `meter`) has to open the file.
- A name chosen for its length is sometimes the second-best word. `index` is the gallery page, `specs` the preset manifest, `print` three different writers in three folders.
- The line takes judgment at its edge. `escape` parses ANSI sequences and `index` writes HTML; both are read here as anina's own step, not as a wrapper around a named thing.
- The same four files moved twice in seven weeks. `git log --follow` crosses both moves.
- Still nothing checks the rule but review.

### Follow-up

- A new coined name with a public word gets its `TERMS.toml` entry in the same change (ADR-0014).

## References

- ADR-0013 (the split that renamed the modules), ADR-0014 (code and CLI)
- `docs/TERMS.toml`, `docs/CHANGELOG`
- Issue #1 (the rename list), issue #119
