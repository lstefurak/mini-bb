# Spec 003 — Kafka pipeline demo (distributed mini-bb)

- **Status:** implemented
- **Scope:** a new `kafka-demo` crate (≤ 300 code lines) + a web section
  visualizing it. The spec-001 engine is reused as a library, unchanged.
- **ID convention:** `DEMO-*` for the crate, `WEB-10+` continues the web
  sequence, `NF-6` continues the non-functional sequence.

## Motivation — what Blackbird actually uses Kafka for

From the GitHub blog: *"Kafka provides events that tell us to go index
something"*, and after crawling, *"we use Kafka, again, to allow each shard
to consume documents for indexing at its own pace… Each shard consumes a
single Kafka partition in the topic."* Documents are partitioned by git blob
SHA. Crucially, **queries do not travel through Kafka**: a coordinator
service *"fan[s] out requests to each host in the search cluster"*
synchronously. The demo teaches exactly that split: **async ingest via
Kafka, sync query via fan-out**.

## Requirements — the demo crate

- **DEMO-1 Crate & broker.** Workspace member `kafka-demo` (binary). It
  speaks the real Kafka protocol to a broker at `localhost:9092` (override
  with `KAFKA_BROKER`). `kafka-demo/docker-compose.yml` provides a
  single-node Redpanda broker. On startup the demo creates two topics:
  `pushes` (1 partition) and `docs` (3 partitions), tolerating
  already-exists.
- **DEMO-2 Ingest flow.** A push event published to `pushes` wakes the
  crawler, which ingests this repository with `mini_bb::ingest` (dedupe and
  filtering included) and produces one JSON document message to `docs` per
  doc. The partition is `fnv1a(content) % 3` — content addressing decides
  the shard, mirroring Blackbird's blob-SHA partitioning.
- **DEMO-3 Shards.** Three consumer tasks; shard *i* fetches **only
  partition *i*** (explicit assignment — the point is "each shard consumes a
  single Kafka partition"). Each shard accumulates its docs and builds its
  own `mini_bb::Index` with the unmodified spec-001 engine.
- **DEMO-4 Query fan-out (not Kafka).** `GET /search?q=` parses and plans
  the query once (`mini_bb::query`), fans out to all three shards, runs
  candidates + verify per shard (`mini_bb::search`), and merges the results.
  The response includes per-shard results and the merged list.
- **DEMO-5 Observability.** Every pipeline step (push, crawl, produce → 
  partition, shard indexed, query fan-out, shard result, merge) emits a JSON
  event on `GET /events` (server-sent events, CORS `*`). `--record <file>`
  additionally appends each event as a JSON line, producing the replay
  bundle for WEB-11.
- **DEMO-6 HTTP surface.** Port 7878: `GET /push` (publish a push event),
  `GET /search?q=`, `GET /events` (SSE). All responses carry
  `Access-Control-Allow-Origin: *` so the GitHub Pages site can talk to a
  locally running demo.

## Requirements — the web section

- **WEB-10 Pipeline visualization.** A new section renders the pipeline as
  an animated diagram (D3, WEB-9 vendor): push event → crawler → `docs`
  topic partitions p0–p2 → shards s0–s2 with live doc/trigram counters;
  a query node fanning out to the shards and a merge node collecting
  results. Messages animate as moving dots; the query path is visually
  distinct from the ingest path (sync vs async is the lesson).
- **WEB-11 Two data sources.**
  - **Replay** (default, works on the static Pages site): plays
    `web/demo/kafka-events.jsonl`, a recording of a real run against a real
    broker (DEMO-5).
  - **Live**: connects `EventSource` to `http://localhost:7878/events`; the
    section's push/search controls call the local demo. (Browsers exempt
    `localhost` from mixed-content blocking, so this works from the HTTPS
    Pages site in Chrome/Firefox.)

## Non-functional

- **NF-6 Budget.** `kafka-demo/src` ≤ **300** code lines, counted exactly
  like NF-1 (blank + `//` excluded, tests excluded, `//` comments only).
  `scripts/loc.sh` enforces both budgets; the engine's 500 stays unchanged.
- **Dependencies (kafka-demo only; engine allowlist unchanged):**
  `mini-bb` (path), `rskafka` (pure-Rust Kafka client — its
  partition-client API *is* the teaching model), `tokio`, `chrono`
  (rskafka's record timestamps), `serde`, `serde_json`.
- **Teaching comments** follow NF-3 conventions where paradigms appear
  (async/await vs Python asyncio / C# Tasks is the natural new tag —
  `// [ASYNC]` is added to the NF-3 tag set by this spec).
- **CI** builds and lints the whole workspace. The broker-dependent
  integration test self-skips when `KAFKA_BROKER` is unset so CI needs no
  broker.

## Acceptance criteria

1. With the compose broker up: `cargo run -p kafka-demo` then `GET /push`
   → events show crawl, per-partition produces, and three shards whose doc
   counts sum to the ingest doc count.
2. **Sharding preserves results:** `GET /search?q=fn+main` returns exactly
   the same merged path set as a monolithic spec-001 index of the same tree
   — asserted by an integration test run against the real broker.
3. A recorded run replays in the web section with no broker (Pages mode).
4. Live mode animates a push and a search end-to-end; no console errors;
   no horizontal overflow at 393 px.
