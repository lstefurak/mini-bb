// mini-bb web frontend (SPEC.md WEB-1..WEB-4).
// This file MIRRORS the Rust engine — src/index.rs, src/query.rs,
// src/search.rs are the reference implementation. Keep the algorithms in
// lockstep: same trigram definition, same plan, same verify semantics.

"use strict";

let INDEX = null; // the loaded/built index, same JSON shape as FR-5

const $ = (id) => document.getElementById(id);
const status = (msg, isError = false) => {
  $("status").textContent = msg;
  $("status").classList.toggle("error", isError);
};

// ---------------------------------------------------------------- engine --
// Mirror of index.rs::trigrams — every window of 3 chars, case-folded.
// Array.from splits by code point (like Rust chars()), not UTF-16 units.
function trigrams(text) {
  const chars = Array.from(text.toLowerCase());
  const out = [];
  for (let i = 0; i + 3 <= chars.length; i++) {
    out.push(chars[i] + chars[i + 1] + chars[i + 2]);
  }
  return out;
}

// Mirror of query.rs::parse — bare words + "quoted strings", folded.
function parseQuery(query) {
  const terms = [];
  let cur = "";
  let inQuotes = false;
  const flush = () => {
    if (cur) terms.push(cur.toLowerCase());
    cur = "";
  };
  for (const c of query) {
    if (c === '"') { inQuotes = !inQuotes; flush(); }
    else if (/\s/.test(c) && !inQuotes) flush();
    else cur += c;
  }
  flush();
  return terms;
}

// Mirror of query.rs::plan.
function plan(terms) {
  return terms.map((term) => {
    const grams = trigrams(term);
    return { term, grams, scanAll: grams.length === 0 };
  });
}

// Mirror of search.rs::candidates — smallest-first sorted intersection.
function candidates(index, plans) {
  const lists = [];
  for (const p of plans)
    for (const g of p.grams) lists.push(index.grams[g] ?? []);
  if (lists.length === 0) return index.docs.map((d) => d.id);
  lists.sort((a, b) => a.length - b.length);
  let acc = lists[0];
  for (const l of lists.slice(1)) {
    acc = intersect(acc, l);
    if (acc.length === 0) break;
  }
  return acc;
}

// Mirror of search.rs::intersect — two-pointer merge of sorted ID lists.
function intersect(a, b) {
  const out = [];
  let i = 0, j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] < b[j]) i++;
    else if (a[i] > b[j]) j++;
    else { out.push(a[i]); i++; j++; }
  }
  return out;
}

// Mirror of search.rs::verify — trigram hits are candidates, not matches.
function verify(index, plans, ids) {
  const matches = [];
  for (const id of ids) {
    const doc = index.docs[id];
    const folded = doc.content.toLowerCase();
    if (!plans.every((p) => folded.includes(p.term))) continue;
    const lines = [];
    const foldedLines = folded.split("\n");
    const origLines = doc.content.split("\n");
    for (const p of plans) {
      const n = foldedLines.findIndex((l) => l.includes(p.term));
      if (n >= 0) lines.push({ term: p.term, lineNo: n + 1, text: origLines[n] });
    }
    matches.push({ doc, lines });
  }
  return matches;
}

// ------------------------------------------------------------- rendering --
const esc = (s) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function highlight(text, term) {
  const i = text.toLowerCase().indexOf(term);
  if (i < 0) return esc(text);
  return (
    esc(text.slice(0, i)) +
    "<mark>" + esc(text.slice(i, i + term.length)) + "</mark>" +
    esc(text.slice(i + term.length))
  );
}

function stage(title, bodyHtml) {
  return `<div class="stage"><h3>${title}</h3><div>${bodyHtml}</div></div>`;
}

// The FR-9 / WEB-4 pipeline: terms → expansion → plan → candidates → matches.
function runSearch() {
  if (!INDEX) return;
  const terms = parseQuery($("query").value);
  const el = $("stages");
  if (terms.length === 0) { el.innerHTML = ""; return; }
  const plans = plan(terms);

  let html = stage("terms (AND)", terms.map((t) => `<span class="chip term">${esc(t)}</span>`).join(" "));

  html += stage("expand each term into covering trigrams",
    plans.map((p) =>
      `<div class="expansion"><span class="chip term">${esc(p.term)}</span> → ` +
      (p.scanAll
        ? `<span class="warn">shorter than a trigram — full scan of every doc!</span>`
        : p.grams.map((g) => `<span class="chip gram">${esc(g)}</span>`).join(" ")) +
      `</div>`).join(""));

  html += stage("plan — AND over posting lists <span class='dim'>(chip badge = list length)</span>",
    plans.flatMap((p) => p.grams).map((g) => {
      const n = (INDEX.grams[g] ?? []).length;
      return `<span class="chip gram">${esc(g)}<b>${n}</b></span>`;
    }).join(" ∧ ") || `<span class="warn">no trigrams to gate on</span>`);

  const cand = candidates(INDEX, plans);
  html += stage("candidates after intersection",
    `<b>${cand.length}</b> of ${INDEX.docs.length} docs <span class="dim">— necessary, not sufficient: now verify</span>`);

  const matches = verify(INDEX, plans, cand);
  const removed = cand.length - matches.length;
  html += stage(`verified matches — ${matches.length} <span class="dim">(${removed} false positive${removed === 1 ? "" : "s"} removed)</span>`,
    matches.map((m) =>
      `<div class="match"><div class="paths">${m.doc.paths.map(esc).join(" = ")}</div>` +
      m.lines.map((l) => `<pre>${String(l.lineNo).padStart(4)}: ${highlight(l.text, l.term)}</pre>`).join("") +
      `</div>`).join("") || `<span class="dim">nothing matched</span>`);

  el.innerHTML = html;
}

// --------------------------------------------------------------- ingest --
// WEB-3: prebuilt demo index (this repo, generated by the Rust CLI).
async function loadDemo() {
  status("loading demo index…");
  try {
    const res = await fetch("demo/index.json");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    setIndex(await res.json(), "demo index");
  } catch (e) {
    status(`demo index failed to load: ${e.message}`, true);
  }
}

// WEB-2: in-browser ingest of a small public repo via the GitHub API.
// Mirrors FR-2 (text ≤ 100 KB here) and FR-3 (dedupe by content).
const MAX_FILES = 100;
const MAX_BYTES = 100 * 1024;

async function fetchRepo() {
  const m = $("repo-url").value.match(/^https:\/\/github\.com\/([^/]+)\/([^/]+?)(?:\.git|\/.*)?$/);
  if (!m) { status("enter a URL like https://github.com/owner/repo", true); return; }
  const [, owner, repo] = m;
  const headers = { Accept: "application/vnd.github+json" };
  const token = $("token").value.trim();
  if (token) headers.Authorization = `Bearer ${token}`;

  try {
    status("fetching file tree…");
    const treeRes = await fetch(
      `https://api.github.com/repos/${owner}/${repo}/git/trees/HEAD?recursive=1`, { headers });
    if (!treeRes.ok) throw new Error(`tree fetch: HTTP ${treeRes.status} (rate limit? private repo?)`);
    const tree = await treeRes.json();

    const files = tree.tree
      .filter((e) => e.type === "blob" && e.size <= MAX_BYTES && !e.path.split("/").some((s) => s.startsWith(".")))
      .slice(0, MAX_FILES);
    if (files.length === 0) throw new Error("no indexable files found");

    // Dedupe by blob SHA — the same content-addressing trick as Blackbird
    // and FR-3, except GitHub already computed the hash for us.
    const bySha = new Map();
    const docs = [];
    let done = 0;
    for (const f of files) {
      status(`fetching blobs… ${++done}/${files.length}`);
      if (bySha.has(f.sha)) { docs[bySha.get(f.sha)].paths.push(f.path); continue; }
      const blobRes = await fetch(
        `https://api.github.com/repos/${owner}/${repo}/git/blobs/${f.sha}`,
        { headers: { ...headers, Accept: "application/vnd.github.raw+json" } });
      if (!blobRes.ok) throw new Error(`blob fetch: HTTP ${blobRes.status} (rate limit?)`);
      const content = await blobRes.text();
      if (content.includes("\0")) continue; // binary, mirror FR-2
      bySha.set(f.sha, docs.length);
      docs.push({ id: docs.length, paths: [f.path], hash: f.sha, content });
    }

    // Mirror of index.rs::build — one posting per doc per distinct gram;
    // ascending doc IDs keep the lists sorted for the merge-intersection.
    status("building trigram index…");
    const grams = {};
    for (const doc of docs) {
      for (const g of new Set(trigrams(doc.content))) (grams[g] ??= []).push(doc.id);
    }
    setIndex(
      { version: 1, source: `https://github.com/${owner}/${repo}`, docs, grams },
      `${owner}/${repo}`);
  } catch (e) {
    status(e.message, true);
  }
}

function setIndex(index, label) {
  INDEX = index;
  const grams = Object.keys(index.grams).length;
  status(`indexed ${label}: ${index.docs.length} docs, ${grams} distinct trigrams — search away`);
  $("search-panel").hidden = false;
  $("query").focus();
  runSearch();
}

// ------------------------------------------------------------------ wire --
$("demo-btn").addEventListener("click", loadDemo);
$("fetch-btn").addEventListener("click", fetchRepo);
$("search-btn").addEventListener("click", runSearch);
$("query").addEventListener("input", runSearch);
$("repo-url").addEventListener("keydown", (e) => { if (e.key === "Enter") fetchRepo(); });
