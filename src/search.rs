//! Plan execution: posting-list intersection (FR-7), verification (FR-8),
//! and match snippets (FR-9).

use crate::query::TermPlan;
use crate::{Doc, Index};

/// A verified match: the doc plus, per term, the first matching line.
pub struct Match<'a> {
    // [LIFETIMES] `'a` says: this Match *borrows* from an Index that must
    // outlive it. Python/C# would let results outlive (and pin) the index
    // via GC; Rust instead proves at compile time that we never hold a
    // result after the index is gone — no copy of the doc is ever made.
    pub doc: &'a Doc,
    /// (term, 1-based line number, line text) — first hit per term.
    pub lines: Vec<(String, usize, String)>,
}

/// Intersect every trigram's posting list across all terms → candidate doc
/// IDs (FR-7). Trigram hits are necessary but not sufficient: a doc holding
/// all of `abc`,`bcd` need not contain `abcd`. Verification (below) fixes that.
pub fn candidates(index: &Index, plans: &[TermPlan]) -> Vec<u32> {
    let mut lists: Vec<&[u32]> = Vec::new();
    for p in plans {
        for g in &p.grams {
            // A gram absent from the index means no doc can match: the
            // empty slice makes the whole intersection empty, for free.
            lists.push(index.grams.get(g).map_or(&[], |v| v));
        }
    }
    // Every term was too short to gate anything (scan_all): candidates are
    // all docs, and correctness rests entirely on verification.
    if lists.is_empty() {
        return index.docs.iter().map(|d| d.id).collect();
    }
    // Smallest list first: the intersection can only shrink, so starting
    // tiny keeps every later merge cheap (Blackbird's iterators do the same
    // lazily; sorting by length is our simple version of that idea).
    lists.sort_by_key(|l| l.len());
    let mut acc: Vec<u32> = lists[0].to_vec();
    for l in &lists[1..] {
        acc = intersect(&acc, l);
        if acc.is_empty() {
            break;
        }
    }
    acc
}

/// Classic two-pointer merge of sorted ID lists — the reason posting lists
/// are kept sorted (FR-4). O(n+m), no hashing, no allocation per element.
fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        // [ENUMS+MATCH] Three-way compare returns the `Ordering` enum and
        // `match` must handle all variants — the compiler rejects a missing
        // `Equal` arm, unlike an if/elif chain that silently falls through.
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// Verify candidates against real content (FR-8) and collect snippets (FR-9).
/// Case-insensitive by construction: terms are already folded, content is
/// folded here — like Blackbird, correctness comes from this pass, not the index.
pub fn verify<'a>(index: &'a Index, plans: &[TermPlan], ids: &[u32]) -> Vec<Match<'a>> {
    let mut out = Vec::new();
    for &id in ids {
        let doc = &index.docs[id as usize];
        let folded = doc.content.to_lowercase();
        if !plans.iter().all(|p| folded.contains(&p.term)) {
            continue;
        }
        let lines = plans
            .iter()
            .filter_map(|p| {
                // [ITERATORS] `find` is lazy: it stops folding lines at the
                // first hit, like C#'s LINQ `First` — not Python's typical
                // "build the whole list, take [0]".
                folded
                    .lines()
                    .enumerate()
                    .find(|(_, l)| l.contains(&p.term))
                    .map(|(n, _)| {
                        let text = doc.content.lines().nth(n).unwrap_or("").to_string();
                        (p.term.clone(), n + 1, text)
                    })
            })
            .collect();
        out.push(Match { doc, lines });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::build;
    use crate::query::{parse, plan};

    fn idx(contents: &[&str]) -> Index {
        let docs = contents
            .iter()
            .enumerate()
            .map(|(i, c)| Doc {
                id: i as u32,
                paths: vec![format!("f{i}.txt")],
                hash: String::new(),
                content: c.to_string(),
            })
            .collect();
        build("test".into(), docs)
    }

    fn search(index: &Index, q: &str) -> Vec<u32> {
        let plans = plan(&parse(q));
        let cand = candidates(index, &plans);
        verify(index, &plans, &cand)
            .iter()
            .map(|m| m.doc.id)
            .collect()
    }

    #[test]
    fn intersect_merges_sorted_lists() {
        assert_eq!(intersect(&[1, 3, 5, 7], &[2, 3, 7, 9]), vec![3, 7]);
        assert_eq!(intersect(&[], &[1]), Vec::<u32>::new());
    }

    #[test]
    fn verification_kills_trigram_false_positives() {
        // Doc 1 contains the trigrams `abc` and `bcd` but not "abcd":
        // it is a candidate (M2 acceptance case) and must be filtered out.
        let index = idx(&["say abcd here", "abc then bcd separately"]);
        let plans = plan(&parse("abcd"));
        assert_eq!(
            candidates(&index, &plans),
            vec![0, 1],
            "both are candidates"
        );
        assert_eq!(
            search(&index, "abcd"),
            vec![0],
            "only doc 0 survives verify"
        );
    }

    #[test]
    fn multi_term_is_and_and_case_insensitive() {
        let index = idx(&["Hello World", "hello there", "world only"]);
        assert_eq!(search(&index, "hello world"), vec![0]);
        assert_eq!(search(&index, "HELLO"), vec![0, 1]);
    }

    #[test]
    fn short_terms_scan_all_docs() {
        let index = idx(&["xy here", "nothing"]);
        assert_eq!(search(&index, "xy"), vec![0]);
    }

    #[test]
    fn snippets_report_first_matching_line() {
        let index = idx(&["no hit\nfn main() {\n}"]);
        let plans = plan(&parse(r#""fn main""#));
        let m = verify(&index, &plans, &[0]);
        assert_eq!(m[0].lines[0], ("fn main".into(), 2, "fn main() {".into()));
    }
}
