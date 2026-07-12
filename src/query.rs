//! Query parsing and expansion into a trigram plan (FR-6).
//!
//! Blackbird compiles `/arguments?/` into `arg AND rgu AND gum AND (…)`.
//! Our v1 query language is simpler — literal terms ANDed together — but the
//! shape is the same: query → terms → covering trigrams → intersection plan.

use crate::index::trigrams;

/// One term's slice of the plan: the literal to verify and the trigrams that
/// gate it. A term shorter than 3 chars has no trigram and forces a full
/// scan (FR-6) — kept explicit so the output can teach *why* that's bad.
#[derive(Debug, PartialEq)]
pub struct TermPlan {
    pub term: String,
    pub grams: Vec<String>,
    pub scan_all: bool,
}

/// Split a query into case-folded literal terms: bare words plus
/// `"quoted strings"` (FR-6). Unclosed quotes just run to end of input.
pub fn parse(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    // [ENUMS+MATCH] A hand-rolled state machine over a char iterator.
    // `match` on `(char, bool)` tuples would also work; the point is that
    // Rust pushes you toward making states explicit values rather than
    // Python's "flag variables plus careful if-ordering".
    for c in query.chars() {
        match (c, in_quotes) {
            ('"', _) => {
                in_quotes = !in_quotes;
                flush(&mut terms, &mut cur);
            }
            (c, false) if c.is_whitespace() => flush(&mut terms, &mut cur),
            (c, _) => cur.push(c),
        }
    }
    flush(&mut terms, &mut cur);
    terms
}

// [BORROWING] Both parameters are mutable borrows: this helper edits the
// caller's data in place and owns nothing. The signature alone tells you
// that — in Python/C# you'd have to read the body to know it mutates.
fn flush(terms: &mut Vec<String>, cur: &mut String) {
    if !cur.is_empty() {
        terms.push(cur.to_lowercase());
        cur.clear();
    }
}

/// Expand each term into its covering trigram set (FR-6).
pub fn plan(terms: &[String]) -> Vec<TermPlan> {
    // [ITERATORS] `map` + `collect` instead of an accumulator loop — the
    // same shape as a Python list comprehension, but the closure borrows
    // `t` and the result type (Vec<TermPlan>) drives what `collect` builds.
    terms
        .iter()
        .map(|t| {
            let grams = trigrams(t);
            TermPlan {
                term: t.clone(),
                scan_all: grams.is_empty(),
                grams,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_and_quoted_terms() {
        assert_eq!(parse("foo bar"), ["foo", "bar"]);
        assert_eq!(parse(r#""fn main" Config"#), ["fn main", "config"]);
        assert_eq!(parse("  "), Vec::<String>::new());
        assert_eq!(parse(r#""unclosed quote"#), ["unclosed quote"]);
    }

    #[test]
    fn expands_terms_to_covering_trigrams() {
        let p = plan(&["limits".to_string()]);
        assert_eq!(p[0].grams, ["lim", "imi", "mit", "its"]);
        assert!(!p[0].scan_all);
    }

    #[test]
    fn short_terms_fall_back_to_full_scan() {
        let p = plan(&["ab".to_string()]);
        assert!(p[0].scan_all);
        assert!(p[0].grams.is_empty());
    }
}
