//! # Repository Ingestion Example
//!
//! Demonstrates the full pipeline: Git repo → AST → RDF → SPARQL queries
//!
//! Run with: `cargo run --example ingest_repo -- /path/to/rust/project`

use graph_transformer::prelude::*;
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("═══════════════════════════════════════════════════════════════");
    println!("         GRAPH TRANSFORMER - Repository Ingestion");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Get repository path from args or use current directory
    let repo_path: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            println!("No path provided, using current directory as example\n");
            PathBuf::from(".")
        });

    println!("📁 Repository: {}\n", repo_path.display());

    // ========================================================================
    // PHASE 1: Discovery
    // ========================================================================
    println!("━━━ Phase 1: Discovery ━━━");

    let pipeline = Pipeline::new();
    println!("   Created pipeline in Uninitialized state");

    let pipeline = match pipeline.discover(&repo_path) {
        Ok(p) => {
            println!("   ✅ Discovered {} source files", p.source_files().len());
            for file in p.source_files().iter().take(5) {
                println!("      • {}", file.display());
            }
            if p.source_files().len() > 5 {
                println!("      • ... and {} more", p.source_files().len() - 5);
            }
            p
        }
        Err(e) => {
            println!("   ❌ Discovery failed: {}", e);
            println!("\n   Creating demo with inline source code instead...\n");
            return run_demo_mode();
        }
    };

    // Now we have CanParse capability
    let _parse_cap = pipeline.parse_capability();
    println!("   🔑 Obtained CanParse capability\n");

    // ========================================================================
    // PHASE 2: Parsing
    // ========================================================================
    println!("━━━ Phase 2: Parsing ━━━");

    let pipeline = match pipeline.parse() {
        Ok(p) => {
            println!("   ✅ Parsed {} AST nodes", p.ast_nodes().len());

            // Show breakdown by type
            let mut type_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for node in p.ast_nodes() {
                *type_counts.entry(node.kind()).or_insert(0) += 1;
            }

            for (kind, count) in type_counts.iter() {
                println!("      • {}: {}", kind, count);
            }
            p
        }
        Err(e) => {
            println!("   ❌ Parsing failed: {}", e);
            return Err(e.into());
        }
    };

    // Now we have CanStore capability
    let _store_cap = pipeline.store_capability();
    println!("   🔑 Obtained CanStore capability\n");

    // ========================================================================
    // PHASE 3: Graph Building
    // ========================================================================
    println!("━━━ Phase 3: Graph Building ━━━");

    let pipeline = match pipeline.build() {
        Ok(p) => {
            println!("   ✅ Generated {} RDF triples", p.triples().len());
            p
        }
        Err(e) => {
            println!("   ❌ Build failed: {}", e);
            return Err(e.into());
        }
    };

    // Now we have CanValidate capability
    let _validate_cap = pipeline.validate_capability();
    println!("   🔑 Obtained CanValidate capability\n");

    // ========================================================================
    // PHASE 4: Validation
    // ========================================================================
    println!("━━━ Phase 4: Validation ━━━");

    let pipeline = match pipeline.validate() {
        Ok(p) => {
            if p.is_valid() {
                println!("   ✅ Validation passed");
            } else {
                println!("   ⚠️  Validation warnings:");
                for error in p.validation_errors() {
                    println!("      • {}", error);
                }
            }
            p
        }
        Err(e) => {
            println!("   ❌ Validation failed: {}", e);
            return Err(e.into());
        }
    };

    // Now we have CanQuery capability
    let _query_cap = pipeline.query_capability();
    println!("   🔑 Obtained CanQuery capability\n");

    // ========================================================================
    // PHASE 5: Ready for Queries
    // ========================================================================
    println!("━━━ Phase 5: Ready ━━━");

    let pipeline = pipeline.ready();
    let stats = pipeline.stats();

    println!("   Pipeline Statistics:");
    println!("      • Files processed: {}", stats.files_processed);
    println!("      • AST nodes: {}", stats.ast_nodes);
    println!("      • Triples generated: {}", stats.triples_generated);
    println!("      • Validation errors: {}", stats.validation_errors);
    println!();

    // ========================================================================
    // PHASE 6: Export and Query
    // ========================================================================
    println!("━━━ Phase 6: Export & Query ━━━");

    // Export sample of Turtle
    let turtle = pipeline.export_turtle();
    println!("   Sample Turtle output (first 1000 chars):");
    println!("   ─────────────────────────────────────────");
    for line in turtle.lines().take(30) {
        println!("   {}", line);
    }
    if turtle.lines().count() > 30 {
        println!("   ... ({} more lines)", turtle.lines().count() - 30);
    }
    println!();

    // Demonstrate SPARQL query building
    println!("   Example SPARQL queries:\n");

    let query1 = SparqlBuilder::new()
        .with_default_prefixes()
        .select(&["name", "type"])
        .where_triple("?entity", "rdf:type", "?type")
        .where_triple("?entity", "rdfs:label", "?name")
        .filter("?type = code:Function || ?type = code:Struct")
        .limit(10)
        .build();

    println!("   Query 1: Find all functions and structs");
    println!("   ─────────────────────────────────────────");
    for line in query1.lines() {
        println!("   {}", line);
    }
    println!();

    let query2 = SparqlBuilder::new()
        .with_default_prefixes()
        .select(&["caller", "callee"])
        .where_triple("?caller", "code:calls", "?callee")
        .build();

    println!("   Query 2: Find call graph relationships");
    println!("   ─────────────────────────────────────────");
    for line in query2.lines() {
        println!("   {}", line);
    }
    println!();

    println!("═══════════════════════════════════════════════════════════════");
    println!("   Pipeline completed successfully!");
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}

/// Demo mode with inline source code
fn run_demo_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("━━━ Demo Mode: Processing Inline Code ━━━\n");

    // Create a demo AST manually
    let demo_source = r#"
/// A simple HTTP client
pub struct HttpClient {
    base_url: String,
    timeout: Duration,
}

impl HttpClient {
    /// Create a new HTTP client
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Send a GET request
    pub async fn get(&self, path: &str) -> Result<Response, Error> {
        self.request(Method::GET, path, None).await
    }

    /// Send a POST request
    pub async fn post(&self, path: &str, body: &str) -> Result<Response, Error> {
        self.request(Method::POST, path, Some(body)).await
    }
}

pub trait Serialize {
    fn serialize(&self) -> Vec<u8>;
}

impl Serialize for HttpClient {
    fn serialize(&self) -> Vec<u8> {
        todo!()
    }
}
"#;

    println!("   Demo source code:");
    println!("   ─────────────────");
    for line in demo_source.lines().take(20) {
        println!("   {}", line);
    }
    println!("   ...\n");

    // Parse the demo source
    let parser = RustParser::new()?;
    let nodes = parser.parse_file(std::path::Path::new("demo.rs"), demo_source)?;

    println!("   Parsed {} AST nodes:", nodes.len());
    for node in &nodes {
        println!("      • {} '{}'", node.kind(), node.name());
    }
    println!();

    // Build the graph
    let builder = GraphBuilder::new("demo-project");
    let triples = builder.build_from_ast(&nodes)?;

    println!("   Generated {} RDF triples\n", triples.len());

    // Show some triples
    println!("   Sample triples:");
    println!("   ─────────────────");
    for triple in triples.iter().take(15) {
        println!("   {}", triple);
    }
    println!();

    // Export Turtle
    let turtle = triples.to_turtle();
    println!("   Turtle output:");
    println!("   ─────────────────");
    for line in turtle.lines().take(40) {
        println!("   {}", line);
    }

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("   Demo completed!");
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
