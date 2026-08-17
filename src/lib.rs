//! # OntoSys - Code Knowledge Graph System
//!
//! A type-safe multi-language code knowledge graph system that extracts semantic
//! information from Git repositories using AST parsing, maps to formal ontologies
//! (BFO 2020 / CodeOntology), and stores in RDF format.
//!
//! ## Supported Languages
//!
//! - **Rust** (.rs)
//! - **Python** (.py)
//! - **TypeScript** (.ts, .tsx)
//! - **JavaScript** (.js, .jsx, .mjs, .cjs)
//!
//! ## Type-Level Safety Features
//!
//! This library demonstrates several advanced Rust patterns:
//!
//! - **Type-State Pattern**: Pipeline stages are encoded in types, preventing
//!   invalid transitions at compile time
//! - **Capability Tokens**: Zero-cost proofs that operations are authorized
//! - **Sealed Traits**: Closed hierarchies for ontology entities
//! - **Session Types**: Protocol-correct interactions with the graph store
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐     ┌──────────────┐
//! │ Git Repo    │────▶│ AST Parser   │────▶│ RDF Builder │────▶│ Graph Store  │
//! │ (Unchecked) │     │ (multi-lang) │     │ (ontology)  │     │ (validated)  │
//! └─────────────┘     └──────────────┘     └─────────────┘     └──────────────┘
//!       │                    │                    │                    │
//!       ▼                    ▼                    ▼                    ▼
//!   Pipeline::           Pipeline::          Pipeline::           Pipeline::
//!   Discovered           Parsed              Built                Ready
//! ```
//!
//! ## CLI Usage
//!
//! ```bash
//! # Initialize .ontosys/ in a git repository
//! ontosys init
//!
//! # Build the knowledge graph
//! ontosys build
//!
//! # Start visualization server
//! ontosys serve
//!
//! # Query the graph
//! ontosys query "function"
//! ```

pub mod capabilities;
pub mod graph;
pub mod log;
pub mod ontology;
pub mod parser;
pub mod pipeline;

pub use capabilities::{CanParse, CanQuery, CanStore, CanValidate};
pub use graph::{CodeGraph, GraphBuilder};
pub use ontology::{CodeEntity, Iri, Namespace, Triple};
pub use parser::{AstNode, RustParser, PythonParser, TypeScriptParser, MultiLanguageParser};
pub use pipeline::{Pipeline, PipelineBuilder};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::capabilities::*;
    pub use crate::graph::*;
    pub use crate::ontology::*;
    pub use crate::parser::*;
    pub use crate::pipeline::*;
}
