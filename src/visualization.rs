//! # Visualization Module
//!
//! Graph visualization utilities.

// Visualization is handled in cli/serve.rs with embedded HTML/JS
// This module provides helper functions for data transformation

use crate::ontology::TripleSet;

/// Transform triples into visualization-friendly format
pub fn triples_to_vis_data(triples: &TripleSet) -> serde_json::Value {
    // The main transformation is done in cli/build.rs
    // This module can be extended with additional utilities

    serde_json::json!({
        "nodes": [],
        "edges": []
    })
}
