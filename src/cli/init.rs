//! # Init Command
//!
//! Initialize .ontosys/ folder and run initial build.

use super::*;
use crate::log::{Log, Action};
use std::path::Path;
use std::fs;

/// Directory structure for .ontosys/
const DIRS: &[&str] = &[
    "data",           // RDF data files
    "data/graphs",    // Named graphs
    "cache",          // Parsed AST cache
    "exports",        // Exported files
    "viz",            // Visualization assets
];

pub async fn run(repo_path: &Path, force: bool) -> anyhow::Result<()> {
    let log = Log::new();

    // Find git root
    let git_root = find_git_root(repo_path).ok_or_else(|| {
        anyhow::anyhow!("Not a git repository. Run 'git init' first.")
    })?;

    let ontosys_path = ontosys_dir(&git_root);

    // Check if already initialized
    if ontosys_path.exists() && !force {
        log.warn("Already initialized. Use --force to reinitialize.");
        return Ok(());
    }

    // Remove existing if force
    if ontosys_path.exists() && force {
        log.action(Action::Clean, "Removing existing .ontosys");
        fs::remove_dir_all(&ontosys_path)?;
        log.ok("Removed");
    }

    log.blank();

    // Create directory structure
    log.action(Action::Init, "Creating .ontosys directory");
    for dir in DIRS {
        let path = ontosys_path.join(dir);
        fs::create_dir_all(&path)?;
    }
    create_config(&ontosys_path, &git_root)?;
    create_gitignore(&ontosys_path)?;
    log.ok("Created .ontosys/");

    log.blank();

    // Run build to process the repository
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    super::build::run(&git_root, false, None, num_cpus).await?;

    log.blank();
    log.info("Next: run 'ontosys serve' to explore the graph");

    Ok(())
}

fn create_config(ontosys_path: &Path, _git_root: &Path) -> anyhow::Result<()> {
    let config = Config {
        version: "0.1.0".to_string(),
        created: chrono::Utc::now().to_rfc3339(),
        languages: vec![
            "rust".to_string(),
            "python".to_string(),
            "typescript".to_string(),
            "javascript".to_string(),
        ],
        exclude_patterns: vec![
            "**/node_modules/**".to_string(),
            "**/target/**".to_string(),
            "**/__pycache__/**".to_string(),
        ],
        include_private: true,
        extract_docs: true,
        extract_calls: true,
    };

    let config_path = ontosys_path.join("config.json");
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

fn create_gitignore(ontosys_path: &Path) -> anyhow::Result<()> {
    let content = r#"# OntoSys generated files
cache/
*.log

# Keep data and config
!data/
!config.json
"#;
    fs::write(ontosys_path.join(".gitignore"), content)?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub version: String,
    pub created: String,
    pub languages: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub include_private: bool,
    pub extract_docs: bool,
    pub extract_calls: bool,
}
