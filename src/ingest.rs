//! Ingestion: fetch a repo (FR-1), filter files (FR-2), dedupe (FR-3).

use crate::{fnv1a, Doc, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Files larger than this are skipped (FR-2).
const MAX_FILE_BYTES: u64 = 200 * 1024;

/// Counters reported to the user instead of failing the run (FR-2, FR-3).
#[derive(Debug, Default)]
pub struct Stats {
    pub indexed: usize,
    pub merged: usize,
    pub skipped: usize,
}

/// Ingest a source (GitHub URL or local directory) into deduplicated docs.
pub fn ingest(source: &str) -> Result<(Vec<Doc>, Stats)> {
    // [ENUMS+MATCH] No `if isinstance(...)` chain: we branch on data and the
    // temp dir is an `Option<PathBuf>` — the "maybe a value" case is a real
    // type, not a nullable reference (C#) or an implicit None (Python).
    let (root, tmp) = if source.starts_with("http://") || source.starts_with("https://") {
        let dir = clone_shallow(source)?;
        (dir.clone(), Some(dir))
    } else {
        (PathBuf::from(source), None)
    };

    let mut docs: Vec<Doc> = Vec::new();
    let mut by_hash: HashMap<u64, usize> = HashMap::new();
    let mut stats = Stats::default();
    walk(&root, &root, &mut docs, &mut by_hash, &mut stats)?;

    // Best-effort cleanup of the clone; a leftover temp dir is not an error.
    if let Some(dir) = tmp {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok((docs, stats))
}

/// `git clone --depth 1` into a temp dir (FR-1). Shelling out to git keeps
/// us dependency-free and mirrors how you'd do this in a Python subprocess.
fn clone_shallow(url: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("mini-bb-{}", std::process::id()));
    // [ERRORS] The `?` operator: "return the error to my caller if this
    // failed, otherwise unwrap the value". It replaces try/except and
    // try/catch blocks with one character, but the propagation is *visible*
    // at every call site — no invisible exception control flow.
    let out = Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(&dir)
        .output()?;
    if !out.status.success() {
        return Err(format!("git clone failed: {}", String::from_utf8_lossy(&out.stderr)).into());
    }
    Ok(dir)
}

/// Recursive directory walk collecting deduplicated text files.
fn walk(
    root: &Path,
    dir: &Path,
    docs: &mut Vec<Doc>,
    by_hash: &mut HashMap<u64, usize>,
    stats: &mut Stats,
) -> Result<()> {
    // [BORROWING] `docs`, `by_hash` and `stats` are `&mut` — explicit,
    // exclusive, temporary loans. In Python/C# any callee holding the object
    // reference could mutate it at any time; here mutation rights are part
    // of the function signature and enforced by the compiler.
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            // FR-2: skip git internals, dot-dirs, and build output.
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            walk(root, &path, docs, by_hash, stats)?;
            continue;
        }
        if entry.metadata()?.len() > MAX_FILE_BYTES {
            stats.skipped += 1;
            continue;
        }
        // [ENUMS+MATCH] UTF-8 validation returns Result, and `match` must
        // handle both arms — you cannot forget the "binary file" case the
        // way an unhandled UnicodeDecodeError slips through in Python.
        let content = match String::from_utf8(std::fs::read(&path)?) {
            Ok(text) => text,
            Err(_) => {
                stats.skipped += 1;
                continue;
            }
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        add_doc(docs, by_hash, stats, rel, content);
    }
    Ok(())
}

/// Content-addressed insert (FR-3): identical content merges into one doc.
fn add_doc(
    docs: &mut Vec<Doc>,
    by_hash: &mut HashMap<u64, usize>,
    stats: &mut Stats,
    path: String,
    content: String,
) {
    let hash = fnv1a(content.as_bytes());
    if let Some(&i) = by_hash.get(&hash) {
        docs[i].paths.push(path);
        stats.merged += 1;
        return;
    }
    by_hash.insert(hash, docs.len());
    stats.indexed += 1;
    // [OWNERSHIP] `content` is *moved* into the Doc — no copy is made and
    // the local variable is dead afterwards. Python and C# would share a
    // reference here; Rust transfers ownership so exactly one owner frees it.
    docs.push(Doc {
        id: docs.len() as u32,
        paths: vec![path],
        hash: format!("{hash:016x}"),
        content,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mini-bb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "same content").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "same content").unwrap();
        std::fs::write(dir.join("c.txt"), "unique content").unwrap();
        std::fs::write(dir.join("bin.dat"), [0u8, 159, 146, 150]).unwrap();
        dir
    }

    #[test]
    fn dedupes_and_skips_binary() {
        let dir = tmp_fixture();
        let (docs, stats) = ingest(dir.to_str().unwrap()).unwrap();
        assert_eq!(stats.indexed, 2, "identical files share one doc");
        assert_eq!(stats.merged, 1);
        assert_eq!(stats.skipped, 1, "non-UTF-8 file skipped");
        let dup = docs.iter().find(|d| d.content == "same content").unwrap();
        assert_eq!(dup.paths.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
