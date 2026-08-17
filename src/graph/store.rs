//! # Graph Store Integration
//!
//! Integration with Oxigraph for RDF storage and SPARQL queries.
//!
//! This module provides type-safe wrappers around Oxigraph operations
//! using session types to enforce correct usage patterns.

use crate::ontology::{Iri, Triple, TripleSet};
use crate::capabilities::{CanQuery, CanStore};
use std::marker::PhantomData;

// ============================================================================
// STORE STATES (Session Types)
// ============================================================================

/// Store is closed
pub struct Closed;

/// Store is open for reading
pub struct ReadOnly;

/// Store is open for writing
pub struct ReadWrite;

/// Transaction in progress
pub struct InTransaction;

// ============================================================================
// GRAPH STORE
// ============================================================================

/// A type-safe wrapper around an RDF graph store.
///
/// Uses session types to ensure operations happen in the correct order.
pub struct GraphStore<State> {
    // In production, this would hold the Oxigraph store:
    // store: oxigraph::store::Store,
    triples: TripleSet,
    _state: PhantomData<State>,
}

impl GraphStore<Closed> {
    /// Create a new in-memory graph store
    pub fn new_memory() -> Self {
        Self {
            triples: TripleSet::new(),
            _state: PhantomData,
        }
    }

    /// Open the store for reading
    pub fn open_read(self) -> GraphStore<ReadOnly> {
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }

    /// Open the store for reading and writing
    pub fn open_write(self) -> GraphStore<ReadWrite> {
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }
}

impl GraphStore<ReadOnly> {
    /// Query the store (requires query capability)
    pub fn query(&self, _capability: &CanQuery, sparql: &str) -> QueryResult {
        // In production:
        // self.store.query(sparql)
        QueryResult {
            sparql: sparql.to_string(),
            bindings: vec![],
        }
    }

    /// Get all triples matching a pattern
    pub fn find_triples(
        &self,
        subject: Option<&Iri>,
        predicate: Option<&Iri>,
        object: Option<&Iri>,
    ) -> Vec<&Triple> {
        self.triples
            .iter()
            .filter(|t| {
                subject.map_or(true, |s| {
                    if let crate::ontology::Term::Iri(iri) = &t.subject {
                        iri == s
                    } else {
                        false
                    }
                }) && predicate.map_or(true, |p| &t.predicate == p)
                    && object.map_or(true, |o| {
                        if let crate::ontology::Term::Iri(iri) = &t.object {
                            iri == o
                        } else {
                            false
                        }
                    })
            })
            .collect()
    }

    /// Count total triples
    pub fn triple_count(&self) -> usize {
        self.triples.len()
    }

    /// Close the store
    pub fn close(self) -> GraphStore<Closed> {
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }
}

impl GraphStore<ReadWrite> {
    /// Insert triples (requires store capability)
    pub fn insert(&mut self, _capability: &CanStore, triples: TripleSet) {
        self.triples.extend(triples);
    }

    /// Insert a single triple
    pub fn insert_triple(&mut self, _capability: &CanStore, triple: Triple) {
        self.triples.add(triple);
    }

    /// Begin a transaction
    pub fn begin_transaction(self) -> GraphStore<InTransaction> {
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }

    /// Query the store
    pub fn query(&self, _capability: &CanQuery, sparql: &str) -> QueryResult {
        QueryResult {
            sparql: sparql.to_string(),
            bindings: vec![],
        }
    }

    /// Close the store
    pub fn close(self) -> GraphStore<Closed> {
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }

    /// Downgrade to read-only
    pub fn downgrade(self) -> GraphStore<ReadOnly> {
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }
}

impl GraphStore<InTransaction> {
    /// Insert triples within transaction
    pub fn insert(&mut self, _capability: &CanStore, triples: TripleSet) {
        self.triples.extend(triples);
    }

    /// Commit the transaction
    pub fn commit(self) -> GraphStore<ReadWrite> {
        // In production: self.store.commit()
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }

    /// Rollback the transaction
    pub fn rollback(self) -> GraphStore<ReadWrite> {
        // In production: self.store.rollback()
        // For now, we'd need to track the original state
        GraphStore {
            triples: self.triples,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// QUERY RESULTS
// ============================================================================

/// Result of a SPARQL query
#[derive(Debug)]
pub struct QueryResult {
    pub sparql: String,
    pub bindings: Vec<QueryBinding>,
}

/// A single binding in query results
#[derive(Debug)]
pub struct QueryBinding {
    pub variables: Vec<(String, String)>,
}

impl QueryResult {
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }
}

// ============================================================================
// STORE BUILDER
// ============================================================================

/// Builder for creating graph stores with various configurations
pub struct StoreBuilder {
    backend: StoreBackend,
    namespaces: Vec<(String, String)>,
}

#[derive(Default)]
pub enum StoreBackend {
    #[default]
    Memory,
    #[allow(dead_code)]
    File(std::path::PathBuf),
}

impl StoreBuilder {
    pub fn new() -> Self {
        Self {
            backend: StoreBackend::Memory,
            namespaces: vec![],
        }
    }

    pub fn memory(mut self) -> Self {
        self.backend = StoreBackend::Memory;
        self
    }

    #[allow(dead_code)]
    pub fn file(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.backend = StoreBackend::File(path.into());
        self
    }

    pub fn with_namespace(mut self, prefix: &str, uri: &str) -> Self {
        self.namespaces.push((prefix.to_string(), uri.to_string()));
        self
    }

    pub fn with_default_namespaces(self) -> Self {
        self.with_namespace("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
            .with_namespace("rdfs", "http://www.w3.org/2000/01/rdf-schema#")
            .with_namespace("owl", "http://www.w3.org/2002/07/owl#")
            .with_namespace("xsd", "http://www.w3.org/2001/XMLSchema#")
            .with_namespace("co", "http://codeontology.org/ontology/")
            .with_namespace("code", "http://example.org/code/")
            .with_namespace("data", "http://example.org/data/")
    }

    pub fn build(self) -> GraphStore<Closed> {
        GraphStore::new_memory()
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SPARQL QUERY BUILDER
// ============================================================================

/// Builder for constructing SPARQL queries in a type-safe manner
pub struct SparqlBuilder {
    prefixes: Vec<String>,
    select: Vec<String>,
    where_clauses: Vec<String>,
    filters: Vec<String>,
    order_by: Option<String>,
    limit: Option<usize>,
}

impl SparqlBuilder {
    pub fn new() -> Self {
        Self {
            prefixes: vec![],
            select: vec![],
            where_clauses: vec![],
            filters: vec![],
            order_by: None,
            limit: None,
        }
    }

    pub fn prefix(mut self, prefix: &str, uri: &str) -> Self {
        self.prefixes.push(format!("PREFIX {}: <{}>", prefix, uri));
        self
    }

    pub fn with_default_prefixes(self) -> Self {
        self.prefix("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
            .prefix("rdfs", "http://www.w3.org/2000/01/rdf-schema#")
            .prefix("co", "http://codeontology.org/ontology/")
            .prefix("code", "http://example.org/code/")
    }

    pub fn select(mut self, vars: &[&str]) -> Self {
        self.select = vars.iter().map(|v| format!("?{}", v)).collect();
        self
    }

    pub fn select_all(mut self) -> Self {
        self.select = vec!["*".to_string()];
        self
    }

    pub fn where_triple(mut self, subject: &str, predicate: &str, object: &str) -> Self {
        self.where_clauses.push(format!("{} {} {} .", subject, predicate, object));
        self
    }

    pub fn filter(mut self, condition: &str) -> Self {
        self.filters.push(format!("FILTER({})", condition));
        self
    }

    pub fn order_by(mut self, var: &str, descending: bool) -> Self {
        let dir = if descending { "DESC" } else { "ASC" };
        self.order_by = Some(format!("ORDER BY {}(?{})", dir, var));
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn build(self) -> String {
        let mut query = String::new();

        // Prefixes
        for prefix in &self.prefixes {
            query.push_str(prefix);
            query.push('\n');
        }

        // SELECT
        query.push_str(&format!("SELECT {}\n", self.select.join(" ")));

        // WHERE
        query.push_str("WHERE {\n");
        for clause in &self.where_clauses {
            query.push_str("  ");
            query.push_str(clause);
            query.push('\n');
        }
        for filter in &self.filters {
            query.push_str("  ");
            query.push_str(filter);
            query.push('\n');
        }
        query.push_str("}\n");

        // ORDER BY
        if let Some(order) = &self.order_by {
            query.push_str(order);
            query.push('\n');
        }

        // LIMIT
        if let Some(limit) = self.limit {
            query.push_str(&format!("LIMIT {}\n", limit));
        }

        query
    }
}

impl Default for SparqlBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::grants;

    #[test]
    fn store_state_transitions() {
        let store = GraphStore::new_memory();

        // Open for writing
        let mut store = store.open_write();

        // Insert some data
        let capability = grants::grant_store();
        store.insert(&capability, TripleSet::new());

        // Begin transaction
        let store = store.begin_transaction();

        // Commit
        let store = store.commit();

        // Close
        let _store = store.close();
    }

    #[test]
    fn sparql_builder() {
        let query = SparqlBuilder::new()
            .with_default_prefixes()
            .select(&["name", "type"])
            .where_triple("?entity", "rdf:type", "code:Function")
            .where_triple("?entity", "rdfs:label", "?name")
            .filter("CONTAINS(?name, \"parse\")")
            .order_by("name", false)
            .limit(10)
            .build();

        assert!(query.contains("SELECT ?name ?type"));
        assert!(query.contains("rdf:type"));
        assert!(query.contains("FILTER"));
        assert!(query.contains("LIMIT 10"));
    }

    #[test]
    fn store_builder() {
        let store = StoreBuilder::new()
            .memory()
            .with_default_namespaces()
            .build();

        let store = store.open_read();
        assert_eq!(store.triple_count(), 0);
    }
}
