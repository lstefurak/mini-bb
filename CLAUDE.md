# mini-bb

Educational trigram code-search engine (mini Blackbird) in ≤ 500 lines of Rust.
**SPEC.md is the source of truth** — read it before implementing anything.

## Workflow (spec-driven)

- Every code change must trace to a `FR-*` / `NF-*` / `WEB-*` requirement in
  SPEC.md; cite the IDs in commit messages.
- Behavior change ⇒ update SPEC.md first (same PR, spec commit first).
- New dependencies require a spec change. Current allowlist: clap, serde,
  serde_json.
- Work lands milestone-by-milestone (SPEC.md §9), one PR per milestone.

## Commands

- `cargo test` — unit + integration tests (golden oracle test is the key check)
- `cargo clippy -- -D warnings` and `cargo fmt --check` — must be clean
- `scripts/loc.sh` — line-budget gate; run it after every editing session

## Hard rules

- **Line budget:** ≤ 500 code lines in `src/` (blank + `//` lines excluded).
  Only `//` comments — never `/* */` (the counter depends on it). Tests
  (`#[cfg(test)]`, `tests/`) don't count.
- **Teaching comments:** contrast Rust with Python/C# using tagged comments
  `// [TAG] …` where TAG ∈ OWNERSHIP, BORROWING, LIFETIMES, TRAITS,
  ENUMS+MATCH, ERRORS, ITERATORS, DERIVE (see SPEC.md NF-3). Explain the
  paradigm, not the line ("what a reader coming from Python/C# would find
  surprising here").
- No `unwrap()` outside tests; `expect("why this can't fail")` only for real
  invariants. Fallible fns return `Result`.
- Clarity over performance; this is a teaching codebase.

## Layout

- `src/` — engine (budgeted; per-file budgets in SPEC.md §6)
- `tests/` — integration tests + fixtures (unbudgeted)
- `web/` — static frontend for GitHub Pages, vanilla JS, no build step
- `scripts/loc.sh` — budget counter used locally and in CI
