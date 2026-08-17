//! # Clean Command
//!
//! Clean the .ontosys folder.

use super::*;
use std::path::Path;
use std::fs;
use console::style;

pub async fn run(repo_path: &Path, all: bool) -> anyhow::Result<()> {
    println!("\n{}", style("OntoSys Clean").cyan().bold());
    println!("{}\n", style("═".repeat(50)).dim());

    let git_root = find_git_root(repo_path).ok_or_else(|| {
        anyhow::anyhow!("Not a git repository")
    })?;

    let ontosys_path = ontosys_dir(&git_root);
    if !ontosys_path.exists() {
        info("Nothing to clean - .ontosys folder doesn't exist.");
        return Ok(());
    }

    if all {
        warn("Removing entire .ontosys folder...");
        fs::remove_dir_all(&ontosys_path)?;
        success("Cleaned all OntoSys data");
        info("Run 'ontosys init' to reinitialize");
    } else {
        info("Cleaning generated files...");

        // Remove cache
        let cache_path = ontosys_path.join("cache");
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path)?;
            fs::create_dir_all(&cache_path)?;
            println!("  {} Cleaned cache/", style("→").dim());
        }

        // Remove exports
        let exports_path = ontosys_path.join("exports");
        if exports_path.exists() {
            fs::remove_dir_all(&exports_path)?;
            fs::create_dir_all(&exports_path)?;
            println!("  {} Cleaned exports/", style("→").dim());
        }

        // Remove generated data files but keep the data directory
        let data_path = ontosys_path.join("data");
        if data_path.exists() {
            for entry in fs::read_dir(&data_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    fs::remove_file(&path)?;
                    println!("  {} Removed {}", style("→").dim(),
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"));
                }
            }
        }

        println!();
        success("Cleaned generated files");
        info("Configuration preserved. Run 'ontosys build' to rebuild.");
    }

    println!();
    Ok(())
}
