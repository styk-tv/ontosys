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
use crate::grounding::vocab;
use crate::complexity;
use crate::lang_c::{self, CFile};
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

    // Which tree this build reads, taken before reading it.
    let (git_head, git_dirty) = git_state(&git_root);

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

    // C goes through tree-sitter; the other languages through their own parsers.
    let (c_paths, files): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| {
        matches!(p.extension().and_then(|e| e.to_str()), Some("c" | "h"))
    });

    // Setup progress tracking
    let multi_progress = MultiProgress::new();
    let overall_pb = multi_progress.add(ProgressBar::new((files.len() + c_paths.len()) as u64));
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

    // C: read every file once, derive the loop and erased macros from the
    // tree's own #defines (never from a shipped list), then parse with them.
    // A file that is not valid UTF-8 is parsed from a lossy decode, which is
    // not its bytes: such files get no definition hashes.
    let (c_sources, c_exact): (Vec<(String, String)>, Vec<bool>) = c_paths
        .par_iter()
        .map(|path| {
            let bytes = fs::read(path).unwrap_or_default();
            let exact = std::str::from_utf8(&bytes).is_ok();
            let raw = String::from_utf8_lossy(&bytes).into_owned();
            let rel = path.strip_prefix(&git_root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            ((rel, raw), exact)
        })
        .unzip();
    let macro_lists = lang_c::prepass::derive_macros(&c_sources);
    let unresolved_macro_statements: usize = c_sources
        .par_iter()
        .map(|(_, raw)| lang_c::prepass::unresolved_macro_statements(raw, &macro_lists.defined))
        .sum();
    let mut c_files: Vec<CFile> = c_sources
        .par_iter()
        .zip(c_exact.par_iter())
        .map_init(lang_c::new_parser, |parser, ((rel, raw), exact)| {
            let f = lang_c::extract_with(parser, rel, raw, Some(&macro_lists), *exact);
            overall_pb.inc(1);
            f
        })
        .collect();
    c_files.sort_by(|a, b| a.path.cmp(&b.path));

    overall_pb.finish_with_message("Parsing complete");
    println!();

    let mut nodes = Arc::try_unwrap(all_nodes)
        .unwrap()
        .into_inner()
        .unwrap();
    // Parallel collection order is arbitrary; sort so output does not depend on it.
    nodes.sort_by(|a, b| a.0.cmp(&b.0));

    let parsed_count = success_count.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);

    // Every file this build reads, and every file it passes over by extension.
    let md_files = if config.extract_docs { discover_markdown_files(&git_root)? } else { Vec::new() };
    let read_inputs: Vec<std::path::PathBuf> = files.iter().chain(&c_paths).chain(&md_files).cloned().collect();
    let inputs_manifest_text = inputs_manifest(&git_root, &read_inputs)?;
    let read_extensions: std::collections::HashSet<&str> =
        target_languages.iter().flat_map(|l| l.extensions()).copied().collect();
    let unparsed = unparsed_by_extension(&git_root, |ext| {
        read_extensions.contains(ext) || (config.extract_docs && ext.eq_ignore_ascii_case("md"))
    });

    println!("  {} Parsed: {} files", style("✓").green(), parsed_count + c_files.len());
    let c_functions: usize = c_files.iter().map(|f| f.functions.len()).sum();
    let c_errors: usize = c_files.iter().map(|f| f.parse_errors).sum();
    let c_clean = c_files.iter().filter(|f| f.parse_errors == 0).count();
    if !c_files.is_empty() {
        println!(
            "  {} C (tree-sitter): {} files, {} functions; {} files parse cleanly, {} error nodes in the rest",
            style("✓").green(), c_files.len(), c_functions, c_clean, c_errors
        );
    }
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

    let project_name = project_name(&git_root, config.project.as_deref());
    let project_id = project_name.as_str();
    info(&format!("Project: {}", project_id));

    let builder = GraphBuilder::new(project_id);

    // Flatten all nodes (clone to retain `nodes` for docs extraction)
    let all_ast_nodes: Vec<AstNode> = nodes.iter()
        .flat_map(|(_, n)| n.clone())
        .collect();

    let mut triples = builder.build_from_ast(&all_ast_nodes)?;

    // Tier A for the line-parsed languages: a tree-sitter scan of each file,
    // attached to its declarations by the line of each function's name.
    let scanned: Vec<(String, (Vec<complexity::scan::FunctionFacts>, usize), complexity::Grammar)> = files
        .par_iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(&git_root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            let grammar = complexity::Grammar::for_path(&rel).filter(|g| *g != complexity::Grammar::C)?;
            let bytes = fs::read(path).ok()?;
            let scan = complexity::scan::scan_file(&rel, &bytes, grammar);
            Some((rel, scan, grammar))
        })
        .collect();
    let mut per_file = std::collections::BTreeMap::new();
    let mut scan_errors: std::collections::BTreeMap<complexity::Grammar, usize> = std::collections::BTreeMap::new();
    for (rel, (facts, errors), grammar) in scanned {
        *scan_errors.entry(grammar).or_default() += errors;
        per_file.insert(rel, facts);
    }
    let (attached, mut tier_a_stats) = complexity::join::attach(&triples, &per_file);
    for (grammar, errors) in scan_errors {
        tier_a_stats.entry(grammar).or_default().error_nodes += errors;
    }
    for (iri, facts) in &attached {
        complexity::rdf::tier_a_triples(iri, &facts.tier_a, facts.definition_sha256.as_deref(), &mut triples);
    }

    if !c_files.is_empty() {
        let ix = lang_c::rdf::index(project_id, &c_files);
        lang_c::rdf::to_triples(project_id, &c_files, &ix, &mut triples);
        vocab::emit(&mut triples);
    }
    triples.canonicalize();

    build_pb.finish_with_message("Graph built");
    println!();

    println!("  Generated {} RDF triples", style(triples.len()).cyan());
    println!();
    if !c_files.is_empty() {
        println!(
            "  C prepass (derived from the tree's own #defines): {} loop macros, {} erased macros, {} statement macros with no definition",
            macro_lists.loop_macros.len(), macro_lists.erased.len(), unresolved_macro_statements
        );
        println!();
    }

    // Save outputs
    info("Saving outputs...");
    let data_path = ontosys_path.join("data");

    if config.redact_emails {
        triples = redact_triples(triples);
    }

    // A graph a strict loader would refuse is not written at all.
    let nt_text = triples.to_ntriples();
    require_strict(&nt_text)?;

    // Save as Turtle
    let turtle_path = data_path.join("graph.ttl");
    fs::write(&turtle_path, triples.to_turtle())?;
    println!("  {} graph.ttl ({} bytes)",
        style("→").dim(),
        fs::metadata(&turtle_path)?.len()
    );

    // Save as N-Triples
    let nt_path = data_path.join("graph.nt");
    fs::write(&nt_path, &nt_text)?;
    println!("  {} graph.nt ({} bytes)",
        style("→").dim(),
        fs::metadata(&nt_path)?.len()
    );
    let graph_sha256 = sha256_hex(nt_text.as_bytes());

    // `sha256sum -c .ontosys/data/inputs.sha256` from the repository root names
    // every input that changed since this build.
    fs::write(data_path.join("inputs.sha256"), &inputs_manifest_text)?;
    println!("  {} inputs.sha256 ({} files)",
        style("→").dim(),
        inputs_manifest_text.lines().count()
    );

    // Save as JSON-LD (for visualization). The browser view cannot usefully
    // render millions of edges, so very large graphs skip it rather than ship a
    // file nobody can open; a stale one from an earlier build is removed.
    let json_path = data_path.join("graph.json");
    if triples.len() <= MAX_VIZ_TRIPLES {
        let jsonld = triples_to_jsonld(&triples, project_id);
        fs::write(&json_path, serde_json::to_string_pretty(&jsonld)?)?;
        println!("  {} graph.json ({} bytes)",
            style("→").dim(),
            fs::metadata(&json_path)?.len()
        );
    } else {
        let _ = fs::remove_file(&json_path);
        println!("  {} graph.json skipped ({} triples > {} visualization limit)",
            style("→").dim(), triples.len(), MAX_VIZ_TRIPLES);
    }

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

        // Parse the markdown files discovered with the other inputs
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
        let mut docs_json = serde_json::to_value(&docs_metadata)?;
        if config.redact_emails {
            redact_json(&mut docs_json);
        }
        fs::write(&docs_path, serde_json::to_string_pretty(&docs_json)?)?;
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
    let (ontosys_commit, ontosys_dirty) = tool_commit();
    let metadata = BuildMetadata {
        ontosys_version: env!("CARGO_PKG_VERSION"),
        ontosys_commit,
        ontosys_dirty,
        redact_emails: config.redact_emails,
        timestamp: chrono::Utc::now().to_rfc3339(),
        git_head,
        git_dirty,
        project: project_id.to_string(),
        c_files: c_files.len(),
        c_functions,
        c_parse_error_nodes: c_errors,
        complexity: ComplexityMeta {
            hash_skipped_invalid_utf8: c_files.iter().filter(|f| f.hash_skipped).count(),
            tier_a: {
                let mut m: std::collections::BTreeMap<&'static str, complexity::join::AttachStats> =
                    tier_a_stats.into_iter().map(|(g, st)| (g.name(), st)).collect();
                if !c_files.is_empty() {
                    m.insert(
                        complexity::Grammar::C.name(),
                        complexity::join::AttachStats { analysed: c_functions, attached: c_functions, error_nodes: c_errors, ..Default::default() },
                    );
                }
                m
            },
        },
        prepass: (!c_files.is_empty()).then(|| PrepassMeta {
            loop_macros: macro_lists.loop_macros.iter().cloned().collect(),
            erased_macro_count: macro_lists.erased.len(),
            defined_macro_count: macro_lists.defined.len(),
            unresolved_macro_statements,
        }),
        files_processed: parsed_count,
        files_failed: errors,
        triples_generated: triples.len(),
        graph_sha256,
        inputs_sha256: sha256_hex(inputs_manifest_text.as_bytes()),
        languages: target_languages.iter().map(|l| l.name().to_string()).collect(),
        unparsed_by_extension: unparsed,
        docs_extracted: docs_extracted_fields.0,
        doc_comments_count: docs_extracted_fields.1,
        regular_comments_count: docs_extracted_fields.2,
        todo_count: docs_extracted_fields.3,
    };
    fs::write(
        data_path.join("build-meta.json"),
        serde_json::to_string_pretty(&metadata)?
    )?;
    // The vocabulary every oc: term in this build's output is declared in.
    fs::write(data_path.join("code.ttl"), include_str!("../../ontology/code.ttl"))?;
    // The C parse input in full: which macros the tree's own #defines made
    // loop headers or erased (mostly include guards, hence not in build-meta).
    let prepass_path = data_path.join("prepass.json");
    if c_files.is_empty() {
        let _ = fs::remove_file(&prepass_path);
    } else {
        fs::write(
            &prepass_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "loop_macros": macro_lists.loop_macros,
                "erased_macros": macro_lists.erased,
            }))?,
        )?;
    }

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
    let _ = exclude_patterns; // TODO: config exclude patterns are not applied yet
    let extensions: std::collections::HashSet<&str> = languages
        .iter()
        .flat_map(|l| l.extensions())
        .copied()
        .collect();
    Ok(walk_sources(root, |ext| extensions.contains(ext)))
}

fn discover_markdown_files(root: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    Ok(walk_sources(root, |ext| ext.eq_ignore_ascii_case("md")))
}

/// Files under `root` whose extension `keep` accepts. The repository boundary
/// is git's: `.gitignore`, `.git/info/exclude` and the global excludes apply
/// (a virtualenv or build output is not part of the project); hidden but
/// tracked directories such as `.github/` are kept. Dependency and output
/// directories are skipped even where nothing ignores them.
fn walk_sources(root: &Path, keep: impl Fn(&str) -> bool) -> Vec<std::path::PathBuf> {
    walk_files(root)
        .into_iter()
        .filter(|p| p.extension().and_then(|x| x.to_str()).map_or(false, &keep))
        .collect()
}

/// Every file inside the repository boundary (see `walk_sources`), sorted.
fn walk_files(root: &Path) -> Vec<std::path::PathBuf> {
    const SKIP_DIRS: &[&str] = &["node_modules", "target", "__pycache__", "venv", ".venv", "dist", "build", ".git", ".ontosys"];
    let mut files: Vec<std::path::PathBuf> = ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

/// `sha256sum` lines (`<hex>  <repo-relative path>`) for `files`, sorted by
/// path. A file that cannot be read fails the build rather than being left
/// out of the record of what was read.
fn inputs_manifest(root: &Path, files: &[std::path::PathBuf]) -> anyhow::Result<String> {
    let mut lines = files
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
            let bytes = fs::read(p).map_err(|e| anyhow::anyhow!("cannot read input {}: {}", rel, e))?;
            Ok((rel, sha256_hex(&bytes)))
        })
        .collect::<anyhow::Result<Vec<(String, String)>>>()?;
    lines.sort();
    lines.dedup();
    Ok(lines.into_iter().map(|(rel, hex)| format!("{}  {}\n", hex, rel)).collect())
}

/// Files inside the repository boundary that the build does not read, counted
/// by lower-cased extension (`(none)` for files without one).
fn unparsed_by_extension(root: &Path, reads: impl Fn(&str) -> bool) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for path in walk_files(root) {
        let key = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if reads(ext) => continue,
            Some(ext) => format!(".{}", ext.to_lowercase()),
            None => "(none)".to_string(),
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// HEAD's commit and whether the working tree differs from it (tracked
/// changes or untracked, unignored files; `.ontosys/` excluded). `(None, None)`
/// when `root` is not the top of a git repository with a commit.
fn git_state(root: &Path) -> (Option<String>, Option<bool>) {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    // Only `root`'s own repository counts, never one that merely contains it.
    if git(&["rev-parse", "--show-cdup"]).as_deref() != Some("") {
        return (None, None);
    }
    let Some(head) = git(&["rev-parse", "--verify", "-q", "HEAD"]) else {
        return (None, None);
    };
    let dirty = git(&["status", "--porcelain", "--untracked-files=normal", "--", ".", ":(exclude).ontosys"])
        .map(|out| !out.is_empty());
    (Some(head), dirty)
}

fn process_file(
    parser: &MultiLanguageParser,
    path: &Path,
    root: &Path,
) -> anyhow::Result<Vec<AstNode>> {
    let source = fs::read_to_string(path)?;
    // Locations are recorded relative to the repository, so they are the same
    // in every checkout and do not leak where it lives.
    let relative_path = path.strip_prefix(root).unwrap_or(path);
    parser.parse_file(relative_path, &source).map_err(|e| anyhow::anyhow!("{}", e))
}

/// Above this many triples the full graph.json (browser visualization) is not written.
const MAX_VIZ_TRIPLES: usize = 250_000;

#[derive(serde::Serialize)]
struct BuildMetadata {
    ontosys_version: &'static str,
    /// The ontosys commit this binary was built from ("unknown" outside git).
    ontosys_commit: &'static str,
    /// Whether that ontosys tree had uncommitted changes when built.
    ontosys_dirty: bool,
    redact_emails: bool,
    timestamp: String,
    /// HEAD's commit when the build started; absent outside a git repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    git_head: Option<String>,
    /// Whether the working tree differed from HEAD (`.ontosys/` excluded).
    #[serde(skip_serializing_if = "Option::is_none")]
    git_dirty: Option<bool>,
    project: String,
    c_files: usize,
    c_functions: usize,
    c_parse_error_nodes: usize,
    complexity: ComplexityMeta,
    /// What the C prepass derived from the tree's own #defines (full lists in prepass.json).
    #[serde(skip_serializing_if = "Option::is_none")]
    prepass: Option<PrepassMeta>,
    files_processed: usize,
    files_failed: usize,
    triples_generated: usize,
    /// sha256 of graph.nt's bytes as written.
    graph_sha256: String,
    /// sha256 of inputs.sha256's bytes (every file this build read).
    inputs_sha256: String,
    languages: Vec<String>,
    /// Files inside the repository boundary this build did not read, by extension.
    unparsed_by_extension: std::collections::BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs_extracted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_comments_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    regular_comments_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    todo_count: Option<usize>,
}

#[derive(serde::Serialize)]
struct ComplexityMeta {
    /// C files parsed from a lossy UTF-8 decode, whose functions carry no definition hash.
    hash_skipped_invalid_utf8: usize,
    /// Per grammar: functions analysed, attached to a declaration, and why not.
    tier_a: std::collections::BTreeMap<&'static str, complexity::join::AttachStats>,
}

/// The ontosys commit this binary was built from, and whether its tree was dirty.
fn tool_commit() -> (&'static str, bool) {
    (env!("ONTOSYS_COMMIT"), env!("ONTOSYS_DIRTY") == "true")
}

#[derive(serde::Serialize)]
struct PrepassMeta {
    loop_macros: Vec<String>,
    erased_macro_count: usize,
    defined_macro_count: usize,
    /// Indented `NAME(args)` lines whose NAME no scanned file defines.
    unresolved_macro_statements: usize,
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

/// `triples` with every email address in a literal replaced by `<email>`.
fn redact_triples(triples: TripleSet) -> TripleSet {
    use crate::ontology::redact::emails;
    use crate::ontology::{Literal, Term};
    triples
        .into_iter()
        .map(|mut t| {
            if let Term::Literal(l) = &mut t.object {
                *l = match std::mem::replace(l, Literal::string("")) {
                    Literal::String(v) => Literal::String(emails(&v)),
                    Literal::LangString { value, lang } => Literal::LangString { value: emails(&value), lang },
                    Literal::Typed { value, datatype } => Literal::Typed { value: emails(&value), datatype },
                };
            }
            t
        })
        .collect()
}

/// `v` with every email address in any string replaced by `<email>`.
fn redact_json(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::String(s) => *s = crate::ontology::redact::emails(s),
        Value::Array(a) => a.iter_mut().for_each(redact_json),
        Value::Object(o) => o.values_mut().for_each(redact_json),
        _ => {}
    }
}

/// Refuse to write a graph a strict N-Triples loader would refuse.
fn require_strict(nt: &str) -> anyhow::Result<()> {
    let bad = crate::ontology::strict_violations(nt);
    if bad.is_empty() {
        return Ok(());
    }
    let shown: Vec<String> = bad.iter().take(5).map(|v| format!("line {}: {}", v.line, v.reason)).collect();
    anyhow::bail!(
        "graph is not strict N-Triples: {} line(s) refused, e.g. {}",
        bad.len(),
        shown.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_graph_with_an_invalid_iri_fails_the_build_and_names_the_lines() {
        let nt = "<http://e/s> <http://e/p> <http://e/o> .\n<http://e/fn/a b> <http://e/p> <http://e/o> .\n";
        let err = require_strict(nt).expect_err("an invalid IRI must fail the build").to_string();
        assert!(err.contains("line 2"), "{}", err);
    }

    #[test]
    fn redaction_covers_every_literal_kind_and_leaves_iris_alone() {
        use crate::ontology::{Iri, Literal, Term, Triple};
        let s = Iri::new("http://e/mail-me");
        let p = Iri::new("http://e/p");
        let mut set = TripleSet::new();
        set.add(Triple::new(s.clone(), p.clone(), Literal::string("ask a@b.io")));
        set.add(Triple::new(s.clone(), p.clone(), Literal::LangString { value: "c@d.org".into(), lang: "en".into() }));
        set.add(Triple::new(s.clone(), p.clone(), Literal::Typed { value: "e@f.com".into(), datatype: Iri::new("http://e/t") }));
        let out = redact_triples(set).to_ntriples();
        assert!(!out.contains('@') || out.contains("@en"), "{}", out);
        assert!(!out.contains("a@b.io") && !out.contains("d.org") && !out.contains("f.com"), "{}", out);
        assert!(out.contains("<http://e/mail-me>"), "the IRI is left alone: {}", out);
        assert!(out.contains("&lt;email&gt;") || out.contains("<email>"), "{}", out);
        let _ = Term::iri("x");
    }

    #[test]
    fn json_redaction_reaches_nested_strings_and_keys_stay() {
        let mut v = serde_json::json!({"files": [{"path": "a.md", "text": "mail x@y.io now", "n": 3}]});
        redact_json(&mut v);
        assert_eq!(v["files"][0]["text"], "mail <email> now");
        assert_eq!(v["files"][0]["n"], 3);
        assert_eq!(v["files"][0]["path"], "a.md");
    }

    #[test]
    fn a_valid_graph_passes_the_strict_check() {
        require_strict("<http://e/s> <http://e/p> \"x y {z}\" .\n").unwrap();
    }

    /// The repository boundary is git's: ignored files (a virtualenv, build
    /// output) are not part of the project, hidden but tracked ones are.
    #[test]
    fn discovery_respects_gitignore_and_keeps_hidden_tracked_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), ".venv-agent/\n*.gen.py\n").unwrap();
        for f in ["src/app.py", ".venv-agent/lib/site.py", "src/skip.gen.py", ".github/scripts/ci.py", "README.md", ".venv-agent/NOTES.md"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x = 1\n").unwrap();
        }
        let rel = |v: Vec<std::path::PathBuf>| -> Vec<String> {
            let mut v: Vec<String> = v.iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect();
            v.sort();
            v
        };
        assert_eq!(rel(discover_files(root, &[Language::Python], &[]).unwrap()), vec![".github/scripts/ci.py", "src/app.py"]);
        assert_eq!(rel(discover_markdown_files(root).unwrap()), vec!["README.md"]);
    }

    #[test]
    fn build_meta_names_the_tool_commit() {
        let (commit, _dirty) = tool_commit();
        assert!(
            commit == "unknown" || (commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit())),
            "{commit}"
        );
    }

    #[test]
    fn sha256_hex_matches_the_standard_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// The inputs manifest is `sha256sum` format sorted by repo-relative path,
    /// so `sha256sum -c .ontosys/data/inputs.sha256` from the repository root
    /// names every input that changed since the build.
    #[test]
    fn inputs_manifest_is_sha256sum_format_sorted_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("b/x.py"), "x = 1\n").unwrap();
        std::fs::write(root.join("a.py"), "").unwrap();
        let manifest = inputs_manifest(root, &[root.join("b/x.py"), root.join("a.py")]).unwrap();
        assert_eq!(manifest, concat!(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a.py\n",
            "9e26bf369911c45c243c684147b23fc9e1dcfcf257d299a1c632016a6fcd33f4  b/x.py\n",
        ));
    }

    /// Files the build does not read are counted per extension, so silence is
    /// never read as coverage (a consumer's report, 2026-10-07). The repository
    /// boundary is the same as discovery's.
    #[test]
    fn unparsed_files_are_counted_per_extension() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), ".venv-agent/\n").unwrap();
        for f in ["src/app.py", "run.sh", "deploy/main.tf", "deploy/vars.tf", "Makefile", ".venv-agent/tool.sh", ".ontosys/data/graph.nt"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x\n").unwrap();
        }
        let counts = unparsed_by_extension(root, |ext| ext == "py");
        let expected: std::collections::BTreeMap<String, usize> =
            [(".sh", 1), (".tf", 2), ("(none)", 2)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(counts, expected, ".gitignore and Makefile have no extension; ignored and .ontosys files are not counted");
    }

    /// The build names the tree it read: HEAD's commit, and whether the
    /// working tree differed from it. `.ontosys/` is output, never input.
    #[test]
    fn git_state_names_head_and_whether_the_tree_differs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(git_state(root), (None, None), "not a repository");
        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git").arg("-C").arg(root)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
                .args(args).output().unwrap();
            assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap()
        };
        git(&["init", "-q"]);
        std::fs::write(root.join("app.py"), "x = 1\n").unwrap();
        git(&["add", "app.py"]);
        git(&["commit", "-q", "--no-verify", "-m", "one"]);
        let head = git(&["rev-parse", "HEAD"]).trim().to_string();
        std::fs::create_dir_all(root.join(".ontosys/data")).unwrap();
        std::fs::write(root.join(".ontosys/data/graph.nt"), "").unwrap();
        assert_eq!(git_state(root), (Some(head.clone()), Some(false)), ".ontosys/ does not make the tree differ");
        std::fs::write(root.join("new.py"), "y = 1\n").unwrap();
        assert_eq!(git_state(root), (Some(head.clone()), Some(true)), "an untracked file the build would read");
        std::fs::remove_file(root.join("new.py")).unwrap();
        std::fs::write(root.join("app.py"), "x = 2\n").unwrap();
        assert_eq!(git_state(root), (Some(head), Some(true)), "a modified tracked file");
    }
}
