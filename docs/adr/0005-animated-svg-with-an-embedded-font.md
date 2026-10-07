---
id: ADR-0005
title: Animate as a self-contained SVG with an embedded font subset
kind: decision
status: accepted
category: RENDER
date: 2026-10-06
last-updated: 2026-10-06
tags: [svg, smil, font, png]
supersedes: []
superseded-by: null
related: [rust/render/src/shape.rs, rust/render/src/pixel.rs, rust/render/src/fonts.rs]
---

# ADR-0005: Animate as a self-contained SVG with an embedded font subset

## Context

A demo has to play in a README, inside an `<img>`, with no script. tuicli's SVG (SMIL discrete opacity, rows deduplicated in `<defs>`, braille as dots, blocks as rectangles) proved the approach: a 52 s fullscreen journey in 1.3 MB, 82 KB gzipped. But its text used the viewer's monospace font, and an `<img>` cannot load web fonts, so the demo looked different on every machine.

## Decision drivers

- The same picture everywhere, including GitHub.
- Small enough for a README.
- No browser to make screenshots.

## Decision

- The animated SVG keeps tuicli's technique (one timeline, `calcMode="discrete"`, `textLength` on a fixed grid, braille and block glyphs drawn as shapes, captions, progress bar, a poster frame for viewers without animation).
- The text font is embedded as a subset (only the glyphs used) of JetBrains Mono (OFL) in a data URI.
- PNG screenshots render the same SVG for a single screen and rasterize it with resvg, using the same embedded font. No browser.
- Each SVG carries `<title>`/`<desc>` and a text transcript, so the demo is readable without sight.

## Consequences

### Positive

- Identical rendering on every machine; screenshots need nothing installed.

### Negative or accepted trade-offs

- The font subset adds a few tens of KB per SVG.

### Follow-up

- None.

## References

- tuicli ADR-0009 (the SVG technique)
