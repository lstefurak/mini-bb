# mini-bb

A miniature, heavily-commented re-implementation of the core ideas behind
[Blackbird](https://github.blog/engineering/the-technology-behind-githubs-new-code-search/),
GitHub's code search engine — in **under 500 lines of Rust**.

Point it at a public GitHub repo, get a searchable trigram index, and watch it
show its work: query → terms → trigram tokens → posting-list plan → candidates
→ verified matches.

```
mini-bb index https://github.com/owner/repo -o index.json
mini-bb search "fn main" -i index.json
```

- **[SPEC.md](SPEC.md)** — the spec this project is driven by (start here)
- **[CLAUDE.md](CLAUDE.md)** — agent/contributor working rules
- `web/` — tiny GitHub Pages frontend visualizing query expansion (milestone M3)

The source is deliberately over-commented: `// [TAG]` comments contrast Rust
paradigms (ownership, `Result`, traits, iterators, derive macros…) with their
Python and C# equivalents, for readers coming from those languages.

Status: **M0 — spec & scaffolding.** See SPEC.md §9 for the milestone plan.
