# mini-bb — Specification

A miniature, educational re-implementation of the core ideas behind **Blackbird**,
GitHub's code search engine, in **under 500 lines of Rust** (code lines; teaching
comments excluded). It ingests a public GitHub repository, builds a trigram
inverted index, and answers substring queries — showing its work the way the
GitHub engineering blog does.

This document is the **source of truth**. Every implementation change must trace
to a numbered requirement here. Behavior changes require a spec change first
(same PR is fine, spec commit first). Requirements are numbered `FR-*`
(functional), `NF-*` (non-functional), `WEB-*` (frontend) so commits and PRs can
reference them.

Reference material:

- [The technology behind GitHub's new code search](https://github.blog/engineering/the-technology-behind-githubs-new-code-search/)
- [Anthropic / Claude Code best practices](https://code.claude.com/docs/en/best-practices)

---

## 1. Goals and non-goals

### Goals

- **G-1** Teach how an ngram code-search engine works: content-addressed
  ingestion, trigram inverted index, query → ngram expansion, lazy posting-list
  intersection, and false-positive verification.
- **G-2** Stay small enough to read in one sitting: **≤ 500 lines of Rust code**
  in `src/` (blank lines and `//` comment lines excluded — see NF-1).
- **G-3** Comment extensively, explicitly contrasting Rust paradigms with
  Python and C# (see NF-3).
- **G-4** Ship a tiny static web frontend on GitHub Pages that takes a repo URL
  and a query, and visualizes the query's expansion into ngram tokens and the
  search plan, mirroring the diagrams in the GitHub blog.

### Non-goals

- No sharding, no servers, no incremental/delta indexing, no ranking beyond
  trivial ordering, no symbol extraction, no regex engine (a tiny regex subset
  is a stretch goal, see §8).
- Not a production tool. Clarity beats performance everywhere they conflict.
- No support for private repos or authentication in the CLI.

---

## 2. Background: what we borrow from Blackbird

Blackbird's pipeline, and the piece of it each mini-bb component mimics:

| Blackbird concept | What Blackbird does | mini-bb equivalent |
|---|---|---|
| Content addressing | Shards & dedupes by git blob SHA; identical blobs indexed once | Dedupe files by content hash before indexing (FR-3) |
| Trigram ngram index | `"limits"` → `lim`, `imi`, `mit`, `its`; each gram → posting list of doc IDs | Same, at document granularity (FR-4) |
| Query planning | `/arguments?/` → `arg AND rgu AND gum AND (ume AND ment OR uments)` | Query → covering trigram set → AND plan (FR-6) |
| Lazy iterators | Intersects posting lists without materializing them | Sorted-list intersection via Rust iterators (FR-7) |
| Verification | Ngram hits are candidates; real content is scanned to kill false positives | Same: candidates re-scanned for the literal query (FR-8) |
| Sparse grams | Weighted bigrams pick variable-length grams to shrink hot posting lists | Documented + visualized only; stretch goal (§8) |

Key simplifications: we index whole documents (no positions in posting lists),
we hold one repo in one JSON file, and trigrams are 3 consecutive Unicode
scalar values rather than 3 bytes.

---

## 3. Functional requirements — indexing

- **FR-1 Ingest.** `mini-bb index <SOURCE> [-o index.json]` accepts either a
  public GitHub URL (`https://github.com/owner/repo`) or a local directory
  path. GitHub URLs are fetched with `git clone --depth 1` into a temp
  directory via `std::process::Command` (no git library dependency).
- **FR-2 File filtering.** Only UTF-8 text files ≤ 200 KB are indexed. Binary
  files (non-UTF-8), oversized files, and anything under `.git/` are skipped.
  Skips are counted and reported, not errors.
- **FR-3 Dedupe (content addressing).** Each file's content is hashed
  (FNV-1a 64-bit, implemented inline — ~6 lines, no dependency). Files with
  identical hashes share one document entry; all their paths are recorded on
  it. This mirrors Blackbird's dedupe-by-blob-SHA.
- **FR-4 Trigram extraction.** For each document, content is case-folded to
  lowercase and every window of 3 consecutive `char`s becomes a trigram.
  Each distinct trigram maps to a **sorted, deduplicated** list of doc IDs
  (the posting list).
- **FR-5 Index format.** The index serializes to JSON via serde:

  ```json
  {
    "version": 1,
    "source": "https://github.com/owner/repo",
    "docs": [
      { "id": 0, "paths": ["src/main.rs"], "hash": "a1b2c3…", "content": "…" }
    ],
    "grams": { "fn ": [0, 3, 7], "ain": [0] }
  }
  ```

  Document content is stored in the index. That makes the index self-contained
  for verification (FR-8) and lets the web frontend (§5) search and render
  snippets from a single fetched file. The `index` subcommand prints summary
  stats: docs indexed, duplicates merged, files skipped, distinct grams,
  index size.

## 4. Functional requirements — search

- **FR-6 Query parsing & expansion.** `mini-bb search <QUERY> [-i index.json]`
  parses the query into terms:
  - Bare words and `"quoted strings"` are literal terms.
  - Multiple terms are combined with **AND**.
  - Matching is **case-insensitive** (both index and query are case-folded;
    like Blackbird, correctness comes from the verification pass).
  - Each term expands to its covering trigram set (all windows of 3 chars).
    Terms shorter than 3 chars fall back to a full scan of all docs, with a
    printed warning (a real engine handles this differently; we show why the
    problem exists).
- **FR-7 Planning & intersection.** The plan is the AND of every trigram's
  posting list across all terms. Lists are intersected smallest-first over
  sorted doc-ID slices (lazy, iterator-based — no set allocation per list).
  The result is the **candidate set**.
- **FR-8 Verification.** Every candidate document's content is scanned for
  each literal term (case-folded `contains`). Only documents containing all
  terms are matches. This step exists because trigram hits are necessary but
  not sufficient — the same reason Blackbird verifies.
- **FR-9 Explainable output.** Search output shows the engine's work, in
  order: (1) parsed terms, (2) each term's trigram expansion, (3) the plan
  with each trigram's posting-list length, (4) candidate count before
  verification, (5) verified matches — path(s) plus the first matching line
  per term with the match highlighted. `--explain` stops after step 4 and
  runs no verification. Machine-readable `--json` output of the same
  structure is a stretch goal (§8).

## 5. Functional requirements — web frontend

A static site in `web/` (plain HTML + CSS + one vanilla JS file, no framework,
no build step), deployed to GitHub Pages by a workflow. The JS reimplements
the tokenizer/planner (~150 lines, mirrors the Rust logic; the Rust source is
the reference). Compiling the actual Rust core to WASM is a stretch goal (§8).

- **WEB-1 Inputs.** A repo URL field and a query field.
- **WEB-2 In-browser ingest.** Given a repo URL, fetch the file tree via
  `https://api.github.com/repos/{o}/{r}/git/trees/HEAD?recursive=1` and blob
  contents via the contents API (both CORS-enabled). Caps: ≤ 100 files,
  ≤ 100 KB per file, same text-only filter as FR-2. Show a clear warning about
  the 60 req/hour unauthenticated rate limit, with an optional
  personal-access-token field (kept in memory only, never stored).
- **WEB-3 Demo index.** A "load demo index" button fetches a prebuilt
  `web/demo/index.json` (generated by the CLI from this very repo and
  committed), so the demo works instantly with zero API calls.
- **WEB-4 Query visualization.** On search, render the same pipeline as FR-9
  as distinct visual stages: query → terms → trigram tokens (as chips, like
  the blog's `lim imi mit its` example) → plan with posting-list sizes →
  candidates → verified matches with highlighted snippets.
- **WEB-5 Deploy.** GitHub Actions workflow publishes `web/` to Pages on push
  to the default branch.

---

## 6. Architecture

```
mini-bb index URL ──clone──▶ ingest.rs ──docs──▶ index.rs ──▶ index.json
                                                                  │
mini-bb search Q ──▶ query.rs ──trigram plan──▶ search.rs ◀───────┘
                        │                          │
                        ▼                          ▼
                   expansion display      intersect → verify → results
```

Module layout with per-file **code-line** budgets (sum ≤ 500, checked by
`scripts/loc.sh`):

| File | Responsibility | Budget |
|---|---|---|
| `src/main.rs` | CLI definition (clap derive), wiring, output formatting | 90 |
| `src/lib.rs` | Shared types (`Index`, `Doc`), module decls, FNV hash | 60 |
| `src/ingest.rs` | Clone / walk / filter / read / dedupe (FR-1..FR-3) | 90 |
| `src/index.rs` | Trigram extraction, posting lists, (de)serialization (FR-4, FR-5) | 80 |
| `src/query.rs` | Parse terms, expand to trigrams, build plan (FR-6) | 80 |
| `src/search.rs` | Intersection, verification, snippet extraction (FR-7..FR-9) | 100 |

Dependencies (locked; adding one requires a spec change): `clap` (derive),
`serde` + `serde_json`. Both are chosen partly as teaching material — derive
macros are Rust's answer to Python decorators / C# attributes.

## 7. Non-functional requirements

- **NF-1 Line budget.** ≤ 500 code lines across `src/**/*.rs`. A line counts
  unless it is blank or its first non-whitespace characters are `//`. Only
  `//` comments are allowed (no `/* */`), so counting stays honest.
  `scripts/loc.sh` enforces this and CI fails when over budget.
- **NF-2 Quality gates.** `cargo test`, `cargo clippy -- -D warnings`,
  `cargo fmt --check`, and `scripts/loc.sh` must all pass; CI runs all four.
- **NF-3 Teaching comments.** Comments carry the pedagogy and are exempt from
  the budget. Paradigm contrasts use tagged form
  `// [TAG] explanation…` with tags: `OWNERSHIP`, `BORROWING`, `LIFETIMES`,
  `TRAITS`, `ENUMS+MATCH`, `ERRORS`, `ITERATORS`, `DERIVE`. Each tag must
  appear at least once in the codebase; prefer contrasting with the concrete
  Python/C# equivalent (e.g. `Result` vs exceptions, iterators vs LINQ,
  ownership vs GC).
- **NF-4 Testing.** Unit tests live in `#[cfg(test)]` modules and integration
  tests in `tests/` — neither counts toward the budget. The keystone test is a
  **golden oracle**: for a fixture directory and a set of queries, indexed
  search results must equal a naive full scan of every file. This is the
  runnable check that lets an agent verify its own work end-to-end.
- **NF-5 Errors.** Fallible functions return `Result`; the binary exits
  non-zero with a one-line message on failure. No `unwrap()` outside tests
  (`expect()` allowed only for invariants with a message saying why).

## 8. Stretch goals (explicitly out of v1)

Ordered; none may break NF-1.

1. **S-1 Regex subset**: support `?` (optional previous char) and `(a|b)`
   alternation on literals, so the blog's `/arguments?/` →
   `arg ∧ rgu ∧ gum ∧ ((ume ∧ ent) ∨ uments)` demo reproduces exactly.
2. **S-2 Sparse-gram visualizer**: web-only widget showing weighted bigrams
   and covering-gram selection for a typed string (no engine changes).
3. **S-3 `--json` search output** (FR-9 structure, machine-readable).
4. **S-4 WASM build** of the Rust core so the frontend runs the real engine.

## 9. Milestones & acceptance criteria

- **M0 — Spec & scaffolding** *(this document)*: repo builds (`cargo test`
  green on a stub), CLAUDE.md, spec, loc script, CI skeleton.
- **M1 — Index pipeline** (FR-1..FR-5): `mini-bb index` on this repo itself
  produces a valid `index.json`; dedupe demonstrated by a test with two
  identical files; stats printed.
- **M2 — Search pipeline** (FR-6..FR-9): golden-oracle test passes;
  `mini-bb search "fn main"` on the self-index finds `src/main.rs`;
  `--explain` shows expansion and plan; a query with a false-positive-only
  candidate is covered by a test proving verification removes it.
- **M3 — Web frontend** (WEB-1..WEB-5): Pages site live; demo index loads;
  typing a query shows the token/plan visualization; in-browser ingest works
  on a small public repo.
- **M4 — Polish**: comment-tag audit (NF-3 complete), README walkthrough,
  final budget check with headroom noted; stretch goals only if budget allows.

Each milestone lands as one PR referencing its requirement IDs, with the
relevant checks green.
