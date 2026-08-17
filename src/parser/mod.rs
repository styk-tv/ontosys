//! # AST Parser Module
//!
//! Multi-language parser supporting Rust, Python, TypeScript, and JavaScript.
//! Extracts semantic information for knowledge graph construction.

mod rust_parser;
mod python_parser;
mod typescript_parser;
mod ast_node;
mod visitor;
pub mod comment_extractor;
pub mod markdown_parser;

pub use rust_parser::*;
pub use python_parser::*;
pub use typescript_parser::*;
pub use ast_node::*;
pub use visitor::*;

use crate::pipeline::PipelineError;
use std::path::Path;

/// Multi-language parser that dispatches to the appropriate language parser
pub struct MultiLanguageParser {
    rust: RustParser,
    python: PythonParser,
    typescript: TypeScriptParser,
}

impl MultiLanguageParser {
    pub fn new() -> Result<Self, PipelineError> {
        Ok(Self {
            rust: RustParser::new()?,
            python: PythonParser::new()?,
            typescript: TypeScriptParser::new()?,
        })
    }

    /// Parse a file, automatically detecting the language from extension
    pub fn parse_file(&self, path: &Path, source: &str) -> Result<Vec<AstNode>, PipelineError> {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext.to_lowercase().as_str() {
            "rs" => self.rust.parse_file(path, source),
            "py" => self.python.parse_file(path, source),
            "ts" | "tsx" => self.typescript.parse_file(path, source),
            "js" | "jsx" | "mjs" | "cjs" => self.typescript.parse_file(path, source),
            _ => Err(PipelineError::ParseError(format!("Unsupported file extension: {}", ext))),
        }
    }

    /// Get supported extensions
    pub fn supported_extensions() -> &'static [&'static str] {
        &["rs", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_language_parser_dispatch() {
        let parser = MultiLanguageParser::new().unwrap();

        // Rust
        let rust_source = "pub fn main() {}";
        let nodes = parser.parse_file(Path::new("test.rs"), rust_source).unwrap();
        assert!(!nodes.is_empty());

        // Python
        let py_source = "def main(): pass";
        let nodes = parser.parse_file(Path::new("test.py"), py_source).unwrap();
        assert!(!nodes.is_empty());

        // TypeScript
        let ts_source = "export function main(): void {}";
        let nodes = parser.parse_file(Path::new("test.ts"), ts_source).unwrap();
        assert!(!nodes.is_empty());

        // JavaScript
        let js_source = "export function main() {}";
        let nodes = parser.parse_file(Path::new("test.js"), js_source).unwrap();
        assert!(!nodes.is_empty());
    }
}
