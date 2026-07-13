# Spec 002 — Index explorer (web)

- **Status:** implemented
- **Scope:** web frontend only — a read-only exploration UI over the index
  JSON defined by spec 001 FR-5. No engine (Rust) changes, no changes to the
  search pipeline or the `app.js` mirror functions.
- **ID convention:** requirement IDs are globally unique across specs; this
  spec continues 001's `WEB-*` sequence at WEB-6.
- **See also:** [001](001-trigram-engine.md) for the index format;
  [docs/architecture.md](../docs/architecture.md) for the module map.

## Motivation

The search view (001 WEB-4) shows how a *query* is executed, but the index
itself stays a black box. This spec makes the two halves of the index
tangible: the deduplicated **document store** (browsable like a file tree)
and the **inverted index** (trigram → posting lists). Seeing the posting-list
size distribution also motivates Blackbird's sparse grams (001 §8 S-2): a few
"hot" grams have lists a large fraction of all docs long, which is exactly
the problem sparse grams exist to solve.

## Requirements

- **WEB-6 Explorer section.** Once any index is loaded (demo or fetched), an
  "Explore the index" section appears with two switchable views: **Files**
  and **Inverted index**. It is a pure view over the already-loaded FR-5
  JSON; it lives in its own file (`web/explorer.js`) so `web/app.js` remains
  exclusively the engine mirror.
- **WEB-7 Files view.** Doc paths render as a collapsible directory tree
  (native `<details>`/`<summary>`, styled like a file browser). Directories
  show an aggregate file count. Expanding a file reveals its attributes:
  doc id, content hash, size (bytes and lines), distinct-trigram count, and
  — when content-dedupe (001 FR-3) merged paths — every sibling path sharing
  the doc, labeled as duplicates.
- **WEB-8 Inverted-index view.**
  - Summary stats: distinct grams, total postings, average and max
    posting-list length.
  - A **histogram** of posting-list sizes (the long-tail shape is the
    teaching point) and a **top-30 grams** horizontal bar chart, both built
    with D3 and following the chart rules below.
  - A filter box: typing lists matching grams with their list lengths;
    clicking any gram (bar or list entry) expands its posting list as
    doc-path chips.
- **WEB-9 Web dependency rule.** D3 v7 is vendored as a single file at
  `web/vendor/d3.v7.min.js` (ISC license, copyright header retained). The
  no-build-step rule (001 §5) still holds — vendoring, not bundling. Web
  dependencies, like crates, require a spec change to add; the web allowlist
  is now: **d3**.

## Chart rules (apply to both WEB-8 charts)

Single-series magnitude charts: one hue (the site accent, `#0969da` light /
`#4493f8` dark — validated for lightness band, chroma, and ≥3:1 surface
contrast in both modes), no legend (the title names the series), thin bars
with a hairline gap, hover tooltip with exact values, axis/grid text in the
site's muted ink — never in the series color.

## Acceptance criteria

1. Load the demo index → explorer appears; Files shows this repo's tree;
   expanding `src/main.rs` shows id, hash, bytes, lines, trigram count.
2. The deduped fixture pair (`tests/fixture/dup1.txt` = `sub/dup2.txt`)
   renders as one doc with both paths visible at either location.
3. Inverted view: histogram + top-30 bars render; typing `arg` filters the
   gram list; clicking a gram shows its posting list as path chips whose
   count equals the gram's advertised list length.
4. No console errors; no page-level horizontal overflow at 393 px; both
   color schemes render correctly.
