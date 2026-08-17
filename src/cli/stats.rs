//! # Stats Command
//!
//! Show statistics about the knowledge graph.

use super::*;
use std::path::Path;
use std::fs;
use console::style;

pub async fn run(repo_path: &Path) -> anyhow::Result<()> {
    println!("\n{}", style("OntoSys Statistics").cyan().bold());
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
    let meta_path = data_path.join("build-meta.json");

    if !meta_path.exists() {
        warn("No build data found. Run 'ontosys build' first.");
        return Ok(());
    }

    // Load metadata
    #[derive(serde::Deserialize)]
    struct BuildMeta {
        timestamp: String,
        files_processed: usize,
        files_failed: usize,
        triples_generated: usize,
        languages: Vec<String>,
    }

    let meta: BuildMeta = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;

    // Load graph data for detailed stats
    let graph_path = data_path.join("graph.json");
    let graph_stats = if graph_path.exists() {
        let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(&graph_path)?)?;
        Some((
            data["stats"]["nodeCount"].as_u64().unwrap_or(0),
            data["stats"]["edgeCount"].as_u64().unwrap_or(0),
        ))
    } else {
        None
    };

    // Print stats
    println!("{}:", style("Last Build").bold());
    println!("  {} {}", style("Timestamp:").dim(), meta.timestamp);
    println!("  {} {:?}", style("Languages:").dim(), meta.languages);
    println!();

    println!("{}:", style("Processing").bold());
    println!("  {} {}", style("Files processed:").dim(), style(meta.files_processed).cyan());
    println!("  {} {}", style("Files failed:").dim(),
        if meta.files_failed > 0 {
            style(meta.files_failed).yellow().to_string()
        } else {
            style(meta.files_failed).green().to_string()
        }
    );
    println!();

    println!("{}:", style("Graph").bold());
    println!("  {} {}", style("RDF triples:").dim(), style(meta.triples_generated).cyan());

    if let Some((nodes, edges)) = graph_stats {
        println!("  {} {}", style("Nodes:").dim(), style(nodes).cyan());
        println!("  {} {}", style("Edges:").dim(), style(edges).cyan());
    }
    println!();

    // File sizes
    println!("{}:", style("Output Files").bold());
    for name in &["graph.ttl", "graph.nt", "graph.json", "docs.json"] {
        let path = data_path.join(name);
        if path.exists() {
            let size = fs::metadata(&path)?.len();
            let size_str = if size > 1_000_000 {
                format!("{:.2} MB", size as f64 / 1_000_000.0)
            } else if size > 1_000 {
                format!("{:.2} KB", size as f64 / 1_000.0)
            } else {
                format!("{} bytes", size)
            };
            println!("  {} {} ({})", style("•").dim(), name, style(size_str).dim());
        }
    }

    // Show docs summary if docs.json exists
    let docs_path = data_path.join("docs.json");
    if docs_path.exists() {
        if let Ok(docs_str) = fs::read_to_string(&docs_path) {
            if let Ok(docs) = serde_json::from_str::<serde_json::Value>(&docs_str) {
                if let Some(summary) = docs.get("summary") {
                    println!();
                    println!("{}:", style("Documentation").bold());
                    if let Some(v) = summary.get("total_doc_comments").and_then(|v| v.as_u64()) {
                        println!("  {} {}", style("Doc comments:").dim(), style(v).cyan());
                    }
                    if let Some(v) = summary.get("total_regular_comments").and_then(|v| v.as_u64()) {
                        println!("  {} {}", style("Regular comments:").dim(), style(v).cyan());
                    }
                    if let Some(v) = summary.get("total_todos").and_then(|v| v.as_u64()) {
                        println!("  {} {}", style("TODOs:").dim(), style(v).cyan());
                    }
                    if let Some(v) = summary.get("total_fixmes").and_then(|v| v.as_u64()) {
                        println!("  {} {}", style("FIXMEs:").dim(), style(v).cyan());
                    }
                    if let Some(v) = summary.get("markdown_files").and_then(|v| v.as_u64()) {
                        println!("  {} {}", style("Markdown files:").dim(), style(v).cyan());
                    }
                }
            }
        }
    }

    println!();
    Ok(())
}
