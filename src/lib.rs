//! mini-bb — an educational trigram code-search engine (see SPEC.md).
//!
//! Pipeline: `ingest` (clone/walk/dedupe) → `index` (trigram posting lists)
//! → `query` (parse + expand to a trigram plan) → `search` (intersect +
//! verify). Module layout and budgets are fixed by SPEC.md §6.

pub mod index;
pub mod ingest;
pub mod query;
pub mod search;

// [DERIVE] `derive` asks the compiler to *generate* trait implementations at
// compile time — Rust's answer to Python decorators / C# attributes, except
// nothing happens at runtime: serde emits real (de)serialization code here.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// [ERRORS] Rust has no exceptions. Fallible functions return
// `Result<T, E>` (like Go, or C#'s Try* pattern made mandatory), and the
// caller *must* do something with it. `Box<dyn Error>` is a trait object —
// "any error type" — the closest thing to `catch (Exception e)`.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One deduplicated document (Blackbird shards by git blob SHA; we dedupe by
/// content hash — identical files across paths are indexed once, FR-3).
#[derive(Debug, Serialize, Deserialize)]
pub struct Doc {
    pub id: u32,
    /// All paths whose content hashed identically.
    pub paths: Vec<String>,
    pub hash: String,
    pub content: String,
}

/// The whole searchable index — serialized to a single JSON file (FR-5).
#[derive(Debug, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub source: String,
    pub docs: Vec<Doc>,
    /// trigram → sorted, deduplicated doc IDs (the posting lists, FR-4).
    // [TRAITS] BTreeMap (not HashMap) so JSON output is deterministic: its
    // keys only need `Ord`. Python dicts keep insertion order; Rust's
    // HashMap order is deliberately randomized, so determinism is opt-in
    // via a different data structure rather than a dict flag.
    pub grams: BTreeMap<String, Vec<u32>>,
}

/// FNV-1a 64-bit content hash (FR-3). Six lines beats a dependency, and —
/// unlike Python's `hash()` or C#'s `GetHashCode()` — it is stable across
/// runs, which an on-disk index requires.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        // [OWNERSHIP] `wrapping_mul`: integer overflow is a *panic* in debug
        // builds, not silent wraparound like C#'s unchecked math or a
        // silent bigint promotion like Python. Hashing wants wraparound,
        // so we must say so explicitly.
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_is_stable_across_runs() {
        // Known FNV-1a test vector: empty input returns the offset basis.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"hello"), fnv1a(b"hello"));
        assert_ne!(fnv1a(b"hello"), fnv1a(b"world"));
    }
}
