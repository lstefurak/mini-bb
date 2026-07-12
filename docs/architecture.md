# Architecture — how spec 001 is implemented

This document maps every concept in [spec 001](../spec/001-trigram-engine.md)
to the code that implements it. If you change the code, keep this map true;
if you change the map, change the spec first. Requirement IDs (`FR-*`, `NF-*`,
`WEB-*`) below refer to spec 001.

## The pipeline at a glance

```
 mini-bb index <url|path>                          mini-bb search <query>
 ────────────────────────                          ─────────────────────
        │                                                  │
        ▼                                                  ▼
 ┌──────────────┐   Vec<Doc>   ┌──────────────┐    ┌──────────────┐
 │  ingest.rs   ├─────────────▶│   index.rs   │    │   query.rs   │
 │ clone, walk, │              │ trigrams →   │    │ parse terms, │
 │ filter,      │              │ posting      │    │ expand regex │
 │ dedupe       │              │ lists → JSON │    │ subset →     │
 └──────────────┘              └──────┬───────┘    │ trigram plan │
                                      │            └──────┬───────┘
                                 index.json               │ Vec<TermPlan>
                                      │                   ▼
                                      │            ┌──────────────┐
                                      └───────────▶│  search.rs   │
                                                   │ intersect →  │
                                                   │ union →      │
                                                   │ verify       │
                                                   └──────┬───────┘
                                                          ▼
                                            main.rs: 5-stage display (FR-9)
```

The web frontend ([web/app.js](../web/app.js)) is a **line-for-line JS mirror**
of `index.rs`/`query.rs`/`search.rs` — see [Mirror contract](#the-mirror-contract).

## Module map

| File | Owns | Spec IDs | Key functions |
|---|---|---|---|
| [src/lib.rs](../src/lib.rs) | Shared types `Doc`, `Index`; `Result` alias; FNV-1a hash | FR-3, FR-5, NF-5 | `fnv1a` |
| [src/ingest.rs](../src/ingest.rs) | Getting files: shallow clone or local walk, text/size filter, content dedupe | FR-1, FR-2, FR-3 | `ingest`, `clone_shallow`, `walk`, `add_doc` |
| [src/index.rs](../src/index.rs) | Trigram extraction, posting lists, JSON (de)serialization | FR-4, FR-5 | `trigrams`, `build`, `save`, `load` |
| [src/query.rs](../src/query.rs) | Term parsing, regex-subset expansion into variants, trigram plan | FR-6 | `parse`, `expand`, `plan` |
| [src/search.rs](../src/search.rs) | Plan execution: intersect/union posting lists, verify, snippets | FR-7, FR-8, FR-9 | `candidates`, `term_candidates`, `intersect`, `union`, `verify` |
| [src/main.rs](../src/main.rs) | CLI (clap derive) and all output formatting | FR-1, FR-6, FR-9, NF-5 | `cmd_index`, `cmd_search`, `highlight` |
| [web/app.js](../web/app.js) | JS mirror of the engine + GitHub-API ingest + visualization | WEB-1..WEB-4 | mirrors named after their Rust counterparts |
| [.github/workflows/pages.yml](../.github/workflows/pages.yml) | Pages deploy of `web/` on push to main | WEB-5 | — |
| [scripts/loc.sh](../scripts/loc.sh) | Line-budget gate | NF-1 | — |
| [tests/golden.rs](../tests/golden.rs) | Golden oracle: engine ≡ naive scan | NF-4 | `naive_scan` |

## Data model (FR-5)

One repo → one self-contained JSON file:

```
Index
├── version: 1
├── source:  what was indexed (URL or path)
├── docs:    Vec<Doc>          ← deduplicated documents
│   ├── id:      position in `docs` (u32; doc IDs ARE indices)
│   ├── paths:   every path whose content hashed identically (FR-3)
│   ├── hash:    FNV-1a 64 of the content
│   └── content: full text — kept so verification (FR-8) and the web
│                demo (WEB-3) need nothing but this file
└── grams:   BTreeMap<String, Vec<u32>>   ← trigram → sorted doc IDs
```

Two properties everything downstream relies on:

1. **Posting lists are sorted and deduplicated.** `index.rs::build` inserts
   doc IDs in ascending order via a per-doc `BTreeSet`, which is what makes
   the two-pointer `intersect`/`union` in `search.rs` correct and O(n+m).
2. **`docs[i].id == i`.** `search.rs::verify` indexes `docs` directly with a
   candidate ID. Never reorder `docs` without renumbering.

## Query lifecycle (FR-6 → FR-9)

Worked example, the blog's own: `arguments?`

1. **Parse** (`query.rs::parse`): split on whitespace outside quotes, fold
   case → one bare term `arguments?`. Quoted terms skip step 2 entirely.
2. **Expand** (`query.rs::expand`): the regex subset — `?` optional previous
   char, `(a|b)` alternation, `\` escape — produces literal variants:
   `argument`, `arguments`.
3. **Plan** (`query.rs::plan`): each variant gets its covering trigrams
   (`index.rs::trigrams`, same function used at index time — that symmetry
   is the whole trick). Variants < 3 chars get `scan_all` instead.
4. **Gate** (`search.rs::candidates`): `AND(terms) of OR(variants) of
   AND(posting lists)`, smallest list first. Result: candidate doc IDs —
   *necessary but not sufficient* matches.
5. **Verify** (`search.rs::verify`): scan each candidate's real content for
   any variant literal per term; collect first-matching-line snippets.
   This kills trigram false positives (a doc with `abc` and `bcd` but no
   `abcd`) — same reason Blackbird verifies.
6. **Display** (`main.rs::cmd_search`): the five numbered stages (FR-9).
   `--explain` stops after stage 4.

## The mirror contract

`web/app.js` reimplements `trigrams`, `parse`, `expand`, `plan`,
`candidates`, `term_candidates`, `intersect`, `union`, and `verify` in JS,
with the same names and the same semantics. **The Rust side is the reference
implementation.** Any change to engine behavior lands in three places, in
this order:

1. spec 001 (the requirement),
2. the Rust module (+ tests),
3. the JS mirror in `web/app.js`.

The demo index (`web/demo/index.json`) is generated by the Rust CLI, so a
drifted mirror shows up as web results disagreeing with CLI results on the
same file — checking that one query by hand is the fastest drift test.

## Invariants that keep the project "as intended"

| Invariant | Enforced by |
|---|---|
| ≤ 500 code lines in `src/`, tests/comments free (NF-1) | `scripts/loc.sh`, CI |
| Only `//` comments, test module last in file | loc.sh counts on it |
| Teaching comments: all 8 `// [TAG]` paradigm tags present (NF-3) | M4 audit; re-check with `grep -r "\[TAG\]" src/` per tag |
| Engine ≡ naive scan on fixtures (NF-4) | `tests/golden.rs` golden oracle |
| No `unwrap()` outside tests; fallible fns return `Result` (NF-5) | review + clippy habits |
| clippy `-D warnings`, rustfmt clean (NF-2) | CI |
| Deps fixed to clap/serde/serde_json | spec 001 §6; adding one = spec change |
| Clarity over performance | it's a teaching codebase — resist cleverness |

## Extending the system

- **Behavior change to the engine** → amend spec 001 (spec commit first),
  implement in Rust, mirror in JS, extend the golden oracle with a query
  shape that exercises it.
- **New feature area** (e.g. sparse grams, WASM) → new numbered spec in
  [spec/](../spec/README.md), then implement against it. Candidates already
  scoped: spec 001 §8 stretch goals S-2..S-4.
- **Docs**: this file changes in the same PR as any change that moves a
  function, renames a module, or alters the pipeline shape.
