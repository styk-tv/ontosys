//! # Namespace Definitions
//!
//! Standard RDF namespaces and custom namespaces for code ontology.

use super::Iri;

/// An RDF namespace with a prefix
#[derive(Debug, Clone)]
pub struct Namespace {
    prefix: &'static str,
    short: &'static str,
}

impl Namespace {
    pub const fn new(prefix: &'static str, short: &'static str) -> Self {
        Self { prefix, short }
    }

    pub fn prefix(&self) -> &str {
        self.prefix
    }

    pub fn short(&self) -> &str {
        self.short
    }

    /// Create an IRI in this namespace
    pub fn iri(&self, local: &str) -> Iri {
        Iri::from_namespace(self, local)
    }
}

// ============================================================================
// STANDARD NAMESPACES
// ============================================================================

/// RDF Core namespace
pub const RDF: Namespace = Namespace::new(
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
    "rdf",
);

/// RDF Schema namespace
pub const RDFS: Namespace = Namespace::new(
    "http://www.w3.org/2000/01/rdf-schema#",
    "rdfs",
);

/// OWL namespace
pub const OWL: Namespace = Namespace::new(
    "http://www.w3.org/2002/07/owl#",
    "owl",
);

/// XML Schema datatypes
pub const XSD: Namespace = Namespace::new(
    "http://www.w3.org/2001/XMLSchema#",
    "xsd",
);

/// Dublin Core elements
pub const DC: Namespace = Namespace::new(
    "http://purl.org/dc/elements/1.1/",
    "dc",
);

/// Dublin Core terms
pub const DCT: Namespace = Namespace::new(
    "http://purl.org/dc/terms/",
    "dct",
);

/// PROV-O Provenance
pub const PROV: Namespace = Namespace::new(
    "http://www.w3.org/ns/prov#",
    "prov",
);

/// DOAP (Description of a Project)
pub const DOAP: Namespace = Namespace::new(
    "http://usefulinc.com/ns/doap#",
    "doap",
);

// ============================================================================
// CODE ONTOLOGY NAMESPACES
// ============================================================================

/// CodeOntology namespace (primary domain ontology)
pub const CO: Namespace = Namespace::new(
    "http://codeontology.org/ontology/",
    "co",
);

/// BFO 2020 (Basic Formal Ontology)
pub const BFO: Namespace = Namespace::new(
    "http://purl.obolibrary.org/obo/BFO_",
    "bfo",
);

/// Custom code namespace for Rust-specific extensions
pub const CODE: Namespace = Namespace::new(
    "http://example.org/code/",
    "code",
);

/// Instance data namespace (for actual code entities)
pub const DATA: Namespace = Namespace::new(
    "http://example.org/data/",
    "data",
);

/// C language constructs extracted from a tree-sitter syntax tree
pub const CX: Namespace = Namespace::new(
    "https://ontosys.io/ns/c#",
    "cx",
);

/// PostgreSQL domain ontology (subsystems, catalogs, settings, error states, ...)
pub const PG: Namespace = Namespace::new(
    "https://ontosys.io/ns/pg#",
    "pg",
);

/// PostgreSQL catalog columns as properties of bootstrap catalog rows
pub const PGCAT: Namespace = Namespace::new(
    "https://ontosys.io/ns/pgcat#",
    "pgcat",
);

/// Base for version-independent instance IRIs.
///
/// Instance IRIs are built from the project name and a repository-relative
/// path or natural key only — never from the checkout directory, a counter, or
/// processing order — so two builds of the same project (e.g. two release tags
/// in two worktrees) name the same entity identically and can be diffed.
pub fn instance_iri(project: &str, kind: &str, key: &str) -> Iri {
    Iri::new(format!(
        "https://ontosys.io/id/{}/{}/{}",
        iri_segment(project),
        kind,
        iri_segment(key)
    ))
}

/// Percent-encode everything outside a conservative IRI-safe set.
///
/// `/` is kept so repository paths stay readable; `%` is always encoded so the
/// mapping is reversible. Operator names such as `<>` or `|/` would otherwise
/// produce an invalid N-Triples IRIREF.
pub fn iri_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':' | b'(' | b')' | b',' | b'@' | b'!' | b'$' | b'*' | b'+' | b';' | b'=');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Every namespace the serializers know a prefix for.
pub const ALL_PREFIXES: &[&Namespace] = &[&RDF, &RDFS, &OWL, &XSD, &DC, &DCT, &PROV, &DOAP, &CO, &CODE, &DATA, &CX, &PG, &PGCAT];

// ============================================================================
// COMMON IRIs
// ============================================================================

pub mod rdf {
    use super::*;

    pub fn type_() -> Iri {
        RDF.iri("type")
    }

    pub fn property() -> Iri {
        RDF.iri("Property")
    }
}

pub mod rdfs {
    use super::*;

    pub fn label() -> Iri {
        RDFS.iri("label")
    }

    pub fn comment() -> Iri {
        RDFS.iri("comment")
    }

    pub fn subclass_of() -> Iri {
        RDFS.iri("subClassOf")
    }

    pub fn domain() -> Iri {
        RDFS.iri("domain")
    }

    pub fn range() -> Iri {
        RDFS.iri("range")
    }
}

pub mod owl {
    use super::*;

    pub fn class() -> Iri {
        OWL.iri("Class")
    }

    pub fn object_property() -> Iri {
        OWL.iri("ObjectProperty")
    }

    pub fn datatype_property() -> Iri {
        OWL.iri("DatatypeProperty")
    }
}

pub mod dc {
    use super::*;

    pub fn title() -> Iri {
        DC.iri("title")
    }

    pub fn description() -> Iri {
        DC.iri("description")
    }

    pub fn creator() -> Iri {
        DC.iri("creator")
    }

    pub fn created() -> Iri {
        DCT.iri("created")
    }

    pub fn modified() -> Iri {
        DCT.iri("modified")
    }
}

pub mod prov {
    use super::*;

    pub fn entity() -> Iri {
        PROV.iri("Entity")
    }

    pub fn was_derived_from() -> Iri {
        PROV.iri("wasDerivedFrom")
    }

    pub fn was_generated_by() -> Iri {
        PROV.iri("wasGeneratedBy")
    }

    pub fn was_attributed_to() -> Iri {
        PROV.iri("wasAttributedTo")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_iri_generation() {
        let class_iri = CO.iri("Class");
        assert_eq!(class_iri.as_str(), "http://codeontology.org/ontology/Class");
    }

    #[test]
    fn common_iris() {
        assert_eq!(
            rdf::type_().as_str(),
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
        );
        assert_eq!(
            rdfs::label().as_str(),
            "http://www.w3.org/2000/01/rdf-schema#label"
        );
    }
}
