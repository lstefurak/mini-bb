// Kafka pipeline visualization (spec 003, WEB-10/WEB-11): animates real
// recorded (or live) pipeline events — push → crawler → docs topic
// partitions → shards, and the query fan-out that deliberately bypasses
// Kafka. Data sources: web/demo/kafka-events.jsonl (replay, recorded from a
// real broker run) or a local kafka-demo at http://localhost:7878 (live).
// View-only; exposes no globals.

"use strict";

(() => {
  const $ = (id) => document.getElementById(id);
  const esc = (s) =>
    String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  const LIVE_URL = "http://localhost:7878";
  const INGEST = "var(--chart-bar)"; // async path (validated, both themes)
  const QUERY = "var(--chart-query)"; // sync path (validated, both themes)

  // ---------------------------------------------------------- the diagram --
  // Fixed layout in a 760×330 viewBox; nodes are (x, y, w, h).
  const N = {
    push: [10, 40, 96, 36, "push event"],
    crawler: [160, 40, 96, 36, "crawler"],
    p0: [310, 16, 130, 40, "docs · p0"],
    p1: [310, 72, 130, 40, "docs · p1"],
    p2: [310, 128, 130, 40, "docs · p2"],
    s0: [520, 10, 228, 52, "shard 0"],
    s1: [520, 66, 228, 52, "shard 1"],
    s2: [520, 122, 228, 52, "shard 2"],
    query: [10, 250, 96, 36, "query"],
    merge: [310, 250, 130, 40, "merge"],
  };
  const mid = (k) => [N[k][0] + N[k][2] / 2, N[k][1] + N[k][3] / 2];
  const right = (k) => [N[k][0] + N[k][2], N[k][1] + N[k][3] / 2];
  const left = (k) => [N[k][0], N[k][1] + N[k][3] / 2];

  let svg;
  function initDiagram() {
    svg = d3
      .select("#pipe-diagram")
      .append("svg")
      .attr("viewBox", "0 0 760 330")
      .attr("preserveAspectRatio", "xMidYMid meet");
    const edges = [
      ["push", "crawler", INGEST, ""],
      ["crawler", "p0", INGEST, ""], ["crawler", "p1", INGEST, ""], ["crawler", "p2", INGEST, ""],
      ["p0", "s0", INGEST, ""], ["p1", "s1", INGEST, ""], ["p2", "s2", INGEST, ""],
      ["query", "s0", QUERY, "4 3"], ["query", "s1", QUERY, "4 3"], ["query", "s2", QUERY, "4 3"],
      ["s0", "merge", QUERY, "4 3"], ["s1", "merge", QUERY, "4 3"], ["s2", "merge", QUERY, "4 3"],
    ];
    for (const [a, b, , dash] of edges) {
      const [x1, y1] = right(a);
      const [x2, y2] = left(b);
      svg.append("line").attr("class", "pipe-edge").attr("stroke-dasharray", dash)
        .attr("x1", x1).attr("y1", y1).attr("x2", x2).attr("y2", y2);
    }
    for (const [key, [x, y, w, h, label]] of Object.entries(N)) {
      const g = svg.append("g");
      g.append("rect").attr("class", `pipe-node ${/^s\d/.test(key) ? "pipe-shard" : ""}`)
        .attr("x", x).attr("y", y).attr("width", w).attr("height", h).attr("rx", 8);
      g.append("text").attr("class", "pipe-label").attr("x", x + 10).attr("y", y + 16).text(label);
      g.append("text").attr("class", "pipe-meta").attr("id", `pipe-meta-${key}`)
        .attr("x", x + 10).attr("y", y + (h > 40 ? 34 : 30)).text("");
    }
    // The lesson, written on the diagram itself.
    svg.append("text").attr("class", "pipe-meta").attr("x", 10).attr("y", 205)
      .text("solid = async ingest (real Kafka messages)");
    svg.append("text").attr("class", "pipe-meta").attr("x", 10).attr("y", 222)
      .text("dashed = sync query fan-out (no Kafka)");
  }

  const meta = (key, text) => svg.select(`#pipe-meta-${key}`).text(text);

  function dot(a, b, color, dur = 550) {
    const [x1, y1] = right(a);
    const [x2, y2] = left(b);
    svg.append("circle").attr("class", "pipe-dot").attr("r", 5).style("fill", color)
      .attr("cx", x1).attr("cy", y1)
      .transition().duration(dur).ease(d3.easeQuadInOut)
      .attr("cx", x2).attr("cy", y2)
      .remove();
  }

  // --------------------------------------------------------- event router --
  const partCount = [0, 0, 0];
  function handle(ev) {
    switch (ev.ev) {
      case "push":
        dot("push", "crawler", INGEST);
        break;
      case "crawl":
        meta("crawler", `${ev.docs} docs (${ev.merged} deduped)`);
        break;
      case "produce":
        partCount[ev.partition]++;
        dot("crawler", `p${ev.partition}`, INGEST);
        meta(`p${ev.partition}`, `${partCount[ev.partition]} msgs`);
        break;
      case "indexed":
        dot(`p${ev.shard}`, `s${ev.shard}`, INGEST);
        meta(`s${ev.shard}`, `${ev.docs} docs · ${ev.grams.toLocaleString()} trigrams`);
        break;
      case "fanout":
        meta("query", `“${ev.q}”`);
        meta("merge", "");
        for (const s of ["s0", "s1", "s2"]) dot("query", s, QUERY);
        break;
      case "shard_result":
        dot(`s${ev.shard}`, "merge", QUERY);
        break;
      case "merged":
        meta("merge", `${ev.total} results`);
        $("pipe-log").innerHTML =
          `<span class="chip term">${esc(ev.q)}</span> → <b>${ev.total}</b> merged results` +
          `<span class="dim"> — fan-out answered by all shards</span>`;
        break;
    }
  }

  // -------------------------------------------------------------- replay --
  let replayTimers = [];
  async function replay() {
    stopLive();
    replayTimers.forEach(clearTimeout);
    replayTimers = [];
    partCount.fill(0);
    ["crawler", "p0", "p1", "p2", "s0", "s1", "s2", "query", "merge"].forEach((k) => meta(k, ""));
    $("pipe-status").textContent = "replaying a recorded run against a real broker…";
    let lines;
    try {
      const res = await fetch("demo/kafka-events.jsonl");
      lines = (await res.text()).trim().split("\n").map((l) => JSON.parse(l));
    } catch {
      $("pipe-status").textContent = "could not load the recorded run";
      return;
    }
    // Real timestamps, but gaps compressed to keep the story watchable:
    // ≥120 ms between events, ≤900 ms per gap.
    let t = 300;
    lines.forEach((ev, i) => {
      const dt = i ? Math.min(900, Math.max(120, ev.ts - lines[i - 1].ts)) : 0;
      t += dt;
      replayTimers.push(setTimeout(() => handle(ev), t));
    });
    replayTimers.push(setTimeout(() => {
      $("pipe-status").textContent = "replay finished — every dot was a real Kafka message (or fan-out call)";
    }, t + 700));
  }

  // ---------------------------------------------------------------- live --
  let es = null;
  function stopLive() {
    if (es) es.close();
    es = null;
    $("pipe-live-controls").hidden = true;
  }
  function connectLive() {
    replayTimers.forEach(clearTimeout);
    stopLive();
    $("pipe-status").textContent = `connecting to ${LIVE_URL}…`;
    es = new EventSource(`${LIVE_URL}/events`);
    es.onopen = () => {
      $("pipe-status").textContent = "live — connected to the local kafka-demo";
      $("pipe-live-controls").hidden = false;
    };
    es.onmessage = (m) => handle(JSON.parse(m.data));
    es.onerror = () => {
      $("pipe-status").textContent =
        "no local demo found — run: docker compose -f kafka-demo/docker-compose.yml up -d && cargo run -p kafka-demo";
      stopLive();
    };
  }
  async function livePush() {
    try { await fetch(`${LIVE_URL}/push`); } catch { /* surfaced via SSE silence */ }
  }
  async function liveSearch() {
    const q = $("pipe-query").value.trim();
    if (!q) return;
    try {
      const res = await fetch(`${LIVE_URL}/search?q=${encodeURIComponent(q)}`);
      const r = await res.json();
      $("pipe-log").innerHTML =
        `<span class="chip term">${esc(r.q)}</span> → <b>${r.merged.length}</b> merged results<br>` +
        r.merged.map((p) => `<span class="chip path">${esc(p)}</span>`).join(" ");
    } catch {
      $("pipe-log").textContent = "search failed — is the local demo running?";
    }
  }

  // ---------------------------------------------------------------- wire --
  initDiagram();
  $("pipe-replay").addEventListener("click", replay);
  $("pipe-live").addEventListener("click", connectLive);
  $("pipe-push").addEventListener("click", livePush);
  $("pipe-search").addEventListener("click", liveSearch);
  $("pipe-query").addEventListener("keydown", (e) => { if (e.key === "Enter") liveSearch(); });
  replay(); // auto-play the recorded run on page load
})();
