# spec/ — what to build, and why

This project is **spec-driven**: behavior is defined here first, implemented
second. If the code and a spec disagree, the spec wins — either fix the code
or change the spec (in that order of preference).

## The rules

1. **Every code change traces to a requirement ID** (`FR-*`, `NF-*`, `WEB-*`)
   in a spec. Commit messages cite the IDs.
2. **Behavior change ⇒ spec change first.** Same PR is fine, but the spec
   commit comes before the code commit.
3. **New dependencies require a spec change** — the allowlist lives in the
   spec that introduced it (001 §6).

## The specs

| Spec | Title | Status |
|---|---|---|
| [001](001-trigram-engine.md) | Trigram search engine (CLI + web) | Implemented |

## Adding a spec

New feature areas get a new numbered file: `NNN-short-name.md`, next number
in sequence. Start from this skeleton:

- **Header**: number, title, status (`draft` → `accepted` → `implemented`),
  scope.
- **Goals / non-goals** — what "done" means, and what is deliberately out.
- **Numbered requirements** — `FR-*` (functional), `NF-*` (non-functional),
  prefixed uniquely across all specs (e.g. spec 002 continues from where 001
  stopped or uses a `002.FR-1` style — pick one and note it in the spec).
- **Milestones & acceptance criteria** — each with a runnable check, so an
  agent (or human) can verify its own work.

Amendments to shipped behavior go in the spec that owns it (edit 001 rather
than writing "002: change 001"). Keep the history in git, not in the prose.

Related reading: [docs/architecture.md](../docs/architecture.md) explains how
the current implementation realizes spec 001; [CLAUDE.md](../CLAUDE.md) holds
the working rules for agents and contributors.
