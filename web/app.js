// mini-bb web frontend (spec 001 WEB-1..WEB-4).
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
// Quoted terms stay fully literal; bare terms get the regex subset.
function parseQuery(query) {
  const terms = [];
  let cur = "";
  let inQuotes = false;
  const flush = (quoted) => {
    if (cur) terms.push({ raw: cur.toLowerCase(), quoted });
    cur = "";
  };
  for (const c of query) {
    if (c === '"') { flush(inQuotes); inQuotes = !inQuotes; }
    else if (/\s/.test(c) && !inQuotes) flush(false);
    else cur += c;
  }
  flush(inQuotes);
  return terms;
}

// Mirror of query.rs::expand — the regex subset: `?` (previous char
// optional), `(a|b)` alternation, `\` escape → all literal variants.
function expand(raw) {
  let out = [""];
  const chars = Array.from(raw);
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i];
    if (c === "\\") { if (++i < chars.length) out = out.map((v) => v + chars[i]); }
    else if (c === "(") {
      let group = "";
      while (++i < chars.length && chars[i] !== ")") group += chars[i];
      out = out.flatMap((v) => group.split("|").map((b) => v + b));
    } else if (c === "?") {
      out = [...new Set([...out, ...out.map((v) => v.slice(0, -1))])].sort();
    } else out = out.map((v) => v + c);
  }
  return out;
}

// Mirror of query.rs::plan — terms AND, variants OR, trigrams AND.
function plan(terms) {
  return terms.map((t) => ({
    term: t.raw,
    variants: (t.quoted ? [t.raw] : expand(t.raw)).map((literal) => {
      const grams = trigrams(literal);
      return { literal, grams, scanAll: grams.length === 0 };
    }),
  }));
}

// Mirror of search.rs::candidates — AND over terms of term OR-sets.
function candidates(index, plans) {
  let acc = null;
  for (const p of plans) {
    const ids = termCandidates(index, p.variants);
    acc = acc === null ? ids : intersect(acc, ids);
  }
  return acc ?? [];
}

// Mirror of search.rs::term_candidates — union of variant intersections.
function termCandidates(index, variants) {
  let ids = [];
  for (const v of variants) {
    if (v.scanAll) return index.docs.map((d) => d.id);
    const lists = v.grams.map((g) => index.grams[g] ?? []);
    lists.sort((a, b) => a.length - b.length);
    let vIds = lists[0];
    for (const l of lists.slice(1)) {
      vIds = intersect(vIds, l);
      if (vIds.length === 0) break;
    }
    ids = union(ids, vIds);
  }
  return ids;
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

// Mirror of search.rs::union — same walk, keeping everything once.
function union(a, b) {
  const out = [];
  let i = 0, j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] < b[j]) out.push(a[i++]);
    else if (a[i] > b[j]) out.push(b[j++]);
    else { out.push(a[i]); i++; j++; }
  }
  return out.concat(a.slice(i), b.slice(j));
}

// Mirror of search.rs::verify — trigram hits are candidates, not matches.
// A doc matches when every term has at least one variant literal present.
function verify(index, plans, ids) {
  const matches = [];
  for (const id of ids) {
    const doc = index.docs[id];
    const folded = doc.content.toLowerCase();
    if (!plans.every((p) => p.variants.some((v) => folded.includes(v.literal)))) continue;
    const lines = [];
    const foldedLines = folded.split("\n");
    const origLines = doc.content.split("\n");
    for (const p of plans) {
      for (let n = 0; n < foldedLines.length; n++) {
        const v = p.variants.find((v) => foldedLines[n].includes(v.literal));
        if (v) { lines.push({ term: v.literal, lineNo: n + 1, text: origLines[n] }); break; }
      }
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

  let html = stage("terms (AND)", terms.map((t) => `<span class="chip term">${esc(t.raw)}</span>`).join(" "));

  html += stage("expand each term into variants (OR), each into covering trigrams",
    plans.map((p) =>
      `<div class="expansion"><span class="chip term">${esc(p.term)}</span> → ` +
      p.variants.map((v) =>
        `<span class="chip variant">${esc(v.literal) || "ε"}</span> ` +
        (v.scanAll
          ? `<span class="warn">shorter than a trigram — full scan of every doc!</span>`
          : v.grams.map((g) => `<span class="chip gram">${esc(g)}</span>`).join(" "))
      ).join(` <span class="dim">∨</span> `) +
      `</div>`).join(""));

  html += stage("plan — AND terms, OR variants, AND posting lists <span class='dim'>(chip badge = list length)</span>",
    plans.map((p) =>
      `<div class="expansion">` +
      p.variants.map((v) =>
        v.scanAll
          ? `<span class="warn">full scan</span>`
          : "( " + v.grams.map((g) => {
              const n = (INDEX.grams[g] ?? []).length;
              return `<span class="chip gram">${esc(g)}<b>${n}</b></span>`;
            }).join(" ∧ ") + " )"
      ).join(` <span class="dim">∨</span> `) +
      `</div>`).join(`<div class="dim">∧</div>`));

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
  // WEB-6 (spec 002): the explorer is a separate, view-only module.
  if (window.renderExplorer) window.renderExplorer(index);
  $("query").focus();
  runSearch();
}

// ------------------------------------------------------------------ wire --
$("demo-btn").addEventListener("click", loadDemo);
$("fetch-btn").addEventListener("click", fetchRepo);
$("search-btn").addEventListener("click", runSearch);
$("query").addEventListener("input", runSearch);
$("repo-url").addEventListener("keydown", (e) => { if (e.key === "Enter") fetchRepo(); });
