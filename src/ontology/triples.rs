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

    /// Put the set into canonical form: sorted by N-Triples line, duplicates removed.
    ///
    /// Output order otherwise depends on parallel parse order; canonical order makes
    /// two builds of identical sources byte-identical, which the diff relies on.
    pub fn canonicalize(&mut self) {
        let mut keyed: Vec<(String, Triple)> = std::mem::take(&mut self.triples)
            .into_iter()
            .map(|t| (t.to_ntriples(), t))
            .collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        keyed.dedup_by(|a, b| a.0 == b.0);
        self.triples = keyed.into_iter().map(|(_, t)| t).collect();
    }

    /// Serialize to N-Triples format
    pub fn to_ntriples(&self) -> String {
        let mut out = String::with_capacity(self.triples.len() * 160);
        for t in &self.triples {
            out.push_str(&t.to_ntriples());
            out.push('\n');
        }
        out
    }

    /// Serialize to Turtle: prefixed predicates/classes, statements grouped by subject.
    pub fn to_turtle(&self) -> String {
        use super::namespaces::ALL_PREFIXES;
        let mut out = String::with_capacity(self.triples.len() * 90);
        for ns in ALL_PREFIXES {
            out.push_str(&format!("@prefix {}: <{}> .\n", ns.short(), ns.prefix()));
        }
        out.push('\n');

        let compact = |iri: &super::Iri| -> String {
            let s = iri.as_str();
            for ns in ALL_PREFIXES {
                if let Some(local) = s.strip_prefix(ns.prefix()) {
                    let simple = !local.is_empty()
                        && local.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        && local.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_');
                    if simple {
                        return format!("{}:{}", ns.short(), local);
                    }
                }
            }
            format!("<{}>", s)
        };
        let term = |t: &Term| -> String {
            match t {
                Term::Iri(i) => compact(i),
                Term::BlankNode(b) => format!("_:{}", b),
                Term::Literal(l) => l.to_string(),
            }
        };

        let mut i = 0;
        while i < self.triples.len() {
            let subj = &self.triples[i].subject;
            out.push_str(&term(subj));
            let mut first = true;
            while i < self.triples.len() && &self.triples[i].subject == subj {
                let t = &self.triples[i];
                let pred = if t.predicate.as_str() == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type" {
                    "a".to_string()
                } else {
                    compact(&t.predicate)
                };
                out.push_str(if first { "\n    " } else { " ;\n    " });
                out.push_str(&pred);
                out.push(' ');
                out.push_str(&term(&t.object));
                first = false;
                i += 1;
            }
            out.push_str(" .\n\n");
        }
        out
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
