// Index explorer (spec 002, WEB-6..WEB-8): two read-only views over the
// FR-5 index JSON — a file-browser view of the deduplicated doc store, and
// an inverted-index view (D3 charts + gram lookup). This module is NOT part
// of the Rust↔JS mirror; it only *reads* the index shape.
// Exposes exactly one global: window.renderExplorer(index).

"use strict";

(() => {
  const $ = (id) => document.getElementById(id);
  const escx = (s) =>
    String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  // Grams can contain whitespace; make it visible in labels.
  const visGram = (g) => g.replaceAll(" ", "·").replaceAll("\n", "⏎").replaceAll("\t", "⇥");
  const showGram = (g) => escx(visGram(g)); // HTML-safe variant

  let IDX = null;
  let gramCountPerDoc = [];

  // ------------------------------------------------------------- files --
  // Build a nested directory tree from every path of every doc (WEB-7).
  function buildTree(docs) {
    const root = { dirs: new Map(), files: [] };
    for (const doc of docs) {
      for (const path of doc.paths) {
        const parts = path.split("/");
        let node = root;
        for (const part of parts.slice(0, -1)) {
          if (!node.dirs.has(part)) node.dirs.set(part, { dirs: new Map(), files: [] });
          node = node.dirs.get(part);
        }
        node.files.push({ name: parts[parts.length - 1], path, doc });
      }
    }
    return root;
  }

  function countFiles(node) {
    let n = node.files.length;
    for (const d of node.dirs.values()) n += countFiles(d);
    return n;
  }

  function fileAttrs(f) {
    const { doc } = f;
    const bytes = new TextEncoder().encode(doc.content).length;
    const lines = doc.content.split("\n").length;
    const dups = doc.paths.filter((p) => p !== f.path);
    return `<table class="attrs">
      <tr><th>doc id</th><td>${doc.id}</td></tr>
      <tr><th>content hash</th><td><code>${escx(doc.hash)}</code></td></tr>
      <tr><th>size</th><td>${bytes.toLocaleString()} bytes · ${lines.toLocaleString()} lines</td></tr>
      <tr><th>distinct trigrams</th><td>${(gramCountPerDoc[doc.id] ?? 0).toLocaleString()}</td></tr>
      ${dups.length
        ? `<tr><th>deduped with</th><td>${dups.map((p) => `<code>${escx(p)}</code>`).join("<br>")}
           <span class="dim">(identical content — indexed once, FR-3)</span></td></tr>`
        : ""}
    </table>`;
  }

  function renderNode(node) {
    let html = "";
    for (const name of [...node.dirs.keys()].sort()) {
      const dir = node.dirs.get(name);
      const n = countFiles(dir);
      html += `<details class="tree dir"><summary>${escx(name)}/
        <span class="dim">${n} file${n === 1 ? "" : "s"}</span></summary>${renderNode(dir)}</details>`;
    }
    for (const f of [...node.files].sort((a, b) => a.name.localeCompare(b.name))) {
      const dup = f.doc.paths.length > 1 ? ` <span class="dup" title="content shared with another path">=</span>` : "";
      html += `<details class="tree file"><summary>${escx(f.name)}${dup}
        <span class="dim">doc ${f.doc.id}</span></summary>${fileAttrs(f)}</details>`;
    }
    return html;
  }

  // ------------------------------------------------------------- charts --
  const BAR = "var(--chart-bar)"; // validated for both themes (spec 002)

  function tooltip(html, event) {
    const t = $("exp-tooltip");
    if (!html) { t.hidden = true; return; }
    t.innerHTML = html;
    t.hidden = false;
    const pad = 12;
    const w = t.offsetWidth;
    const x = Math.min(event.pageX + pad, document.documentElement.clientWidth - w - pad);
    t.style.left = `${Math.max(pad, x)}px`;
    t.style.top = `${event.pageY + pad}px`;
  }

  // Histogram of posting-list sizes (WEB-8): x = docs per gram, y = grams.
  function renderHist(lens) {
    const el = $("exp-hist");
    el.innerHTML = "";
    const width = Math.max(el.clientWidth || 320, 280);
    const height = 180;
    const m = { top: 8, right: 12, bottom: 34, left: 56 };
    const bins = d3.bin().domain([1, d3.max(lens) + 1]).thresholds(Math.min(24, d3.max(lens)))(lens);
    const x = d3.scaleLinear().domain([1, d3.max(lens) + 1]).range([m.left, width - m.right]);
    const y = d3.scaleLinear().domain([0, d3.max(bins, (b) => b.length)]).nice()
      .range([height - m.bottom, m.top]);

    const svg = d3.select(el).append("svg").attr("width", width).attr("height", height);
    svg.append("g").attr("class", "axis")
      .attr("transform", `translate(0,${height - m.bottom})`)
      .call(d3.axisBottom(x).ticks(6).tickSizeOuter(0));
    svg.append("g").attr("class", "axis")
      .attr("transform", `translate(${m.left},0)`)
      .call(d3.axisLeft(y).ticks(4).tickSizeOuter(0));
    svg.append("text").attr("class", "axis-label")
      .attr("x", (m.left + width - m.right) / 2).attr("y", height - 4)
      .text("posting-list length (docs containing the gram)");
    svg.append("text").attr("class", "axis-label")
      .attr("transform", `translate(11,${(m.top + height - m.bottom) / 2}) rotate(-90)`)
      .text("grams");

    svg.append("g").selectAll("rect").data(bins).join("rect")
      .attr("x", (b) => x(b.x0) + 1)
      .attr("width", (b) => Math.max(1, x(b.x1) - x(b.x0) - 2))
      .attr("y", (b) => y(b.length))
      .attr("height", (b) => y(0) - y(b.length))
      .attr("rx", 2)
      .style("fill", BAR)
      .on("mousemove", (ev, b) => tooltip(
        `<b>${b.length.toLocaleString()}</b> grams appear in ${b.x0}–${b.x1 - 1} docs`, ev))
      .on("mouseleave", () => tooltip(null));
  }

  // Top-N grams by posting-list length (WEB-8), horizontal bars.
  function renderTop(entries) {
    const el = $("exp-top");
    el.innerHTML = "";
    const top = entries.slice(0, 30);
    const width = Math.max(el.clientWidth || 320, 280);
    const rowH = 18;
    const m = { top: 4, right: 46, bottom: 4, left: 52 };
    const height = m.top + m.bottom + top.length * rowH;
    const x = d3.scaleLinear().domain([0, top[0][1].length]).range([0, width - m.left - m.right]);

    const svg = d3.select(el).append("svg").attr("width", width).attr("height", height);
    const row = svg.append("g").selectAll("g").data(top).join("g")
      .attr("transform", (_, i) => `translate(${m.left},${m.top + i * rowH})`)
      .attr("class", "gram-row")
      .style("cursor", "pointer")
      .on("mousemove", (ev, [g, list]) => tooltip(
        `<code>${showGram(g)}</code> appears in <b>${list.length}</b> of ${IDX.docs.length} docs — click for the posting list`, ev))
      .on("mouseleave", () => tooltip(null))
      .on("click", (_, [g]) => showPostings(g));
    row.append("text").attr("class", "gram-label")
      .attr("x", -6).attr("y", rowH / 2).attr("dy", "0.35em").attr("text-anchor", "end")
      .text(([g]) => visGram(g));
    row.append("rect")
      .attr("y", 2).attr("height", rowH - 4).attr("rx", 2)
      .attr("width", ([, list]) => Math.max(1, x(list.length)))
      .style("fill", BAR);
    row.append("text").attr("class", "bar-value")
      .attr("x", ([, list]) => x(list.length) + 5).attr("y", rowH / 2).attr("dy", "0.35em")
      .text(([, list]) => list.length);
  }

  // ------------------------------------------------------ gram lookup --
  function showPostings(gram) {
    const list = IDX.grams[gram] ?? [];
    $("gram-detail").innerHTML =
      `<h3>posting list for <span class="chip gram">${showGram(gram)}</span> — ${list.length} docs</h3>` +
      list.map((id) => `<span class="chip path">${escx(IDX.docs[id].paths.join(" = "))}</span>`).join(" ");
    $("gram-detail").scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  function renderFilter() {
    const q = $("gram-filter").value.toLowerCase();
    const el = $("gram-list");
    if (!q) { el.innerHTML = ""; return; }
    const hits = Object.entries(IDX.grams).filter(([g]) => g.includes(q)).slice(0, 60);
    el.innerHTML = hits.length
      ? hits.map(([g, list]) =>
          `<button class="chip gram clickable" data-gram="${escx(g)}">${showGram(g)}<b>${list.length}</b></button>`).join(" ")
      : `<span class="dim">no grams match</span>`;
    for (const b of el.querySelectorAll("[data-gram]")) {
      b.addEventListener("click", () => showPostings(b.dataset.gram));
    }
  }

  // -------------------------------------------------------------- wire --
  function renderGrams() {
    const entries = Object.entries(IDX.grams).sort((a, b) => b[1].length - a[1].length);
    const lens = entries.map(([, list]) => list.length);
    const total = d3.sum(lens);
    $("gram-stats").textContent =
      `${entries.length.toLocaleString()} distinct trigrams · ${total.toLocaleString()} postings · ` +
      `average list ${(total / entries.length).toFixed(1)} docs · longest ${lens[0]} of ${IDX.docs.length} docs`;
    renderHist(lens);
    renderTop(entries);
  }

  window.renderExplorer = (index) => {
    IDX = index;
    // Distinct-trigram count per doc, derived from the inverted index itself.
    gramCountPerDoc = new Array(index.docs.length).fill(0);
    for (const list of Object.values(index.grams)) for (const id of list) gramCountPerDoc[id]++;
    $("view-files").innerHTML = renderNode(buildTree(index.docs));
    $("gram-detail").innerHTML = "";
    $("gram-list").innerHTML = "";
    $("gram-filter").value = "";
    if (!$("view-grams").hidden) renderGrams();
    $("explore-panel").hidden = false;
  };

  function selectTab(which) {
    $("view-files").hidden = which !== "files";
    $("view-grams").hidden = which !== "grams";
    $("tab-files").classList.toggle("active", which === "files");
    $("tab-grams").classList.toggle("active", which === "grams");
    if (which === "grams" && IDX) renderGrams(); // charts need visible widths
  }
  $("tab-files").addEventListener("click", () => selectTab("files"));
  $("tab-grams").addEventListener("click", () => selectTab("grams"));
  $("gram-filter").addEventListener("input", renderFilter);
  let resizeTimer;
  window.addEventListener("resize", () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => { if (IDX && !$("view-grams").hidden) renderGrams(); }, 150);
  });
})();
