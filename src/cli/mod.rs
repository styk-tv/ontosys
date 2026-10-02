//! # CLI Commands
//!
//! Implementation of all CLI subcommands.

pub mod init;
pub mod build;
pub mod stats;
pub mod serve;
pub mod query;
pub mod export;
pub mod watch;
pub mod clean;
pub mod diff;
pub mod store;

use std::path::Path;
use console::style;

/// The .ontosys folder name
pub const ONTOSYS_DIR: &str = ".ontosys";

/// Check if we're in a git repository
pub fn find_git_root(start: &Path) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Stable project name used in every instance IRI.
///
/// Order: `project` in config.json → the basename of `remote.origin.url` → the
/// checkout directory name. Two worktrees of one repository share a remote, so
/// they share IRIs; the directory name is only a last resort.
pub fn project_name(git_root: &Path, configured: Option<&str>) -> String {
    if let Some(p) = configured.filter(|p| !p.is_empty()) {
        return p.to_string();
    }
    let remote = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(git_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let from_remote = remote
        .trim_end_matches('/')
        .rsplit(|c| c == '/' || c == ':')
        .next()
        .unwrap_or("")
        .trim_end_matches(".git")
        .to_string();
    if !from_remote.is_empty() {
        return from_remote;
    }
    git_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string()
}

/// Get the .ontosys directory path
pub fn ontosys_dir(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(ONTOSYS_DIR)
}

/// Print a success message
pub fn success(msg: &str) {
    println!("{} {}", style("✓").green().bold(), msg);
}

/// Print an info message
pub fn info(msg: &str) {
    println!("{} {}", style("ℹ").blue().bold(), msg);
}

/// Print a warning message
pub fn warn(msg: &str) {
    println!("{} {}", style("⚠").yellow().bold(), msg);
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("{} {}", style("✗").red().bold(), msg);
}

/// Supported languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    C,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Language::Rust),
            "py" => Some(Language::Python),
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "c" | "h" => Some(Language::C),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::TypeScript => "TypeScript",
            Language::JavaScript => "JavaScript",
            Language::C => "C",
        }
    }

    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Language::Rust => &["rs"],
            Language::Python => &["py"],
            Language::TypeScript => &["ts", "tsx"],
            Language::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Language::C => &["c", "h"],
        }
    }

    pub fn all() -> &'static [Language] {
        &[Language::Rust, Language::Python, Language::TypeScript, Language::JavaScript, Language::C]
    }
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rust" | "rs" => Ok(Language::Rust),
            "python" | "py" => Ok(Language::Python),
            "typescript" | "ts" => Ok(Language::TypeScript),
            "javascript" | "js" => Ok(Language::JavaScript),
            "c" => Ok(Language::C),
            _ => Err(format!("Unknown language: {}", s)),
        }
    }
}
