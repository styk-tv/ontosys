//! # Complexity
//!
//! Per-function structure (Tier A), call-graph relationships (Tier B), growth
//! estimates (Tier C) and reading claims (Tier D). One language-free engine
//! walks any tree-sitter function body; a per-grammar [`engine::Profile`]
//! says which node kinds are decisions, loops, branches and calls.

pub mod engine;
pub mod join;
pub mod output;
pub mod profiles;
pub mod rdf;
pub mod scan;

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
        Some(match self {
            Grammar::C => &profiles::c::C,
            Grammar::Rust => &profiles::rust::RUST,
            Grammar::Python => &profiles::python::PYTHON,
            Grammar::JavaScript => &profiles::js::JAVASCRIPT,
            Grammar::TypeScript => &profiles::js::TYPESCRIPT,
            Grammar::Tsx => &profiles::js::TSX,
        })
    }

    /// The tree-sitter language, when this build links its grammar.
    pub fn language(self) -> Option<tree_sitter::Language> {
        Some(match self {
            Grammar::C => tree_sitter_c::LANGUAGE.into(),
            Grammar::Rust => tree_sitter_rust::LANGUAGE.into(),
            Grammar::Python => tree_sitter_python::LANGUAGE.into(),
            Grammar::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Grammar::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Grammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        })
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
