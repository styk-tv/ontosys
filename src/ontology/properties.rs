//! # Ontology Properties (Relationships)
//!
//! Type-safe RDF properties for code relationships.
//! These map to CodeOntology and custom predicates.

use super::{namespaces::*, Iri};

// ============================================================================
// CONTAINMENT PROPERTIES
// ============================================================================

/// Properties for containment relationships
pub mod containment {
    use super::*;

    /// Generic contains relationship
    pub fn contains() -> Iri {
        CO.iri("contains")
    }

    /// Project contains file
    pub fn has_file() -> Iri {
        CODE.iri("hasFile")
    }

    /// File/module contains function
    pub fn has_function() -> Iri {
        CO.iri("hasMethod") // CodeOntology uses hasMethod for both
    }

    /// File/module contains struct
    pub fn has_struct() -> Iri {
        CODE.iri("hasStruct")
    }

    /// File/module contains enum
    pub fn has_enum() -> Iri {
        CODE.iri("hasEnum")
    }

    /// File/module contains trait
    pub fn has_trait() -> Iri {
        CODE.iri("hasTrait")
    }

    /// Struct/enum contains field
    pub fn has_field() -> Iri {
        CO.iri("hasField")
    }

    /// Function contains parameter
    pub fn has_parameter() -> Iri {
        CO.iri("hasParameter")
    }

    /// File has import
    pub fn has_import() -> Iri {
        CODE.iri("hasImport")
    }

    /// Module contains submodule
    pub fn has_submodule() -> Iri {
        CODE.iri("hasSubmodule")
    }
}

// ============================================================================
// REFERENCE/CALL PROPERTIES
// ============================================================================

/// Properties for reference and call relationships
pub mod reference {
    use super::*;

    /// Function calls function
    pub fn calls() -> Iri {
        CODE.iri("calls")
    }

    /// Import references module/item
    pub fn imports() -> Iri {
        CODE.iri("imports")
    }

    /// Type reference (field type, parameter type, return type)
    pub fn references_type() -> Iri {
        CODE.iri("referencesType")
    }

    /// Function return type
    pub fn return_type() -> Iri {
        CODE.iri("returnType")
    }

    /// Struct implements trait
    pub fn implements() -> Iri {
        CODE.iri("implements")
    }

    /// Trait extends trait (supertraits)
    pub fn extends() -> Iri {
        RDFS.iri("subClassOf")
    }

    /// Generic type parameter
    pub fn has_type_parameter() -> Iri {
        CODE.iri("hasTypeParameter")
    }

    /// Lifetime parameter
    pub fn has_lifetime() -> Iri {
        CODE.iri("hasLifetime")
    }

    /// Where clause constraint
    pub fn has_constraint() -> Iri {
        CODE.iri("hasConstraint")
    }
}

// ============================================================================
// METADATA PROPERTIES
// ============================================================================

/// Properties for code metadata
pub mod metadata {
    use super::*;

    /// Source location (line number)
    pub fn start_line() -> Iri {
        CODE.iri("startLine")
    }

    /// End line number
    pub fn end_line() -> Iri {
        CODE.iri("endLine")
    }

    /// Column number
    pub fn start_column() -> Iri {
        CODE.iri("startColumn")
    }

    /// Documentation comment
    pub fn documentation() -> Iri {
        CODE.iri("documentation")
    }

    /// Attribute/annotation
    pub fn has_attribute() -> Iri {
        CODE.iri("hasAttribute")
    }

    /// Git commit hash
    pub fn commit_hash() -> Iri {
        CODE.iri("commitHash")
    }

    /// File path
    pub fn file_path() -> Iri {
        CODE.iri("filePath")
    }

    /// Programming language
    pub fn language() -> Iri {
        CODE.iri("language")
    }
}

// ============================================================================
// TYPE-SAFE PROPERTY BUILDERS
// ============================================================================

use super::{CodeEntity, HasIri, Term, Triple};

/// Builder for creating relationship triples with type safety
pub struct RelationshipBuilder<S, O> {
    subject: S,
    object: O,
}

impl<S: HasIri, O: HasIri> RelationshipBuilder<S, O> {
    pub fn new(subject: S, object: O) -> Self {
        Self { subject, object }
    }

    /// Create a "contains" relationship
    pub fn contains(self) -> Triple {
        Triple::new(
            self.subject.iri().clone(),
            containment::contains(),
            self.object.iri().clone(),
        )
    }

    /// Create a "calls" relationship
    pub fn calls(self) -> Triple {
        Triple::new(
            self.subject.iri().clone(),
            reference::calls(),
            self.object.iri().clone(),
        )
    }

    /// Create an "implements" relationship
    pub fn implements(self) -> Triple {
        Triple::new(
            self.subject.iri().clone(),
            reference::implements(),
            self.object.iri().clone(),
        )
    }

    /// Create a generic relationship with any predicate
    pub fn with_predicate(self, predicate: Iri) -> Triple {
        Triple::new(
            self.subject.iri().clone(),
            predicate,
            self.object.iri().clone(),
        )
    }
}

/// Extension trait for creating relationships
pub trait Relates: HasIri + Sized {
    fn relates_to<O: HasIri>(self, object: O) -> RelationshipBuilder<Self, O> {
        RelationshipBuilder::new(self, object)
    }
}

impl<T: HasIri> Relates for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::{File, Function, Struct, Trait};

    #[test]
    fn relationship_builder() {
        let file = File::new("proj", "src/lib.rs", "rust");
        let func = Function::new(&file.iri, "main");

        let triple = file.relates_to(func).contains();
        assert_eq!(triple.predicate, containment::contains());
    }

    #[test]
    fn implements_relationship() {
        let file = File::new("proj", "src/lib.rs", "rust");
        let strct = Struct::new(&file.iri, "Parser");
        let trait_ = Trait::new(&file.iri, "Parse");

        let triple = strct.relates_to(trait_).implements();
        assert_eq!(triple.predicate, reference::implements());
    }
}
