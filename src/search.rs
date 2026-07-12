//! Plan execution: posting-list intersection/union (FR-7), verification
//! (FR-8), and match snippets (FR-9).

use crate::query::{TermPlan, Variant};
use crate::{Doc, Index};

/// A verified match: the doc plus, per term, the first matching line.
pub struct Match<'a> {
    // [LIFETIMES] `'a` says: this Match *borrows* from an Index that must
    // outlive it. Python/C# would let results outlive (and pin) the index
    // via GC; Rust instead proves at compile time that we never hold a
    // result after the index is gone — no copy of the doc is ever made.
    pub doc: &'a Doc,
    /// (matched variant literal, 1-based line number, line text) per term.
    pub lines: Vec<(String, usize, String)>,
}

/// Execute the FR-7 plan: AND over terms ( OR over variants ( AND over the
/// variant's posting lists ) ) → candidate doc IDs. Trigram hits are
/// necessary but not sufficient: a doc holding `abc` and `bcd` need not
/// contain `abcd`. Verification (below) fixes that.
pub fn candidates(index: &Index, plans: &[TermPlan]) -> Vec<u32> {
    let mut acc: Option<Vec<u32>> = None;
    for p in plans {
        let ids = term_candidates(index, &p.variants);
        // [ENUMS+MATCH] `Option` instead of a magic "not started" sentinel:
        // `None` = no term processed yet, `Some(ids)` = running intersection.
        // Python would use `acc = None` too — but nothing there *forces* the
        // None-check; here forgetting it simply doesn't compile.
        acc = Some(match acc {
            None => ids,
            Some(prev) => intersect(&prev, &ids),
        });
    }
    acc.unwrap_or_default()
}

/// OR across one term's variants: union of each variant's gram intersection.
/// A sub-trigram variant (scan_all) can't be gated, so the whole term
/// degenerates to "all docs" and verification does the real work.
fn term_candidates(index: &Index, variants: &[Variant]) -> Vec<u32> {
    let mut ids: Vec<u32> = Vec::new();
    for v in variants {
        if v.scan_all {
            return index.docs.iter().map(|d| d.id).collect();
        }
        let mut lists: Vec<&[u32]> = v
            .grams
            .iter()
            .map(|g| index.grams.get(g).map_or(&[][..], |l| l))
            .collect();
        // Smallest list first: the intersection can only shrink, so starting
        // tiny keeps every later merge cheap (Blackbird's lazy iterators
        // exploit the same idea).
        lists.sort_by_key(|l| l.len());
        let mut v_ids: Vec<u32> = lists[0].to_vec();
        for l in &lists[1..] {
            v_ids = intersect(&v_ids, l);
            if v_ids.is_empty() {
                break;
            }
        }
        ids = union(&ids, &v_ids);
    }
    ids
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

/// The same merge walk as `intersect`, keeping everything once (sorted OR).
fn union(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    // One side is exhausted; the sorted remainder of the other passes through.
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

/// Verify candidates against real content (FR-8) and collect snippets (FR-9):
/// a doc matches when every term has at least one variant literal present.
/// Case-insensitive by construction: literals are already folded, content is
/// folded here — like Blackbird, correctness comes from this pass, not the index.
pub fn verify<'a>(index: &'a Index, plans: &[TermPlan], ids: &[u32]) -> Vec<Match<'a>> {
    let mut out = Vec::new();
    for &id in ids {
        let doc = &index.docs[id as usize];
        let folded = doc.content.to_lowercase();
        let hit = |p: &TermPlan| p.variants.iter().any(|v| folded.contains(&v.literal));
        if !plans.iter().all(hit) {
            continue;
        }
        let lines = plans
            .iter()
            .filter_map(|p| {
                // [ITERATORS] `find_map` is lazy: it walks lines until the
                // first one containing any variant, like C#'s LINQ `First` —
                // not Python's typical "build the whole list, take [0]".
                folded.lines().enumerate().find_map(|(n, l)| {
                    let v = p.variants.iter().find(|v| l.contains(&v.literal))?;
                    let text = doc.content.lines().nth(n).unwrap_or("").to_string();
                    Some((v.literal.clone(), n + 1, text))
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
    fn intersect_and_union_merge_sorted_lists() {
        assert_eq!(intersect(&[1, 3, 5, 7], &[2, 3, 7, 9]), vec![3, 7]);
        assert_eq!(intersect(&[], &[1]), Vec::<u32>::new());
        assert_eq!(union(&[1, 3, 7], &[2, 3, 9]), vec![1, 2, 3, 7, 9]);
        assert_eq!(union(&[], &[1, 2]), vec![1, 2]);
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
    fn regex_subset_matches_either_variant() {
        let index = idx(&["one argument only", "many arguments here", "argue"]);
        assert_eq!(search(&index, "arguments?"), vec![0, 1]);
        assert_eq!(search(&index, "argu(e|ment)"), vec![0, 1, 2]);
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
