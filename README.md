# anina

**Audit, map, redact and record any terminal app. No AI required.**

anina runs a program in a real terminal and reads its screen the way you do, cell by cell, colors and all. It never asks which framework drew the screen. ratatui, Textual, Bubble Tea and a plain `printf` all look the same to it.

Think Lighthouse + Playwright + VHS, for terminal apps. The result is deterministic: the same input gives the same output.

<p align="center"><img src="docs/img/urna-tui.svg" alt="anina recording urna's TUI: open a corpus, ask it, read the cited answer" width="100%"></p>

<sub>Recorded by `anina run urna`: a ratatui app with braille art, truecolor and live animation, in a 425 KB self-contained SVG (48 KB gzipped).</sub>

## What it found so far

urna writes truecolor even when the terminal never announced it. Its dimmest palette steps fall below WCAG contrast, with the key-hint footer at 3.6:1, and the home screen rewords itself at 80x24, which breaks any tape waiting on that text.

Killing a burn training run from its TUI exits with code 101, a deliberate `panic!`. And plain `ls -G` on macOS ignores `NO_COLOR`; its directory blue on black is 2.2:1.

## What a run prints

`anina run helix` opens helix in an empty sandbox. The tape searches a word and changes it, opens the file picker, then saves and quits, and anina prints this:

```text
$ anina run helix
anina [helix]   ok in 12.6s: 36 frames, 24 screens, 4 shots
anina [helix] disk: 9 created, 0 modified, 0 deleted, 604 B written
                   555 B  work/greeter
                    49 B  home/.config
                     0 B  home/.cache
anina [helix] redacted home path ×1
```

`anina-out/helix/` gets the animated SVG, four PNGs, the cast, an HTML page and the list of every file helix wrote. The window takes its colors from helix's own screen. Every preset in [`demo/`](demo) works the same way, and `anina presets` lists them.

## Commands

```sh
anina audit -- htop                # five scores, no tape to write
anina presets                      # what ships
anina run shell                    # the smallest journey, no network
anina init -o demo.tape -- htop    # use htop; ctrl+] stops; demo.tape replays it
anina record demo.tape             # -> anina-out/demo/demo.svg
anina explore -- htop              # graph.json, map.md, screens/*.png
anina check                        # nothing in anina-out you would not publish
anina doctor                       # can anina drive a terminal here?
```

`audit` runs the same journey in many terminals, with other sizes, color depths, `NO_COLOR` and `LC_ALL=C`. It scores stability, responsiveness and contrast, plus compatibility and terminal hygiene. `explore` is experimental: it reads the key hints on screen, tries them, and maps every screen it reaches. `render` draws a recording again with another theme or speed. `brand` takes a theme from the app's own colors, and `export` writes asciicast v3 for the asciinema player.

Tapes are one step per line, and vhs tapes run as they are:

```text
Spawn my-tui --flag
Wait "sections"                    # text or /regex/
Type "how do citations work"
Key Enter Down*2
Snap 03-answer                     # a named screenshot
```

The full reference sits at the top of [`rust/runner/src/script.rs`](rust/runner/src/script.rs). A wait that fails shows what was on screen, suggests the closest text, and saves that screen.

## Privacy

Every command redacts what it reads before anything is written. Your home path becomes `~`, your user and host names become `user` and `host`, and git identity and token-shaped secrets become `you@example.com` and `••••`. That holds for every file it writes, screenshots and casts included. A replacement keeps the grid, so the columns after it stay put.

What anina can't guess goes in `anina.toml` next to where you run it, or in `~/.config/anina/config.toml`:

```toml
[redact]
replace = { 'ChatGPT\.app' = "Notes.app" }   # regex = example text
mask = ['/Applications/[^/ ]+\.app']          # regex, shown as •••, cell for cell
```

`anina run` gives every journey a fresh HOME and environment. `--sandbox strict` adds the OS sandbox, `sandbox-exec` on macOS and `bwrap` on Linux, and the program can't write outside it. Before you publish, `anina check` looks through the output for anything that slipped and fails if it finds some.

## Install

```sh
cargo install --git https://github.com/brennercruvinel/anina anina
anina doctor
```

Rust 1.90+, macOS or Linux. tmux is optional, a second engine you can attach to and watch.

## How it's built

anina waits on what the screen shows, never on the clock. Cells that change by themselves, a spinner or a clock, are learned and left out when two screens are compared. The terminal is tui-test's emulator running in process; a full screen read takes about 0.15 ms, so anina can sample often and measure latency.

The SVG is one looping timeline with an embedded subset of JetBrains Mono, so it looks the same everywhere, `<img>` on GitHub included. PNGs come from the same SVG through resvg, no browser.

The code is a workspace of nine crates in [`rust/`](rust), layered from the screen up to the binary. The crates `codecs`, `design`, `motion` and `recode` are still empty. They hold what comes next: a design system read from the screen, a timeline lighter than a GIF, and the tree rewritten for another framework. The reasons behind each choice are in [`docs/adr/`](docs/adr).

## Limits

Mouse input isn't scripted yet. Glyphs outside JetBrains Mono (CJK, emoji, some arrows) fall back to the viewer's fonts. `explore` treats screens with the same key bar and most cells equal as one state, so a search box can show up as a screen of its own. Windows is untested, and on macOS a process list still shows real app names; `mask` rules are how you hide them.

MIT, see [LICENSE](LICENSE). [Hoff Research](https://hoffresearch.com). JetBrains Mono is under the SIL Open Font License ([`OFL.txt`](data/theme/fonts/mono/jetbrains/OFL.txt)).

Made it simple, but significant (∂μFμν = Jν)<br>
Author: Brenner Cruvinel
