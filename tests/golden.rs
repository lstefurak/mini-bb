//! Golden oracle test (NF-4, M2 acceptance): for the fixture directory,
//! indexed search must return exactly the same file set as a naive scan of
//! every file. The naive scanner is the trivially-correct spec; the engine
//! (trigram gate + verify) must never disagree with it.

use mini_bb::query::TermPlan;
use mini_bb::{index, ingest, query, search};
use std::path::Path;

/// The trivially-correct oracle: read every file, keep those where every
/// term has at least one variant literal in the folded content. Variant
/// expansion is reused from query::plan — the oracle checks the *engine*
/// (index + intersection + verify), not the parser.
fn naive_scan(root: &Path, plans: &[TermPlan]) -> Vec<String> {
    let mut hits = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap().to_lowercase();
            let hit = |p: &TermPlan| p.variants.iter().any(|v| content.contains(&v.literal));
            if plans.iter().all(hit) {
                let rel = path.strip_prefix(root).unwrap();
                hits.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    hits.sort();
    hits
}

#[test]
fn engine_agrees_with_naive_scan_on_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture");
    let (docs, _) = ingest::ingest(root.to_str().unwrap()).unwrap();
    let idx = index::build("fixture".into(), docs);

    let queries = [
        "abcd",             // trigram false positive in bait.txt must be filtered
        "fn main",          // two terms, AND
        "\"fn main\"",      // one quoted term
        "config",           // case-insensitive across Config/CONFIG
        "duplicate",        // must report both paths of the deduped doc
        "xy",               // short term → full-scan fallback
        "zzz_not_there",    // no results
        "\"abc then\" bcd", // quoted + bare mix
        "arguments?",       // the blog's optional-suffix expansion
        "conf(ig|use)",     // alternation across variants
        "duplicates? xy?",  // regex subset + full-scan fallback combined
    ];
    for q in queries {
        let terms = query::parse(q);
        let plans = query::plan(&terms);
        let cand = search::candidates(&idx, &plans);
        let mut engine: Vec<String> = search::verify(&idx, &plans, &cand)
            .iter()
            .flat_map(|m| m.doc.paths.iter().cloned())
            .collect();
        engine.sort();
        assert_eq!(engine, naive_scan(&root, &plans), "query {q:?} diverged");
    }
}

#[test]
fn false_positive_is_candidate_but_not_match() {
    // Locks in the M2 acceptance criterion explicitly: bait.txt contains
    // the trigrams of "abcd" but not the substring, so it must appear as a
    // candidate and disappear after verification.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture");
    let (docs, _) = ingest::ingest(root.to_str().unwrap()).unwrap();
    let idx = index::build("fixture".into(), docs);
    let plans = query::plan(&query::parse("abcd"));
    let cand = search::candidates(&idx, &plans);
    let paths_of = |ids: &[u32]| -> Vec<&str> {
        ids.iter()
            .flat_map(|&i| idx.docs[i as usize].paths.iter())
            .map(String::as_str)
            .collect()
    };
    assert!(
        paths_of(&cand).contains(&"bait.txt"),
        "bait must be a candidate"
    );
    let matches = search::verify(&idx, &plans, &cand);
    let matched: Vec<&str> = matches
        .iter()
        .flat_map(|m| &m.doc.paths)
        .map(String::as_str)
        .collect();
    assert!(!matched.contains(&"bait.txt"), "verify must remove bait");
    assert!(matched.contains(&"real.txt"));
}
