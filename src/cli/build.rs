//! # Build Command
//!
//! Process source files and generate the knowledge graph.

use super::*;
use crate::parser::{MultiLanguageParser, AstNode};
use crate::parser::comment_extractor::extract_comments;
use crate::parser::markdown_parser::parse_markdown;
use crate::graph::GraphBuilder;
use crate::graph::docs_builder::DocsBuilder;
use crate::ontology::TripleSet;
use std::path::Path;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use console::style;
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};
use rayon::prelude::*;

pub async fn run(
    repo_path: &Path,
    incremental: bool,
    languages: Option<Vec<String>>,
    jobs: usize,
) -> anyhow::Result<()> {
    println!("\n{}", style("OntoSys Build").cyan().bold());
    println!("{}\n", style("═".repeat(50)).dim());

    // Find git root and .ontosys
    let git_root = find_git_root(repo_path).ok_or_else(|| {
        anyhow::anyhow!("Not a git repository. Run 'ontosys init' first.")
    })?;

    let ontosys_path = ontosys_dir(&git_root);
    if !ontosys_path.exists() {
        error("Not initialized. Run 'ontosys init' first.");
        return Err(anyhow::anyhow!("Not initialized"));
    }

    // Load config
    let config: init::Config = serde_json::from_str(
        &fs::read_to_string(ontosys_path.join("config.json"))?
    )?;

    // Determine which languages to process
    let target_languages: Vec<Language> = match languages {
        Some(langs) => langs.iter()
            .filter_map(|l| l.parse().ok())
            .collect(),
        None => config.languages.iter()
            .filter_map(|l| l.parse().ok())
            .collect(),
    };

    info(&format!("Languages: {}",
        target_languages.iter()
            .map(|l| l.name())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    info(&format!("Parallel workers: {}", jobs));
    println!();

    // Collect files to process
    info("Discovering source files...");
    let files = discover_files(&git_root, &target_languages, &config.exclude_patterns)?;

    if files.is_empty() {
        warn("No source files found.");
        return Ok(());
    }

    println!("  Found {} files to process\n", style(files.len()).cyan());

    // Setup progress tracking
    let multi_progress = MultiProgress::new();
    let overall_pb = multi_progress.add(ProgressBar::new(files.len() as u64));
    overall_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} files ({eta})")
            .unwrap()
            .progress_chars("█▓░")
    );

    // Process files in parallel
    let parser = Arc::new(MultiLanguageParser::new()?);
    let all_nodes: Arc<std::sync::Mutex<Vec<(std::path::PathBuf, Vec<AstNode>)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let error_count = Arc::new(AtomicUsize::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));

    // Configure rayon thread pool
    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build_global()
        .ok();

    // Process files
    files.par_iter().for_each(|file_path| {
        let parser = Arc::clone(&parser);
        let all_nodes = Arc::clone(&all_nodes);
        let error_count = Arc::clone(&error_count);
        let success_count = Arc::clone(&success_count);

        match process_file(&parser, file_path, &git_root) {
            Ok(nodes) => {
                if !nodes.is_empty() {
                    all_nodes.lock().unwrap().push((file_path.clone(), nodes));
                }
                success_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", file_path.display(), e);
                error_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        overall_pb.inc(1);
    });

    overall_pb.finish_with_message("Parsing complete");
    println!();

    let nodes = Arc::try_unwrap(all_nodes)
        .unwrap()
        .into_inner()
        .unwrap();

    let parsed_count = success_count.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);

    println!("  {} Parsed: {} files", style("✓").green(), parsed_count);
    if errors > 0 {
        println!("  {} Errors: {} files", style("⚠").yellow(), errors);
    }
    println!();

    // Build the knowledge graph
    info("Building knowledge graph...");
    let build_pb = multi_progress.add(ProgressBar::new_spinner());
    build_pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap()
    );
    build_pb.set_message("Generating RDF triples...");

    let project_id = git_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let builder = GraphBuilder::new(project_id);

    // Flatten all nodes (clone to retain `nodes` for docs extraction)
    let all_ast_nodes: Vec<AstNode> = nodes.iter()
        .flat_map(|(_, n)| n.clone())
        .collect();

    let triples = builder.build_from_ast(&all_ast_nodes)?;

    build_pb.finish_with_message("Graph built");
    println!();

    println!("  Generated {} RDF triples", style(triples.len()).cyan());
    println!();

    // Save outputs
    info("Saving outputs...");
    let data_path = ontosys_path.join("data");

    // Save as Turtle
    let turtle_path = data_path.join("graph.ttl");
    fs::write(&turtle_path, triples.to_turtle())?;
    println!("  {} graph.ttl ({} bytes)",
        style("→").dim(),
        fs::metadata(&turtle_path)?.len()
    );

    // Save as N-Triples
    let nt_path = data_path.join("graph.nt");
    fs::write(&nt_path, triples.to_ntriples())?;
    println!("  {} graph.nt ({} bytes)",
        style("→").dim(),
        fs::metadata(&nt_path)?.len()
    );

    // Save as JSON-LD (for visualization)
    let jsonld = triples_to_jsonld(&triples, project_id);
    let json_path = data_path.join("graph.json");
    fs::write(&json_path, serde_json::to_string_pretty(&jsonld)?)?;
    println!("  {} graph.json ({} bytes)",
        style("→").dim(),
        fs::metadata(&json_path)?.len()
    );

    // Extract docs metadata if enabled
    let extract_docs = config.extract_docs;
    let mut docs_extracted_fields: (Option<bool>, Option<usize>, Option<usize>, Option<usize>) =
        (None, None, None, None);

    if extract_docs {
        info("Extracting documentation metadata...");

        let mut docs_builder = DocsBuilder::new();

        // Extract comments from all parsed source files
        for (file_path, file_nodes) in &nodes {
            let relative_path = file_path.strip_prefix(&git_root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string();

            let ext = file_path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let language = match ext.to_lowercase().as_str() {
                "rs" => "rust",
                "py" => "python",
                "ts" | "tsx" => "typescript",
                "js" | "jsx" | "mjs" | "cjs" => "javascript",
                _ => continue,
            };

            if let Ok(source) = fs::read_to_string(file_path) {
                let file_comments = extract_comments(&source, language);
                docs_builder.add_source_file(&relative_path, file_comments, file_nodes, project_id);
            }
        }

        // Discover and parse markdown files
        let md_files = discover_markdown_files(&git_root)?;
        for md_path in &md_files {
            if let Ok(source) = fs::read_to_string(md_path) {
                let relative_path = md_path.strip_prefix(&git_root)
                    .unwrap_or(md_path)
                    .to_string_lossy()
                    .to_string();
                let md = parse_markdown(&source);
                docs_builder.add_markdown_file(&relative_path, md);
            }
        }

        let docs_metadata = docs_builder.build();

        // Save docs.json
        let docs_path = data_path.join("docs.json");
        fs::write(&docs_path, serde_json::to_string_pretty(&docs_metadata)?)?;
        println!("  {} docs.json ({} bytes)",
            style("→").dim(),
            fs::metadata(&docs_path)?.len()
        );

        docs_extracted_fields = (
            Some(true),
            Some(docs_metadata.summary.total_doc_comments),
            Some(docs_metadata.summary.total_regular_comments),
            Some(docs_metadata.summary.total_todos + docs_metadata.summary.total_fixmes),
        );
    }

    // Save build metadata
    let metadata = BuildMetadata {
        timestamp: chrono::Utc::now().to_rfc3339(),
        files_processed: parsed_count,
        files_failed: errors,
        triples_generated: triples.len(),
        languages: target_languages.iter().map(|l| l.name().to_string()).collect(),
        docs_extracted: docs_extracted_fields.0,
        doc_comments_count: docs_extracted_fields.1,
        regular_comments_count: docs_extracted_fields.2,
        todo_count: docs_extracted_fields.3,
    };
    fs::write(
        data_path.join("build-meta.json"),
        serde_json::to_string_pretty(&metadata)?
    )?;

    println!();
    success("Build complete!");
    println!();
    println!("  Run {} to visualize the graph", style("ontosys serve").green());
    println!();

    Ok(())
}

fn discover_files(
    root: &Path,
    languages: &[Language],
    exclude_patterns: &[String],
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    use walkdir::WalkDir;

    let extensions: std::collections::HashSet<&str> = languages
        .iter()
        .flat_map(|l| l.extensions())
        .copied()
        .collect();

    let exclude_dirs: std::collections::HashSet<&str> = [
        "node_modules", "target", "__pycache__", "venv", ".venv",
        "dist", "build", ".git", ".ontosys"
    ].into_iter().collect();

    let files: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !exclude_dirs.contains(name.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            if !e.file_type().is_file() {
                return false;
            }
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(ext))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect();

    Ok(files)
}

fn discover_markdown_files(root: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    use walkdir::WalkDir;

    let exclude_dirs: std::collections::HashSet<&str> = [
        "node_modules", "target", "__pycache__", "venv", ".venv",
        "dist", "build", ".git", ".ontosys"
    ].into_iter().collect();

    let files: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !exclude_dirs.contains(name.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            if !e.file_type().is_file() {
                return false;
            }
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect();

    Ok(files)
}

fn process_file(
    parser: &MultiLanguageParser,
    path: &Path,
    root: &Path,
) -> anyhow::Result<Vec<AstNode>> {
    let source = fs::read_to_string(path)?;
    let _relative_path = path.strip_prefix(root).unwrap_or(path);
    parser.parse_file(path, &source).map_err(|e| anyhow::anyhow!("{}", e))
}

#[derive(serde::Serialize)]
struct BuildMetadata {
    timestamp: String,
    files_processed: usize,
    files_failed: usize,
    triples_generated: usize,
    languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs_extracted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_comments_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    regular_comments_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    todo_count: Option<usize>,
}

/// Convert triples to JSON-LD format for visualization
fn triples_to_jsonld(triples: &TripleSet, project_id: &str) -> serde_json::Value {
    use std::collections::{HashMap, HashSet};
    use crate::ontology::{Term, Literal};

    // Collect unique nodes and edges
    let mut nodes: HashMap<String, serde_json::Value> = HashMap::new();
    let mut edges: Vec<serde_json::Value> = Vec::new();

    for triple in triples.iter() {
        let subject_id = match &triple.subject {
            Term::Iri(iri) => iri.as_str().to_string(),
            Term::BlankNode(id) => format!("_:{}", id),
            _ => continue,
        };

        let predicate = triple.predicate.as_str();

        // Add node for subject if not exists
        if !nodes.contains_key(&subject_id) {
            nodes.insert(subject_id.clone(), serde_json::json!({
                "id": subject_id.clone(),
                "label": extract_label(&subject_id),
                "type": "unknown",
                "properties": {}
            }));
        }

        // Handle different predicates
        if predicate.contains("type") {
            if let Term::Iri(type_iri) = &triple.object {
                if let Some(node) = nodes.get_mut(&subject_id) {
                    node["type"] = serde_json::json!(extract_label(type_iri.as_str()));
                }
            }
        } else if predicate.contains("label") {
            if let Term::Literal(Literal::String(label)) = &triple.object {
                if let Some(node) = nodes.get_mut(&subject_id) {
                    node["label"] = serde_json::json!(label);
                }
            }
        } else if let Term::Iri(obj_iri) = &triple.object {
            // This is a relationship
            let obj_id = obj_iri.as_str().to_string();

            // Add node for object if not exists
            if !nodes.contains_key(&obj_id) {
                nodes.insert(obj_id.clone(), serde_json::json!({
                    "id": obj_id.clone(),
                    "label": extract_label(&obj_id),
                    "type": "unknown",
                    "properties": {}
                }));
            }

            edges.push(serde_json::json!({
                "source": subject_id,
                "target": obj_id,
                "label": extract_label(predicate),
                "type": extract_label(predicate)
            }));
        } else if let Term::Literal(lit) = &triple.object {
            // This is a property
            let value = match lit {
                Literal::String(s) => serde_json::json!(s),
                Literal::Typed { value, .. } => serde_json::json!(value),
                Literal::LangString { value, .. } => serde_json::json!(value),
            };

            if let Some(node) = nodes.get_mut(&subject_id) {
                let prop_name = extract_label(predicate);
                node["properties"][prop_name] = value;
            }
        }
    }

    serde_json::json!({
        "@context": {
            "rdf": "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
            "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
            "code": "http://example.org/code/",
            "co": "http://codeontology.org/ontology/"
        },
        "projectId": project_id,
        "stats": {
            "nodeCount": nodes.len(),
            "edgeCount": edges.len()
        },
        "nodes": nodes.into_values().collect::<Vec<_>>(),
        "edges": edges
    })
}

fn extract_label(uri: &str) -> String {
    uri.rsplit(|c| c == '/' || c == '#')
        .next()
        .unwrap_or(uri)
        .to_string()
}
