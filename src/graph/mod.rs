//! # Graph Generation Module
//!
//! Transforms AST nodes into RDF triples following the CodeOntology.

mod builder;
mod store;
pub mod docs_builder;

pub use builder::*;
pub use store::*;

use crate::ontology::TripleSet;

/// A code knowledge graph
#[derive(Debug, Clone)]
pub struct CodeGraph {
    /// The project this graph represents
    pub project_id: String,
    /// All triples in the graph
    pub triples: TripleSet,
}

impl CodeGraph {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            triples: TripleSet::new(),
        }
    }

    pub fn with_triples(project_id: impl Into<String>, triples: TripleSet) -> Self {
        Self {
            project_id: project_id.into(),
            triples,
        }
    }

    pub fn triple_count(&self) -> usize {
        self.triples.len()
    }

    pub fn export_turtle(&self) -> String {
        self.triples.to_turtle()
    }

    pub fn export_ntriples(&self) -> String {
        self.triples.to_ntriples()
    }
}
