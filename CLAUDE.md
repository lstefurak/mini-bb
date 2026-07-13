# mini-bb

Educational trigram code-search engine (mini Blackbird) in ≤ 500 lines of Rust.
**spec/ is the source of truth** (currently `spec/001-trigram-engine.md`) —
read it before implementing anything. `docs/architecture.md` maps the spec to
the code; keep both true in the same PR as any structural change.

## Workflow (spec-driven)

- Every code change must trace to a `FR-*` / `NF-*` / `WEB-*` requirement in
  a spec; cite the IDs in commit messages.
- Behavior change ⇒ update the spec first (same PR, spec commit first).
  New feature area ⇒ new numbered spec (see spec/README.md).
- Engine changes land in three places, in order: spec → Rust (+ tests) →
  JS mirror in web/app.js.
- New dependencies require a spec change. Current allowlist: clap, serde,
  serde_json.

## Commands

- `cargo test` — unit + integration tests (golden oracle test is the key check)
- `cargo clippy -- -D warnings` and `cargo fmt --check` — must be clean
- `scripts/loc.sh` — line-budget gate; run it after every editing session
- Regenerate the demo index (delete first — it would otherwise index itself):
  `rm -f web/demo/index.json && cargo run -- index . -o web/demo/index.json`
- Kafka demo (spec 003): `docker compose -f kafka-demo/docker-compose.yml up -d`
  then `cargo run -p kafka-demo` (add `--record web/demo/kafka-events.jsonl`
  to refresh the web replay bundle); broker-backed test:
  `KAFKA_BROKER=localhost:9092 cargo test -p kafka-demo`

## Hard rules

- **Line budget:** ≤ 500 code lines in `src/` (blank + `//` lines excluded).
  Only `//` comments — never `/* */` (the counter depends on it). Tests
  (`#[cfg(test)]`, `tests/`) don't count.
- **Teaching comments:** contrast Rust with Python/C# using tagged comments
  `// [TAG] …` where TAG ∈ OWNERSHIP, BORROWING, LIFETIMES, TRAITS,
  ENUMS+MATCH, ERRORS, ITERATORS, DERIVE, ASYNC (spec 001 NF-3; ASYNC added
  by spec 003). Explain the
  paradigm, not the line ("what a reader coming from Python/C# would find
  surprising here").
- No `unwrap()` outside tests; `expect("why this can't fail")` only for real
  invariants. Fallible fns return `Result`.
- Clarity over performance; this is a teaching codebase.

## Layout

- `spec/` — numbered specs, source of truth (spec/README.md has the process)
- `docs/` — architecture: how the spec maps to the code
- `src/` — engine (budgeted; per-file budgets in spec 001 §6)
- `tests/` — integration tests + fixtures (unbudgeted)
- `web/` — static frontend for GitHub Pages, vanilla JS, no build step;
  app.js mirrors the Rust engine (Rust is the reference)
- `scripts/loc.sh` — budget counter used locally and in CI
