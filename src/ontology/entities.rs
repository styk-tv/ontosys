//! # Code Entity Types
//!
//! Type-safe representations of code artifacts using sealed traits
//! to create closed hierarchies that match ontology classes.
//!
//! ## Pattern: Sealed Traits (Closed Hierarchies)
//!
//! ```rust,ignore
//! mod private { pub trait Sealed {} }
//!
//! // Only types in this crate can implement CodeEntity
//! pub trait CodeEntity: private::Sealed {
//!     fn entity_type() -> &'static str;
//! }
//!
//! // Explicitly seal each allowed type
//! impl private::Sealed for Function {}
//! impl private::Sealed for Struct {}
//! ```

use super::{namespaces::*, Iri, Literal, Term, Triple};
use std::marker::PhantomData;

// ============================================================================
// SEALED TRAIT PATTERN
// ============================================================================

mod private {
    pub trait Sealed {}
}

/// Trait for all code entities that can be represented in RDF.
/// This trait is sealed - only types defined in this module can implement it.
pub trait CodeEntity: private::Sealed {
    /// The ontology class IRI for this entity type
    fn class_iri() -> Iri;

    /// Human-readable type name
    fn type_name() -> &'static str;

    /// Generate the RDF type triple for this entity
    fn type_triple(&self) -> Triple
    where
        Self: HasIri,
    {
        Triple::new(self.iri().clone(), rdf::type_(), Self::class_iri())
    }
}

/// Trait for entities that have an IRI (most do)
pub trait HasIri {
    fn iri(&self) -> &Iri;
}

/// Trait for entities that have a name/label
pub trait HasName {
    fn name(&self) -> &str;
}

/// Trait for entities that can contain other entities
pub trait Container<T: CodeEntity> {
    fn contains(&self) -> &[T];
}

// ============================================================================
// ENTITY DEFINITIONS
// ============================================================================

/// A software project / repository
#[derive(Debug, Clone)]
pub struct Project {
    pub iri: Iri,
    pub name: String,
    pub description: Option<String>,
    pub repository_url: Option<String>,
}

impl private::Sealed for Project {}

impl CodeEntity for Project {
    fn class_iri() -> Iri {
        DOAP.iri("Project")
    }

    fn type_name() -> &'static str {
        "Project"
    }
}

impl HasIri for Project {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Project {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Project {
    pub fn new(id: &str, name: impl Into<String>) -> Self {
        Self {
            iri: DATA.iri(&format!("project/{}", id)),
            name: name.into(),
            description: None,
            repository_url: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_repo_url(mut self, url: impl Into<String>) -> Self {
        self.repository_url = Some(url.into());
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        let mut triples = vec![
            self.type_triple(),
            Triple::new(
                self.iri.clone(),
                rdfs::label(),
                Literal::string(&self.name),
            ),
        ];

        if let Some(desc) = &self.description {
            triples.push(Triple::new(
                self.iri.clone(),
                dc::description(),
                Literal::string(desc),
            ));
        }

        if let Some(url) = &self.repository_url {
            triples.push(Triple::new(
                self.iri.clone(),
                DOAP.iri("repository"),
                Iri::new(url),
            ));
        }

        triples
    }
}

/// A source code file
#[derive(Debug, Clone)]
pub struct File {
    pub iri: Iri,
    pub path: String,
    pub language: String,
}

impl private::Sealed for File {}

impl CodeEntity for File {
    fn class_iri() -> Iri {
        CO.iri("File")
    }

    fn type_name() -> &'static str {
        "File"
    }
}

impl HasIri for File {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for File {
    fn name(&self) -> &str {
        &self.path
    }
}

impl File {
    pub fn new(project_id: &str, path: impl Into<String>, language: impl Into<String>) -> Self {
        let path = path.into();
        let safe_path = path.replace('/', "_").replace('.', "_");
        Self {
            iri: DATA.iri(&format!("file/{}/{}", project_id, safe_path)),
            path,
            language: language.into(),
        }
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.path)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("filePath"),
                Literal::string(&self.path),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("language"),
                Literal::string(&self.language),
            ),
        ]
    }
}

/// A module or namespace
#[derive(Debug, Clone)]
pub struct Module {
    pub iri: Iri,
    pub name: String,
    pub visibility: Visibility,
}

impl private::Sealed for Module {}

impl CodeEntity for Module {
    fn class_iri() -> Iri {
        CO.iri("Module")
    }

    fn type_name() -> &'static str {
        "Module"
    }
}

impl HasIri for Module {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Module {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Module {
    pub fn new(file_iri: &Iri, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/module/{}", file_iri.as_str(), name)),
            name,
            visibility: Visibility::Private,
        }
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("visibility"),
                Literal::string(self.visibility.as_str()),
            ),
        ]
    }
}

/// A struct (Rust's primary data type)
#[derive(Debug, Clone)]
pub struct Struct {
    pub iri: Iri,
    pub name: String,
    pub visibility: Visibility,
    pub is_tuple: bool,
}

impl private::Sealed for Struct {}

impl CodeEntity for Struct {
    fn class_iri() -> Iri {
        CODE.iri("Struct")
    }

    fn type_name() -> &'static str {
        "Struct"
    }
}

impl HasIri for Struct {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Struct {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Struct {
    pub fn new(parent_iri: &Iri, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/struct/{}", parent_iri.as_str(), name)),
            name,
            visibility: Visibility::Private,
            is_tuple: false,
        }
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn tuple_struct(mut self) -> Self {
        self.is_tuple = true;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("visibility"),
                Literal::string(self.visibility.as_str()),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("isTupleStruct"),
                Literal::boolean(self.is_tuple),
            ),
        ]
    }
}

/// A Rust trait (similar to interface)
#[derive(Debug, Clone)]
pub struct Trait {
    pub iri: Iri,
    pub name: String,
    pub visibility: Visibility,
    pub is_unsafe: bool,
}

impl private::Sealed for Trait {}

impl CodeEntity for Trait {
    fn class_iri() -> Iri {
        CODE.iri("Trait")
    }

    fn type_name() -> &'static str {
        "Trait"
    }
}

impl HasIri for Trait {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Trait {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Trait {
    pub fn new(parent_iri: &Iri, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/trait/{}", parent_iri.as_str(), name)),
            name,
            visibility: Visibility::Private,
            is_unsafe: false,
        }
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn unsafe_trait(mut self) -> Self {
        self.is_unsafe = true;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("visibility"),
                Literal::string(self.visibility.as_str()),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("isUnsafe"),
                Literal::boolean(self.is_unsafe),
            ),
        ]
    }
}

/// An enum type
#[derive(Debug, Clone)]
pub struct Enum {
    pub iri: Iri,
    pub name: String,
    pub visibility: Visibility,
}

impl private::Sealed for Enum {}

impl CodeEntity for Enum {
    fn class_iri() -> Iri {
        CODE.iri("Enum")
    }

    fn type_name() -> &'static str {
        "Enum"
    }
}

impl HasIri for Enum {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Enum {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Enum {
    pub fn new(parent_iri: &Iri, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/enum/{}", parent_iri.as_str(), name)),
            name,
            visibility: Visibility::Private,
        }
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("visibility"),
                Literal::string(self.visibility.as_str()),
            ),
        ]
    }
}

/// A function or method
#[derive(Debug, Clone)]
pub struct Function {
    pub iri: Iri,
    pub name: String,
    pub visibility: Visibility,
    pub is_async: bool,
    pub is_const: bool,
    pub is_unsafe: bool,
}

impl private::Sealed for Function {}

impl CodeEntity for Function {
    fn class_iri() -> Iri {
        CO.iri("Function")
    }

    fn type_name() -> &'static str {
        "Function"
    }
}

impl HasIri for Function {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Function {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Function {
    pub fn new(parent_iri: &Iri, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/fn/{}", parent_iri.as_str(), name)),
            name,
            visibility: Visibility::Private,
            is_async: false,
            is_const: false,
            is_unsafe: false,
        }
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn async_fn(mut self) -> Self {
        self.is_async = true;
        self
    }

    pub fn const_fn(mut self) -> Self {
        self.is_const = true;
        self
    }

    pub fn unsafe_fn(mut self) -> Self {
        self.is_unsafe = true;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("visibility"),
                Literal::string(self.visibility.as_str()),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("isAsync"),
                Literal::boolean(self.is_async),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("isConst"),
                Literal::boolean(self.is_const),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("isUnsafe"),
                Literal::boolean(self.is_unsafe),
            ),
        ]
    }
}

/// A struct field or enum variant field
#[derive(Debug, Clone)]
pub struct Field {
    pub iri: Iri,
    pub name: String,
    pub type_annotation: Option<String>,
    pub visibility: Visibility,
}

impl private::Sealed for Field {}

impl CodeEntity for Field {
    fn class_iri() -> Iri {
        CO.iri("Field")
    }

    fn type_name() -> &'static str {
        "Field"
    }
}

impl HasIri for Field {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Field {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Field {
    pub fn new(parent_iri: &Iri, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/field/{}", parent_iri.as_str(), name)),
            name,
            type_annotation: None,
            visibility: Visibility::Private,
        }
    }

    pub fn with_type(mut self, ty: impl Into<String>) -> Self {
        self.type_annotation = Some(ty.into());
        self
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        let mut triples = vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("visibility"),
                Literal::string(self.visibility.as_str()),
            ),
        ];

        if let Some(ty) = &self.type_annotation {
            triples.push(Triple::new(
                self.iri.clone(),
                CODE.iri("typeAnnotation"),
                Literal::string(ty),
            ));
        }

        triples
    }
}

/// A function parameter
#[derive(Debug, Clone)]
pub struct Parameter {
    pub iri: Iri,
    pub name: String,
    pub position: usize,
    pub type_annotation: Option<String>,
    pub is_mutable: bool,
}

impl private::Sealed for Parameter {}

impl CodeEntity for Parameter {
    fn class_iri() -> Iri {
        CO.iri("Parameter")
    }

    fn type_name() -> &'static str {
        "Parameter"
    }
}

impl HasIri for Parameter {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Parameter {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Parameter {
    pub fn new(function_iri: &Iri, name: impl Into<String>, position: usize) -> Self {
        let name = name.into();
        Self {
            iri: Iri::new(format!("{}/param/{}", function_iri.as_str(), position)),
            name,
            position,
            type_annotation: None,
            is_mutable: false,
        }
    }

    pub fn with_type(mut self, ty: impl Into<String>) -> Self {
        self.type_annotation = Some(ty.into());
        self
    }

    pub fn mutable(mut self) -> Self {
        self.is_mutable = true;
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        let mut triples = vec![
            self.type_triple(),
            Triple::new(self.iri.clone(), rdfs::label(), Literal::string(&self.name)),
            Triple::new(
                self.iri.clone(),
                CODE.iri("position"),
                Literal::integer(self.position as i64),
            ),
            Triple::new(
                self.iri.clone(),
                CODE.iri("isMutable"),
                Literal::boolean(self.is_mutable),
            ),
        ];

        if let Some(ty) = &self.type_annotation {
            triples.push(Triple::new(
                self.iri.clone(),
                CODE.iri("typeAnnotation"),
                Literal::string(ty),
            ));
        }

        triples
    }
}

/// An import/use statement
#[derive(Debug, Clone)]
pub struct Import {
    pub iri: Iri,
    pub path: String,
    pub alias: Option<String>,
}

impl private::Sealed for Import {}

impl CodeEntity for Import {
    fn class_iri() -> Iri {
        CODE.iri("Import")
    }

    fn type_name() -> &'static str {
        "Import"
    }
}

impl HasIri for Import {
    fn iri(&self) -> &Iri {
        &self.iri
    }
}

impl HasName for Import {
    fn name(&self) -> &str {
        &self.path
    }
}

impl Import {
    pub fn new(file_iri: &Iri, path: impl Into<String>) -> Self {
        let path = path.into();
        let safe_path = path.replace("::", "_").replace('*', "glob");
        Self {
            iri: Iri::new(format!("{}/import/{}", file_iri.as_str(), safe_path)),
            path,
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    pub fn to_triples(&self) -> Vec<Triple> {
        let mut triples = vec![
            self.type_triple(),
            Triple::new(
                self.iri.clone(),
                CODE.iri("importPath"),
                Literal::string(&self.path),
            ),
        ];

        if let Some(alias) = &self.alias {
            triples.push(Triple::new(
                self.iri.clone(),
                CODE.iri("importAlias"),
                Literal::string(alias),
            ));
        }

        triples
    }
}

// ============================================================================
// VISIBILITY ENUM
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Private,
    Public,
    Crate,
    Super,
    Restricted(/* path would go here in full impl */),
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Public => "pub",
            Visibility::Crate => "pub(crate)",
            Visibility::Super => "pub(super)",
            Visibility::Restricted() => "pub(in path)",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pub" => Visibility::Public,
            "pub(crate)" => Visibility::Crate,
            "pub(super)" => Visibility::Super,
            _ if s.starts_with("pub(in") => Visibility::Restricted(),
            _ => Visibility::Private,
        }
    }
}

// ============================================================================
// TYPE-LEVEL ENTITY CATEGORIES (for compile-time checks)
// ============================================================================

/// Marker trait for entities that can contain other entities
pub trait ContainerEntity: CodeEntity {}
impl ContainerEntity for Project {}
impl ContainerEntity for File {}
impl ContainerEntity for Module {}
impl ContainerEntity for Struct {}
impl ContainerEntity for Enum {}
impl ContainerEntity for Trait {}
impl ContainerEntity for Function {}

/// Marker trait for entities that can be contained
pub trait ContainedEntity: CodeEntity {}
impl ContainedEntity for File {}
impl ContainedEntity for Module {}
impl ContainedEntity for Struct {}
impl ContainedEntity for Enum {}
impl ContainedEntity for Trait {}
impl ContainedEntity for Function {}
impl ContainedEntity for Field {}
impl ContainedEntity for Parameter {}
impl ContainedEntity for Import {}

/// Type-level encoding of valid containment relationships
pub trait ValidContainment<Container, Contained> {}

// Define valid containment relationships at compile time
impl ValidContainment<Project, File> for () {}
impl ValidContainment<File, Module> for () {}
impl ValidContainment<File, Function> for () {}
impl ValidContainment<File, Struct> for () {}
impl ValidContainment<File, Enum> for () {}
impl ValidContainment<File, Trait> for () {}
impl ValidContainment<File, Import> for () {}
impl ValidContainment<Module, Function> for () {}
impl ValidContainment<Module, Struct> for () {}
impl ValidContainment<Module, Enum> for () {}
impl ValidContainment<Module, Trait> for () {}
impl ValidContainment<Module, Module> for () {} // nested modules
impl ValidContainment<Struct, Field> for () {}
impl ValidContainment<Struct, Function> for () {} // impl methods
impl ValidContainment<Enum, Field> for () {} // variant fields
impl ValidContainment<Trait, Function> for () {} // trait methods
impl ValidContainment<Function, Parameter> for () {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_project() {
        let project = Project::new("test-project", "Test Project")
            .with_description("A test project");

        let triples = project.to_triples();
        assert!(triples.len() >= 2);

        // Verify type triple
        let type_triple = &triples[0];
        assert_eq!(type_triple.predicate, rdf::type_());
    }

    #[test]
    fn create_function() {
        let file = File::new("proj", "src/lib.rs", "rust");
        let func = Function::new(&file.iri, "parse_input")
            .with_visibility(Visibility::Public)
            .async_fn();

        assert!(func.is_async);
        assert_eq!(func.visibility, Visibility::Public);
    }

    #[test]
    fn visibility_roundtrip() {
        assert_eq!(Visibility::from_str("pub"), Visibility::Public);
        assert_eq!(Visibility::from_str("pub(crate)"), Visibility::Crate);
        assert_eq!(Visibility::from_str(""), Visibility::Private);
    }
}
