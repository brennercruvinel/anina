---
id: ADR-0015
title: A written wait holds until the program answered, in every terminal
kind: lesson
status: accepted
category: TIMING
date: 2026-10-07
last-updated: 2026-10-07
tags: [init, waits, audit, scroll, typing]
supersedes: []
superseded-by: null
related: [rust/binary/src/taping/print.rs, rust/screen/src/lib.rs, ADR-0001, ADR-0008]
---

# ADR-0015: A written wait holds until the program answered, in every terminal

## Context

`anina init` turns a session into a tape, a step and then a wait. `anina audit` replays that tape from 120x40 down to 60x20. The first waits it wrote passed too early or never passed:

- After a scroll it wrote `Idle 3`, because what scrolls into view depends on the terminal's height. `Idle` returns after 600 ms without a change and never fails. In a shell, the next keys went out before `cargo build` was done.
- After typing it waited for the end of the typed text. That text is the terminal's echo, not the program's answer. In fzf or glow's `/` filter, the wait passed before the filter ran, and the next `Key enter` picked the old selection.
- A typed line longer than 60 columns wraps in the smallest terminal. A word cut at the edge matched neither row by row nor with rows joined by spaces, so the echo wait never passed.

## Decision

- **After a scroll:** wait for new text near the cursor, up to ten rows above it and ten below, counted as the rows wrap at 60 columns (a 190-column row counts as four). Rows next to the cursor (a shell's last output, an editor's new line) show in every height. When the cursor is hidden or nothing new sits near it, `Idle 3` is still the step.
- **After typing:** first the echo, then the program's answer, found away from the echo's rows: a `Wait` for text it brought up and a `Gone` for text it took away. A filter's bar shows with its first key, but its rows go only once it ran, so both are written when both exist. `Idle 3` comes only when neither exists. The echo may drop leading signs (`/install` echoes as `install`). The echo's rows are the row the cursor ended on and the rows the text wrapped over above it. Where the cursor was before doesn't count, because a key that opened a field moved it (`/` in less takes it from the top to the last row). With the cursor hidden they are the rows that end with the typed text, the way an input field shows it. A row that merely holds the text doesn't count, since typing `e` would hide every row with an e.
- **A key that opens a field is its own step:** keys under 0.25 s apart are one `Type`, except a `/` or `:` after which the screen changed off the cursor rows (a filter's bar). That key gets its own wait before the text goes in.
- **Nothing past the narrow edge:** a `Wait` or `Gone` text must end before column 56, counted in columns (a wide character takes two, though `row_text` gives it one char). In the 60-column terminal a TUI cuts what lies past it, so a `Wait` for it never passes and a `Gone` passes at once. A whole new row counts only when no row of before, cut at that edge, already equals it.
- **No `Gone` after a scroll:** rows that leave at the top leave with the scroll (a long line wrapped on the last row). In a taller terminal they stay, so after a scroll only new text counts.
- **Wrapped text:** a literal `Wait` also matches rows joined where a row is filled to its last column, the way the terminal wrapped it.
- **One reading of the screen:** `Screen::shows` and `Screen::count_shown` are what the replay's literal `Wait` and `Gone` match with. The recorder picks with the same two: a wait never shows before the step and shows once after it, and a `Gone` never shows after it. Text that touches an edge the terminal wrapped at (`comp` ending a full row, `lete` starting the next) is never picked, because another width cuts it elsewhere.

## Consequences

### Positive

- A replay types the next key once the program has answered, in every size the audit tries.
- A `Gone` can't fail because of the terminal's size. A smaller screen that never showed the text passes it right away.

### Negative or accepted trade-offs

- A hidden cursor says nothing about where the program draws, so a scroll there still settles with `Idle`.
- A row that ends exactly at the last column and is followed by an unrelated row reads as one line to a literal wait. A false match needs the two rows to spell the waited text across the edge.

### Follow-up

- None.

## References

- `rust/binary/src/taping/print.rs` (`push_sync`, `appeared`, `reaction`, `vanished`)
- `rust/screen/src/lib.rs` (`Screen::text_unwrapped`, `Screen::shows`, `Screen::count_shown`)
- Review of #58: #59 and its sub-issues #60 to #68
- Review of #70: #84 and its sub-issues #85 to #96
- ADR-0001, ADR-0008
