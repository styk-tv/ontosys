//! # Complexity
//!
//! Per-function structure (Tier A), call-graph relationships (Tier B), growth
//! estimates (Tier C) and reading claims (Tier D). One language-free engine
//! walks any tree-sitter function body; a per-grammar [`engine::Profile`]
//! says which node kinds are decisions, loops, branches and calls.

pub mod engine;
pub mod profiles;
pub mod rdf;

use engine::Profile;

/// A tree-sitter grammar ontosys analyses. TypeScript and TSX are separate
/// grammars; `.jsx` uses the JavaScript grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Grammar {
    C,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
}

impl Grammar {
    /// The grammar for a repository path, by extension.
    pub fn for_path(path: &str) -> Option<Grammar> {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        Some(match ext.as_str() {
            "c" | "h" => Grammar::C,
            "rs" => Grammar::Rust,
            "py" => Grammar::Python,
            "js" | "jsx" | "mjs" | "cjs" => Grammar::JavaScript,
            "ts" => Grammar::TypeScript,
            "tsx" => Grammar::Tsx,
            _ => return None,
        })
    }

    /// The analysis profile, when this grammar has one in this build.
    pub fn profile(self) -> Option<&'static dyn Profile> {
        match self {
            Grammar::C => Some(&profiles::c::C),
            _ => None,
        }
    }

    /// The tree-sitter language, when this build links its grammar.
    pub fn language(self) -> Option<tree_sitter::Language> {
        match self {
            Grammar::C => Some(tree_sitter_c::LANGUAGE.into()),
            _ => None,
        }
    }

    /// Short lowercase name, as used in build-meta keys.
    pub fn name(self) -> &'static str {
        match self {
            Grammar::C => "c",
            Grammar::Rust => "rust",
            Grammar::Python => "python",
            Grammar::JavaScript => "javascript",
            Grammar::TypeScript => "typescript",
            Grammar::Tsx => "tsx",
        }
    }
}
