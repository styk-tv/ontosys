//! # Query Command
//!
//! Execute SPARQL queries against the knowledge graph.

use super::*;
use std::path::Path;
use std::fs;
use console::style;

pub async fn run(repo_path: &Path, query: &str, format: &str) -> anyhow::Result<()> {
    let git_root = find_git_root(repo_path).ok_or_else(|| {
        anyhow::anyhow!("Not a git repository")
    })?;

    let ontosys_path = ontosys_dir(&git_root);
    if !ontosys_path.exists() {
        error("Not initialized. Run 'ontosys init' first.");
        return Err(anyhow::anyhow!("Not initialized"));
    }

    let graph_path = ontosys_path.join("data/graph.json");
    if !graph_path.exists() {
        error("No graph data found. Run 'ontosys build' first.");
        return Err(anyhow::anyhow!("No graph data"));
    }

    // Load graph data
    let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(&graph_path)?)?;

    // For now, implement simple queries on the JSON data
    // In production, this would use Oxigraph's SPARQL engine

    info(&format!("Query: {}", query));
    println!();

    // Simple pattern matching for common queries
    let results = execute_simple_query(&data, query)?;

    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        "csv" => {
            if let Some(arr) = results.as_array() {
                if let Some(first) = arr.first() {
                    if let Some(obj) = first.as_object() {
                        // Print header
                        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                        println!("{}", keys.join(","));

                        // Print rows
                        for item in arr {
                            if let Some(row) = item.as_object() {
                                let values: Vec<String> = keys.iter()
                                    .map(|k| row.get(*k).map(|v| v.to_string()).unwrap_or_default())
                                    .collect();
                                println!("{}", values.join(","));
                            }
                        }
                    }
                }
            }
        }
        _ => {
            // Table format
            print_as_table(&results);
        }
    }

    Ok(())
}

fn execute_simple_query(data: &serde_json::Value, query: &str) -> anyhow::Result<serde_json::Value> {
    let empty_vec = vec![];
    let nodes = data["nodes"].as_array().unwrap_or(&empty_vec);

    let query_lower = query.to_lowercase();

    // Match patterns like "SELECT * WHERE { ?x a code:Function }"
    if query_lower.contains("function") {
        let results: Vec<_> = nodes.iter()
            .filter(|n| n["type"].as_str() == Some("Function"))
            .cloned()
            .collect();
        return Ok(serde_json::json!(results));
    }

    if query_lower.contains("struct") || query_lower.contains("class") {
        let results: Vec<_> = nodes.iter()
            .filter(|n| {
                let t = n["type"].as_str().unwrap_or("");
                t == "Struct" || t == "Class"
            })
            .cloned()
            .collect();
        return Ok(serde_json::json!(results));
    }

    if query_lower.contains("trait") || query_lower.contains("interface") {
        let results: Vec<_> = nodes.iter()
            .filter(|n| {
                let t = n["type"].as_str().unwrap_or("");
                t == "Trait" || t == "Interface"
            })
            .cloned()
            .collect();
        return Ok(serde_json::json!(results));
    }

    if query_lower.contains("import") {
        let results: Vec<_> = nodes.iter()
            .filter(|n| n["type"].as_str() == Some("Import"))
            .cloned()
            .collect();
        return Ok(serde_json::json!(results));
    }

    // Default: return all nodes
    Ok(serde_json::json!(nodes))
}

fn print_as_table(data: &serde_json::Value) {
    if let Some(arr) = data.as_array() {
        if arr.is_empty() {
            println!("No results found.");
            return;
        }

        // Get column widths
        let mut max_name = 4;
        let mut max_type = 4;

        for item in arr {
            if let Some(name) = item["label"].as_str() {
                max_name = max_name.max(name.len());
            }
            if let Some(t) = item["type"].as_str() {
                max_type = max_type.max(t.len());
            }
        }

        max_name = max_name.min(50);
        max_type = max_type.min(20);

        // Print header
        println!(
            "{:width_name$} │ {:width_type$}",
            style("Name").bold(),
            style("Type").bold(),
            width_name = max_name,
            width_type = max_type
        );
        println!("{}┼{}", "─".repeat(max_name + 1), "─".repeat(max_type + 2));

        // Print rows
        for item in arr.iter().take(50) {
            let name = item["label"].as_str().unwrap_or("-");
            let t = item["type"].as_str().unwrap_or("-");

            let name_display = if name.len() > max_name {
                format!("{}...", &name[..max_name - 3])
            } else {
                name.to_string()
            };

            println!(
                "{:width_name$} │ {:width_type$}",
                name_display,
                t,
                width_name = max_name,
                width_type = max_type
            );
        }

        if arr.len() > 50 {
            println!("\n... and {} more results", arr.len() - 50);
        }

        println!("\n{} results total", style(arr.len()).cyan());
    }
}
