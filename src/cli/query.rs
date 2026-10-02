//! # Query Command
//!
//! Run a SPARQL query against the built graph (`.ontosys/data/graph.nt`),
//! loaded into the same in-memory store `serve` uses.

use super::store::Loaded;
use super::*;
use std::path::Path;

pub async fn run(repo_path: &Path, query: &str, format: &str) -> anyhow::Result<()> {
    let root = find_git_root(repo_path).unwrap_or_else(|| repo_path.to_path_buf());
    let graph = super::diff::resolve_graph(&root)?;
    let query = query.to_string();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        let loaded = Loaded::load(&graph, None)?;
        loaded.sparql(&query, usize::MAX)
    })
    .await??;

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&result)?),
        _ => print_rows(&result, format == "csv"),
    }
    Ok(())
}

fn cell(v: &serde_json::Value) -> String {
    match v["type"].as_str() {
        Some("iri") => v["label"].as_str().map(|l| l.to_string()).unwrap_or_else(|| v["short"].as_str().unwrap_or("").to_string()),
        _ => v["value"].as_str().unwrap_or("").to_string(),
    }
}

fn print_rows(r: &serde_json::Value, csv: bool) {
    if r["kind"] == "ask" {
        println!("{}", r["boolean"]);
        return;
    }
    let vars: Vec<String> = r["vars"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let rows: Vec<Vec<String>> = r["rows"]
        .as_array()
        .map(|a| a.iter().map(|row| vars.iter().map(|v| cell(&row[v])).collect()).collect())
        .unwrap_or_default();
    if csv {
        let q = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
        println!("{}", vars.iter().map(|v| q(v)).collect::<Vec<_>>().join(","));
        for row in &rows {
            println!("{}", row.iter().map(|c| q(c)).collect::<Vec<_>>().join(","));
        }
        return;
    }
    let widths: Vec<usize> = vars
        .iter()
        .enumerate()
        .map(|(i, v)| rows.iter().map(|r| r[i].chars().count()).chain([v.len()]).max().unwrap_or(0).min(60))
        .collect();
    let line = |cells: &[String]| {
        cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| {
                let c: String = c.chars().take(*w).collect();
                format!("{:<w$}", c, w = *w)
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    println!("{}", line(&vars));
    println!("{}", widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("  "));
    for row in &rows {
        println!("{}", line(row));
    }
    eprintln!("\n{} rows · {} ms", rows.len(), r["elapsed_ms"]);
}
