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

- **[spec/](spec/README.md)** — the numbered specs this project is driven by
  (start with [001](spec/001-trigram-engine.md))
- **[docs/architecture.md](docs/architecture.md)** — how the spec maps to the
  code: pipeline, module map, invariants
- **[CLAUDE.md](CLAUDE.md)** — agent/contributor working rules
- **[Live demo](https://lstefurak.github.io/mini-bb/)** — GitHub Pages frontend
  (`web/`): paste a small public repo URL or load the prebuilt demo index,
  and watch queries expand into trigram tokens, gate on posting lists, and
  get verified — the pipeline from the blog, visualized.

The source is deliberately over-commented: `// [TAG]` comments contrast Rust
paradigms (ownership, `Result`, traits, iterators, derive macros…) with their
Python and C# equivalents, for readers coming from those languages.

Try the query `arguments?` — it expands to `argument ∨ arguments`, the same
optional-suffix plan the GitHub blog uses as its worked example.

There's also a **distributed mode** (`kafka-demo/`, spec 003): real Kafka
messages carry documents to three shards — each consuming one partition,
Blackbird-style — and queries fan out synchronously and merge. The live demo
page replays a recorded run, or connects to your local broker:

```
docker compose -f kafka-demo/docker-compose.yml up -d
cargo run -p kafka-demo   # then hit "Connect to local demo" on the site
```

Status: **all milestones (M0–M4) complete**, including stretch goal S-1
(the `?` / `(a|b)` regex subset). Budget: 490/500 code lines. Remaining
stretch goals (spec 001 §8): sparse-gram visualizer, `--json` output, WASM.
