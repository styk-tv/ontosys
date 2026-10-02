//! # OntoSys CLI
//!
//! Multi-language code knowledge graph system.
//!
//! ## Usage
//!
//! ```bash
//! # Initialize in current repo (creates .ontosys/ folder)
//! ontosys init
//!
//! # Process all files and generate RDF
//! ontosys build
//!
//! # Start visualization server
//! ontosys serve
//!
//! # Query the graph
//! ontosys query "SELECT ?name WHERE { ?f a code:Function ; rdfs:label ?name }"
//! ```

mod capabilities;
mod graph;
mod grounding;
mod lang_c;
mod log;
mod ontology;
mod parser;
mod pipeline;
mod cli;
mod visualization;

use clap::{Parser, Subcommand};
use console::style;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ontosys")]
#[command(author = "Peter Styk")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Multi-language code knowledge graph system", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to repository (defaults to current directory)
    #[arg(short, long, global = true)]
    path: Option<PathBuf>,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize .ontosys/ folder in a git repository
    Init {
        /// Force re-initialization even if .ontosys exists
        #[arg(short, long)]
        force: bool,
    },

    /// Build/rebuild the knowledge graph from source files
    Build {
        /// Only process files changed since last build
        #[arg(short, long)]
        incremental: bool,

        /// Languages to process (rust, python, typescript, javascript)
        #[arg(short, long, value_delimiter = ',')]
        languages: Option<Vec<String>>,

        /// Number of parallel workers
        #[arg(short, long, default_value = "4")]
        jobs: usize,
    },

    /// Show statistics about the knowledge graph
    Stats,

    /// Start the visualization web server
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Open browser automatically
        #[arg(short, long)]
        open: bool,

        /// Also load another build (repository with .ontosys/, or a graph.nt)
        /// as the baseline, to explore the delta
        #[arg(short, long)]
        compare: Option<PathBuf>,
    },

    /// Run a SPARQL query against the built graph
    Query {
        /// SPARQL query string
        query: String,

        /// Output format (json, table, csv)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Export the graph to a file
    Export {
        /// Output file path
        output: PathBuf,

        /// Format (turtle, ntriples, jsonld)
        #[arg(short, long, default_value = "turtle")]
        format: String,
    },

    /// Watch for file changes and rebuild automatically
    Watch {
        /// Debounce time in milliseconds
        #[arg(short, long, default_value = "500")]
        debounce: u64,
    },

    /// Semantic diff of two builds (repository paths or graph.nt files)
    Diff {
        /// Old build: a repository with .ontosys/, or a graph.nt file
        old: PathBuf,

        /// New build: a repository with .ontosys/, or a graph.nt file
        new: PathBuf,

        /// Markdown report path (default: ontosys-diff-<old>-<new>.md)
        #[arg(short, long)]
        out: Option<PathBuf>,

        /// Also write the entity-level changes as JSON
        #[arg(long)]
        json: Option<PathBuf>,
    },

    /// Clean the .ontosys folder
    Clean {
        /// Remove everything including configuration
        #[arg(long)]
        all: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Setup logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("ontosys=debug")
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("ontosys=info")
            .init();
    }

    // Determine repository path
    let repo_path = cli.path.unwrap_or_else(|| std::env::current_dir().unwrap());

    match cli.command {
        Commands::Init { force } => {
            cli::init::run(&repo_path, force).await
        }
        Commands::Build { incremental, languages, jobs } => {
            cli::build::run(&repo_path, incremental, languages, jobs).await
        }
        Commands::Stats => {
            cli::stats::run(&repo_path).await
        }
        Commands::Serve { port, open, compare } => {
            cli::serve::run(&repo_path, port, open, compare).await
        }
        Commands::Query { query, format } => {
            cli::query::run(&repo_path, &query, &format).await
        }
        Commands::Export { output, format } => {
            cli::export::run(&repo_path, &output, &format).await
        }
        Commands::Watch { debounce } => {
            cli::watch::run(&repo_path, debounce).await
        }
        Commands::Diff { old, new, out, json } => {
            cli::diff::run(&old, &new, out, json).await
        }
        Commands::Clean { all } => {
            cli::clean::run(&repo_path, all).await
        }
    }
}
