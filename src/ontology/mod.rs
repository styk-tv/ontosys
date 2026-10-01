//! # Ontology Definitions
//!
//! Type-safe representations of RDF ontologies for code artifacts.
//! Based on CodeOntology, with BFO 2020 foundational alignment.
//!
//! ## Ontology Hierarchy
//!
//! ```text
//! BFO:Continuant (things that persist)
//! └── CodeOntology:Artifact
//!     ├── co:Project
//!     ├── co:File
//!     ├── co:Module
//!     ├── co:Class / co:Struct
//!     ├── co:Trait (Rust-specific)
//!     ├── co:Function / co:Method
//!     ├── co:Field
//!     └── co:Parameter
//! ```

mod entities;
mod namespaces;
mod properties;
mod triples;

pub use entities::*;
pub use namespaces::*;
pub use properties::*;
pub use triples::*;

use std::fmt;

// ============================================================================
// IRI (Internationalized Resource Identifier)
// ============================================================================

/// A validated IRI for RDF subjects, predicates, or objects.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Iri(String);

impl Iri {
    /// Create a new IRI from a string.
    /// In production, this would validate the IRI format.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the IRI as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create an IRI by joining a namespace and local name
    pub fn from_namespace(ns: &Namespace, local: &str) -> Self {
        Self(format!("{}{}", ns.prefix(), local))
    }
}

impl fmt::Display for Iri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.0)
    }
}

impl From<&str> for Iri {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// ============================================================================
// LITERAL VALUES
// ============================================================================

/// RDF Literal with optional datatype or language tag
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// Plain string literal
    String(String),
    /// String with language tag (e.g., "hello"@en)
    LangString { value: String, lang: String },
    /// Typed literal (e.g., "42"^^xsd:integer)
    Typed { value: String, datatype: Iri },
}

impl Literal {
    pub fn string(s: impl Into<String>) -> Self {
        Literal::String(s.into())
    }

    pub fn integer(n: i64) -> Self {
        Literal::Typed {
            value: n.to_string(),
            datatype: Iri::new("http://www.w3.org/2001/XMLSchema#integer"),
        }
    }

    pub fn boolean(b: bool) -> Self {
        Literal::Typed {
            value: b.to_string(),
            datatype: Iri::new("http://www.w3.org/2001/XMLSchema#boolean"),
        }
    }

    pub fn date(date: &str) -> Self {
        Literal::Typed {
            value: date.to_string(),
            datatype: Iri::new("http://www.w3.org/2001/XMLSchema#date"),
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::String(s) => write!(f, "\"{}\"", escape_literal(s)),
            Literal::LangString { value, lang } => {
                write!(f, "\"{}\"@{}", escape_literal(value), lang)
            }
            Literal::Typed { value, datatype } => {
                write!(f, "\"{}\"^^{}", escape_literal(value), datatype)
            }
        }
    }
}

/// Escape a lexical form per the N-Triples `STRING_LITERAL_QUOTE` production.
///
/// Backslash must be escaped first; C sources routinely carry `\n`, `\\` and
/// raw control characters in string literals and comments, and an unescaped
/// one makes the whole `.nt` file unparseable.
pub fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// RDF TERM (Subject, Predicate, Object)
// ============================================================================

/// An RDF term that can be a subject, predicate, or object
#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Iri(Iri),
    Literal(Literal),
    BlankNode(String),
}

impl Term {
    pub fn iri(s: impl Into<String>) -> Self {
        Term::Iri(Iri::new(s))
    }

    pub fn literal(s: impl Into<String>) -> Self {
        Term::Literal(Literal::string(s))
    }

    pub fn blank(id: impl Into<String>) -> Self {
        Term::BlankNode(id.into())
    }
}

impl From<Iri> for Term {
    fn from(iri: Iri) -> Self {
        Term::Iri(iri)
    }
}

impl From<Literal> for Term {
    fn from(lit: Literal) -> Self {
        Term::Literal(lit)
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Iri(iri) => write!(f, "{}", iri),
            Term::Literal(lit) => write!(f, "{}", lit),
            Term::BlankNode(id) => write!(f, "_:{}", id),
        }
    }
}
