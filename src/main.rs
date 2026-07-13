//! CLI entry point: `mini-bb index` (FR-1..FR-5) and `mini-bb search`
//! (FR-6..FR-9). Output formatting lives here so the engine stays pure.

use clap::Parser;

// [DERIVE] clap builds the whole argument parser from this type at compile
// time — compare Python's argparse (imperative, runtime) or C#'s
// System.CommandLine (builder pattern). The enum *is* the CLI contract:
// `--help`, validation, and typed values all fall out of the definition.
#[derive(Parser)]
#[command(about = "Educational trigram code search (mini Blackbird) — see spec/")]
enum Cmd {
    /// Index a public GitHub repo URL or local directory into a JSON file.
    Index {
        /// GitHub URL (https://github.com/owner/repo) or local path.
        source: String,
        /// Where to write the index.
        #[arg(short, long, default_value = "index.json")]
        output: String,
    },
    /// Search an index, showing the trigram plan the query expands into.
    Search {
        /// Bare words and "quoted strings", ANDed together (case-insensitive).
        query: String,
        /// Index file produced by `index`.
        #[arg(short, long, default_value = "index.json")]
        index: String,
        /// Stop after showing the plan and candidates; skip verification.
        #[arg(long)]
        explain: bool,
    },
}

fn main() {
    // [ERRORS] main delegates to a Result-returning function and maps any
    // error to a one-line message + non-zero exit (NF-5). This is the Rust
    // idiom for "top-level exception handler": errors bubble via `?`, and
    // only the outermost layer decides how to present them.
    if let Err(e) = run(Cmd::parse()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cmd: Cmd) -> mini_bb::Result<()> {
    match cmd {
        Cmd::Index { source, output } => cmd_index(source, &output),
        Cmd::Search {
            query,
            index,
            explain,
        } => cmd_search(&query, &index, explain),
    }
}

fn cmd_index(source: String, output: &str) -> mini_bb::Result<()> {
    let (docs, stats) = mini_bb::ingest::ingest(&source)?;
    let idx = mini_bb::index::build(source, docs);
    mini_bb::index::save(&idx, output)?;
    let bytes = std::fs::metadata(output)?.len();
    println!(
        "indexed {} docs ({} duplicate paths merged, {} files skipped)",
        stats.indexed, stats.merged, stats.skipped
    );
    println!(
        "{} distinct trigrams → {} ({} KB)",
        idx.grams.len(),
        output,
        bytes / 1024
    );
    Ok(())
}

/// ANSI-bold the (already-lowercased) term inside an original-case line.
/// We search the folded copy but splice the original by byte offset, so
/// `Config` highlights when the term is `config`. If case folding changed
/// byte lengths (rare Unicode edge), skip highlighting rather than panic.
fn highlight(text: &str, term: &str) -> String {
    let folded = text.to_lowercase();
    let end = |i: usize| i + term.len();
    match folded.find(term) {
        // [BORROWING] `&text[..i]` slices borrow the original string —
        // three views, zero copies until `format!` assembles the result.
        // Python slices copy; C# needs Span<char> to get this for free.
        Some(i)
            if folded.len() == text.len()
                && text.is_char_boundary(i)
                && text.is_char_boundary(end(i)) =>
        {
            format!(
                "{}\x1b[1m{}\x1b[0m{}",
                &text[..i],
                &text[i..end(i)],
                &text[end(i)..]
            )
        }
        _ => text.to_string(),
    }
}

/// The FR-9 pipeline display: terms → expansion → plan → candidates → matches.
fn cmd_search(query: &str, index_path: &str, explain: bool) -> mini_bb::Result<()> {
    let idx = mini_bb::index::load(index_path)?;
    let terms = mini_bb::query::parse(query);
    if terms.is_empty() {
        return Err("empty query".into());
    }
    let shown: Vec<&str> = terms.iter().map(|t| t.raw.as_str()).collect();
    println!("1. terms (AND):      {shown:?}");
    let plans = mini_bb::query::plan(&terms);
    for p in &plans {
        let vars: Vec<&str> = p.variants.iter().map(|v| v.literal.as_str()).collect();
        println!("2. expand {:12} → {}", p.term, vars.join(" ∨ "));
    }
    for p in &plans {
        let sides: Vec<String> = p
            .variants
            .iter()
            .map(|v| {
                if v.scan_all {
                    format!("{:?} shorter than a trigram: full scan!", v.literal)
                } else {
                    let sized: Vec<String> = v
                        .grams
                        .iter()
                        .map(|g| format!("{g}[{}]", idx.grams.get(g).map_or(0, Vec::len)))
                        .collect();
                    format!("({})", sized.join(" ∧ "))
                }
            })
            .collect();
        println!(
            "3. plan for {:9} → {} — [n] = posting-list length",
            p.term,
            sides.join(" ∨ ")
        );
    }
    let cand = mini_bb::search::candidates(&idx, &plans);
    println!(
        "4. candidates after intersection: {} of {} docs",
        cand.len(),
        idx.docs.len()
    );
    if explain {
        return Ok(());
    }
    let matches = mini_bb::search::verify(&idx, &plans, &cand);
    println!(
        "5. verified matches: {} (false positives removed: {})",
        matches.len(),
        cand.len() - matches.len()
    );
    for m in &matches {
        println!("\n  {}", m.doc.paths.join("  =  "));
        for (term, line_no, text) in &m.lines {
            println!("    {line_no}: {}", highlight(text.trim_end(), term));
        }
    }
    Ok(())
}
