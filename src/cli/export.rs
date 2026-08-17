//! # Export Command
//!
//! Export the graph to various formats.

use super::*;
use std::path::Path;
use std::fs;
use console::style;

pub async fn run(repo_path: &Path, output: &Path, format: &str) -> anyhow::Result<()> {
    println!("\n{}", style("OntoSys Export").cyan().bold());
    println!("{}\n", style("═".repeat(50)).dim());

    let git_root = find_git_root(repo_path).ok_or_else(|| {
        anyhow::anyhow!("Not a git repository")
    })?;

    let ontosys_path = ontosys_dir(&git_root);
    if !ontosys_path.exists() {
        error("Not initialized. Run 'ontosys init' first.");
        return Err(anyhow::anyhow!("Not initialized"));
    }

    let data_path = ontosys_path.join("data");

    let source_file = match format {
        "turtle" | "ttl" => "graph.ttl",
        "ntriples" | "nt" => "graph.nt",
        "json" | "jsonld" => "graph.json",
        _ => {
            error(&format!("Unsupported format: {}. Use turtle, ntriples, or jsonld", format));
            return Err(anyhow::anyhow!("Unsupported format"));
        }
    };

    let source_path = data_path.join(source_file);
    if !source_path.exists() {
        error("No graph data found. Run 'ontosys build' first.");
        return Err(anyhow::anyhow!("No graph data"));
    }

    info(&format!("Exporting to {} format...", format));

    // Copy the file
    fs::copy(&source_path, output)?;

    let size = fs::metadata(output)?.len();
    let size_str = if size > 1_000_000 {
        format!("{:.2} MB", size as f64 / 1_000_000.0)
    } else if size > 1_000 {
        format!("{:.2} KB", size as f64 / 1_000.0)
    } else {
        format!("{} bytes", size)
    };

    println!();
    success(&format!("Exported to {} ({})", output.display(), size_str));
    println!();

    Ok(())
}
