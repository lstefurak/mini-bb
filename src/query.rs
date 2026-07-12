//! Query parsing and expansion into a trigram plan (FR-6).
//!
//! Blackbird compiles `/arguments?/` into `arg ∧ rgu ∧ gum ∧ (… ∨ …)`.
//! We do the same shape: each term expands into literal *variants* (OR),
//! each variant into covering trigrams (AND). Bare terms get a tiny regex
//! subset — `?` (previous char optional), `(a|b)` alternation, `\` escape —
//! while quoted terms stay fully literal.

use crate::index::trigrams;

/// One literal alternative of a term, with the trigrams that gate it.
/// A variant shorter than 3 chars has no trigram and forces a full scan
/// (FR-6) — kept explicit so the output can teach *why* that's bad.
#[derive(Debug, PartialEq)]
pub struct Variant {
    pub literal: String,
    pub grams: Vec<String>,
    pub scan_all: bool,
}

/// One term's slice of the plan: variants are ORed, terms are ANDed (FR-7).
#[derive(Debug, PartialEq)]
pub struct TermPlan {
    pub term: String,
    pub variants: Vec<Variant>,
}

/// A parsed term: raw text plus whether it was quoted (quoted = literal,
/// no regex expansion).
#[derive(Debug, PartialEq)]
pub struct Term {
    pub raw: String,
    pub quoted: bool,
}

/// Split a query into case-folded terms: bare words plus `"quoted strings"`
/// (FR-6). Unclosed quotes just run to end of input.
pub fn parse(query: &str) -> Vec<Term> {
    let mut terms = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    // [ENUMS+MATCH] A hand-rolled state machine over a char iterator.
    // `match` on `(char, bool)` tuples makes every state/input pair an
    // explicit arm — Rust pushes states into values rather than Python's
    // "flag variables plus careful if-ordering".
    for c in query.chars() {
        match (c, in_quotes) {
            ('"', q) => {
                flush(&mut terms, &mut cur, q);
                in_quotes = !q;
            }
            (c, false) if c.is_whitespace() => flush(&mut terms, &mut cur, false),
            (c, _) => cur.push(c),
        }
    }
    flush(&mut terms, &mut cur, in_quotes);
    terms
}

// [BORROWING] The first two parameters are mutable borrows: this helper
// edits the caller's data in place and owns nothing. The signature alone
// tells you that — in Python/C# you'd read the body to know it mutates.
fn flush(terms: &mut Vec<Term>, cur: &mut String, quoted: bool) {
    if !cur.is_empty() {
        terms.push(Term {
            raw: cur.to_lowercase(),
            quoted,
        });
        cur.clear();
    }
}

/// Expand a bare term's regex subset into every literal variant (FR-6):
/// `arguments?` → `["argument", "arguments"]`, `(foo|bar)s` → `["foos", "bars"]`.
/// No cap on combinations — a teaching tool trusts its user's queries.
fn expand(raw: &str) -> Vec<String> {
    // Start with one empty variant and extend every variant per input char —
    // groups multiply the set, `?` doubles it (with/without the last char).
    let mut out = vec![String::new()];
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(esc) = chars.next() {
                    out.iter_mut().for_each(|v| v.push(esc));
                }
            }
            '(' => {
                let group: String = chars.by_ref().take_while(|&g| g != ')').collect();
                // [ITERATORS] A cross product with no index bookkeeping:
                // for each existing variant × each branch, produce a new
                // string. `flat_map` is Python's nested comprehension /
                // C#'s SelectMany, checked for type mismatches at compile time.
                out = out
                    .iter()
                    .flat_map(|v| group.split('|').map(move |b| format!("{v}{b}")))
                    .collect();
            }
            '?' => {
                let mut without: Vec<String> = out
                    .iter()
                    .map(|v| {
                        let mut w = v.clone();
                        w.pop();
                        w
                    })
                    .collect();
                out.append(&mut without);
                out.sort();
                out.dedup();
            }
            c => out.iter_mut().for_each(|v| v.push(c)),
        }
    }
    out
}

/// Expand each term into ORed variants, each with its covering trigram set.
pub fn plan(terms: &[Term]) -> Vec<TermPlan> {
    terms
        .iter()
        .map(|t| {
            let literals = if t.quoted {
                vec![t.raw.clone()]
            } else {
                expand(&t.raw)
            };
            let variants = literals
                .into_iter()
                .map(|literal| {
                    let grams = trigrams(&literal);
                    Variant {
                        scan_all: grams.is_empty(),
                        literal,
                        grams,
                    }
                })
                .collect();
            TermPlan {
                term: t.raw.clone(),
                variants,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raws(q: &str) -> Vec<String> {
        parse(q).into_iter().map(|t| t.raw).collect()
    }

    #[test]
    fn parses_bare_and_quoted_terms() {
        assert_eq!(raws("foo bar"), ["foo", "bar"]);
        assert_eq!(raws(r#""fn main" Config"#), ["fn main", "config"]);
        assert_eq!(raws("  "), Vec::<String>::new());
        assert_eq!(raws(r#""unclosed quote"#), ["unclosed quote"]);
        assert!(parse(r#""a?b""#)[0].quoted);
        assert!(!parse("a?b")[0].quoted);
    }

    #[test]
    fn expands_blog_example_optional_suffix() {
        assert_eq!(expand("arguments?"), ["argument", "arguments"]);
    }

    #[test]
    fn expands_alternation_and_escapes() {
        assert_eq!(expand("(foo|bar)s"), ["foos", "bars"]);
        assert_eq!(expand(r"what\?"), ["what?"]);
        assert_eq!(expand("(a|b)(c|d)"), ["ac", "ad", "bc", "bd"]);
    }

    #[test]
    fn quoted_terms_are_not_expanded() {
        let p = plan(&parse(r#""a?b""#));
        assert_eq!(p[0].variants.len(), 1);
        assert_eq!(p[0].variants[0].literal, "a?b");
    }

    #[test]
    fn variants_carry_covering_trigrams() {
        let p = plan(&parse("limits"));
        assert_eq!(p[0].variants[0].grams, ["lim", "imi", "mit", "its"]);
        assert!(!p[0].variants[0].scan_all);
    }

    #[test]
    fn short_variants_fall_back_to_full_scan() {
        let p = plan(&parse("ab"));
        assert!(p[0].variants[0].scan_all);
        // `x?` expands to "" and "x" — both sub-trigram, both full scan.
        let p = plan(&parse("x?"));
        assert!(p[0].variants.iter().all(|v| v.scan_all));
    }
}
