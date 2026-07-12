//! mini-bb — an educational trigram code-search engine (see SPEC.md).
//!
//! M0 stub: shared types and modules land in M1/M2 (SPEC.md §9).
//! The module layout below is fixed by SPEC.md §6.

// [ENUMS+MATCH] Placeholder so `cargo test` has something real to check in M0.
// In Python this would be a module-level constant; in C# a static class member.
// Rust makes it a `pub const` with an explicit type — no implicit typing at
// module scope, and it is inlined at compile time rather than looked up.
pub const SPEC_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        assert_eq!(SPEC_VERSION, 1);
    }
}
