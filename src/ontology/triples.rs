//! # RDF Triple Representation
//!
//! Core triple type and collections for building RDF graphs.

use super::{Iri, Literal, Term};
use std::fmt;

/// An RDF triple (subject, predicate, object)
#[derive(Debug, Clone, PartialEq)]
pub struct Triple {
    pub subject: Term,
    pub predicate: Iri,
    pub object: Term,
}

impl Triple {
    /// Create a new triple from IRIs
    pub fn new(subject: impl Into<Term>, predicate: Iri, object: impl Into<Term>) -> Self {
        Self {
            subject: subject.into(),
            predicate,
            object: object.into(),
        }
    }

    /// Create a type assertion triple (s rdf:type o)
    pub fn type_of(subject: impl Into<Term>, class: Iri) -> Self {
        Self::new(subject, super::namespaces::rdf::type_(), class)
    }

    /// Create a label triple (s rdfs:label "label")
    pub fn label(subject: impl Into<Term>, label: &str) -> Self {
        Self::new(
            subject,
            super::namespaces::rdfs::label(),
            Literal::string(label),
        )
    }

    /// Convert to N-Triples format
    pub fn to_ntriples(&self) -> String {
        format!("{} {} {} .", self.subject, self.predicate, self.object)
    }

    /// Convert to Turtle format (with prefixes would need context)
    pub fn to_turtle(&self) -> String {
        self.to_ntriples() // Simplified; full Turtle needs prefix context
    }
}

impl fmt::Display for Triple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_ntriples())
    }
}

// ============================================================================
// TRIPLE COLLECTION
// ============================================================================

/// A collection of triples forming a graph
#[derive(Debug, Clone, Default)]
pub struct TripleSet {
    triples: Vec<Triple>,
}

impl TripleSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            triples: Vec::with_capacity(capacity),
        }
    }

    pub fn add(&mut self, triple: Triple) {
        self.triples.push(triple);
    }

    pub fn extend(&mut self, triples: impl IntoIterator<Item = Triple>) {
        self.triples.extend(triples);
    }

    pub fn len(&self) -> usize {
        self.triples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.triples.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Triple> {
        self.triples.iter()
    }

    pub fn into_vec(self) -> Vec<Triple> {
        self.triples
    }

    /// Serialize to N-Triples format
    pub fn to_ntriples(&self) -> String {
        self.triples
            .iter()
            .map(|t| t.to_ntriples())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Serialize to Turtle format with prefixes
    pub fn to_turtle(&self) -> String {
        let prefixes = r#"@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix dc: <http://purl.org/dc/elements/1.1/> .
@prefix dct: <http://purl.org/dc/terms/> .
@prefix prov: <http://www.w3.org/ns/prov#> .
@prefix doap: <http://usefulinc.com/ns/doap#> .
@prefix co: <http://codeontology.org/ontology/> .
@prefix code: <http://example.org/code/> .
@prefix data: <http://example.org/data/> .

"#;
        format!("{}{}", prefixes, self.to_ntriples())
    }
}

impl IntoIterator for TripleSet {
    type Item = Triple;
    type IntoIter = std::vec::IntoIter<Triple>;

    fn into_iter(self) -> Self::IntoIter {
        self.triples.into_iter()
    }
}

impl<'a> IntoIterator for &'a TripleSet {
    type Item = &'a Triple;
    type IntoIter = std::slice::Iter<'a, Triple>;

    fn into_iter(self) -> Self::IntoIter {
        self.triples.iter()
    }
}

impl FromIterator<Triple> for TripleSet {
    fn from_iter<I: IntoIterator<Item = Triple>>(iter: I) -> Self {
        Self {
            triples: iter.into_iter().collect(),
        }
    }
}

impl Extend<Triple> for TripleSet {
    fn extend<I: IntoIterator<Item = Triple>>(&mut self, iter: I) {
        self.triples.extend(iter);
    }
}

// ============================================================================
// QUAD (Named Graph Support)
// ============================================================================

/// An RDF quad (triple + graph name) for named graph support
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    pub triple: Triple,
    pub graph: Option<Iri>,
}

impl Quad {
    pub fn new(triple: Triple, graph: Option<Iri>) -> Self {
        Self { triple, graph }
    }

    pub fn in_default_graph(triple: Triple) -> Self {
        Self {
            triple,
            graph: None,
        }
    }

    pub fn in_named_graph(triple: Triple, graph: Iri) -> Self {
        Self {
            triple,
            graph: Some(graph),
        }
    }

    /// Convert to N-Quads format
    pub fn to_nquads(&self) -> String {
        match &self.graph {
            Some(g) => format!(
                "{} {} {} {} .",
                self.triple.subject, self.triple.predicate, self.triple.object, g
            ),
            None => self.triple.to_ntriples(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::namespaces::*;

    #[test]
    fn create_triple() {
        let triple = Triple::new(
            Iri::new("http://example.org/subject"),
            rdf::type_(),
            Iri::new("http://example.org/Class"),
        );

        let ntriples = triple.to_ntriples();
        assert!(ntriples.contains("http://example.org/subject"));
        assert!(ntriples.contains("rdf-syntax-ns#type"));
    }

    #[test]
    fn triple_set_operations() {
        let mut set = TripleSet::new();

        set.add(Triple::label(
            Iri::new("http://example.org/thing"),
            "Thing",
        ));

        set.add(Triple::type_of(
            Iri::new("http://example.org/thing"),
            Iri::new("http://example.org/Class"),
        ));

        assert_eq!(set.len(), 2);

        let turtle = set.to_turtle();
        assert!(turtle.contains("@prefix"));
    }

    #[test]
    fn quad_with_named_graph() {
        let triple = Triple::label(Iri::new("http://example.org/s"), "test");
        let quad = Quad::in_named_graph(triple, Iri::new("http://example.org/graph1"));

        let nquads = quad.to_nquads();
        assert!(nquads.contains("graph1"));
    }
}
