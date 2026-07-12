//! CLI entry point: `mini-bb index` (FR-1..FR-5).

use clap::Parser;

// [DERIVE] clap builds the whole argument parser from this type at compile
// time — compare Python's argparse (imperative, runtime) or C#'s
// System.CommandLine (builder pattern). The struct *is* the CLI contract:
// `--help`, validation, and typed values all fall out of the definition.
#[derive(Parser)]
#[command(about = "Educational trigram code search (mini Blackbird) — see SPEC.md")]
enum Cmd {
    /// Index a public GitHub repo URL or local directory into a JSON file.
    Index {
        /// GitHub URL (https://github.com/owner/repo) or local path.
        source: String,
        /// Where to write the index.
        #[arg(short, long, default_value = "index.json")]
        output: String,
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
        Cmd::Index { source, output } => {
            let (docs, stats) = mini_bb::ingest::ingest(&source)?;
            let idx = mini_bb::index::build(source, docs);
            mini_bb::index::save(&idx, &output)?;
            let bytes = std::fs::metadata(&output)?.len();
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
        }
    }
    Ok(())
}
