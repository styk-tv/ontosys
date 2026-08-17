//! # AST Node Types
//!
//! Semantic AST nodes extracted from Tree-sitter parse trees.
//! These are the intermediate representation before RDF generation.

use std::path::PathBuf;

/// Location in source code
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceLocation {
    pub fn new(file: impl Into<PathBuf>, start_line: usize, start_column: usize, end_line: usize, end_column: usize) -> Self {
        Self {
            file: file.into(),
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

/// Visibility modifier
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Private,
    Public,
    Crate,
    Super,
    Restricted(String),
}

impl Visibility {
    pub fn from_tree_sitter(node_text: Option<&str>) -> Self {
        match node_text {
            Some("pub") => Visibility::Public,
            Some(s) if s.starts_with("pub(crate)") => Visibility::Crate,
            Some(s) if s.starts_with("pub(super)") => Visibility::Super,
            Some(s) if s.starts_with("pub(in") => {
                let path = s.trim_start_matches("pub(in ").trim_end_matches(')');
                Visibility::Restricted(path.to_string())
            }
            _ => Visibility::Private,
        }
    }
}

/// A semantic AST node representing a code construct
#[derive(Debug, Clone)]
pub enum AstNode {
    /// A module declaration
    Module(ModuleNode),

    /// A use/import statement
    Use(UseNode),

    /// A struct definition
    Struct(StructNode),

    /// An enum definition
    Enum(EnumNode),

    /// A trait definition
    Trait(TraitNode),

    /// An impl block
    Impl(ImplNode),

    /// A function definition
    Function(FunctionNode),

    /// A constant definition
    Const(ConstNode),

    /// A static variable
    Static(StaticNode),

    /// A type alias
    TypeAlias(TypeAliasNode),

    /// A macro definition
    Macro(MacroNode),
}

impl AstNode {
    pub fn name(&self) -> &str {
        match self {
            AstNode::Module(n) => &n.name,
            AstNode::Use(n) => &n.path,
            AstNode::Struct(n) => &n.name,
            AstNode::Enum(n) => &n.name,
            AstNode::Trait(n) => &n.name,
            AstNode::Impl(n) => n.self_type.as_deref().unwrap_or("impl"),
            AstNode::Function(n) => &n.name,
            AstNode::Const(n) => &n.name,
            AstNode::Static(n) => &n.name,
            AstNode::TypeAlias(n) => &n.name,
            AstNode::Macro(n) => &n.name,
        }
    }

    pub fn location(&self) -> Option<&SourceLocation> {
        match self {
            AstNode::Module(n) => n.location.as_ref(),
            AstNode::Use(n) => n.location.as_ref(),
            AstNode::Struct(n) => n.location.as_ref(),
            AstNode::Enum(n) => n.location.as_ref(),
            AstNode::Trait(n) => n.location.as_ref(),
            AstNode::Impl(n) => n.location.as_ref(),
            AstNode::Function(n) => n.location.as_ref(),
            AstNode::Const(n) => n.location.as_ref(),
            AstNode::Static(n) => n.location.as_ref(),
            AstNode::TypeAlias(n) => n.location.as_ref(),
            AstNode::Macro(n) => n.location.as_ref(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            AstNode::Module(_) => "module",
            AstNode::Use(_) => "use",
            AstNode::Struct(_) => "struct",
            AstNode::Enum(_) => "enum",
            AstNode::Trait(_) => "trait",
            AstNode::Impl(_) => "impl",
            AstNode::Function(_) => "function",
            AstNode::Const(_) => "const",
            AstNode::Static(_) => "static",
            AstNode::TypeAlias(_) => "type_alias",
            AstNode::Macro(_) => "macro",
        }
    }
}

/// Module declaration
#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub name: String,
    pub visibility: Visibility,
    pub is_inline: bool,
    pub items: Vec<AstNode>,
    pub doc_comment: Option<String>,
    pub location: Option<SourceLocation>,
}

/// Use/import statement
#[derive(Debug, Clone)]
pub struct UseNode {
    pub path: String,
    pub alias: Option<String>,
    pub is_glob: bool,
    pub visibility: Visibility,
    pub location: Option<SourceLocation>,
}

/// Struct definition
#[derive(Debug, Clone)]
pub struct StructNode {
    pub name: String,
    pub visibility: Visibility,
    pub generics: Vec<GenericParam>,
    pub fields: Vec<FieldNode>,
    pub is_tuple: bool,
    pub is_unit: bool,
    pub doc_comment: Option<String>,
    pub attributes: Vec<String>,
    pub location: Option<SourceLocation>,
}

/// Enum definition
#[derive(Debug, Clone)]
pub struct EnumNode {
    pub name: String,
    pub visibility: Visibility,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<EnumVariant>,
    pub doc_comment: Option<String>,
    pub attributes: Vec<String>,
    pub location: Option<SourceLocation>,
}

/// Enum variant
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<FieldNode>,
    pub discriminant: Option<String>,
    pub doc_comment: Option<String>,
}

/// Trait definition
#[derive(Debug, Clone)]
pub struct TraitNode {
    pub name: String,
    pub visibility: Visibility,
    pub generics: Vec<GenericParam>,
    pub supertraits: Vec<String>,
    pub items: Vec<TraitItem>,
    pub is_unsafe: bool,
    pub is_auto: bool,
    pub doc_comment: Option<String>,
    pub attributes: Vec<String>,
    pub location: Option<SourceLocation>,
}

/// Item within a trait
#[derive(Debug, Clone)]
pub enum TraitItem {
    Method(FunctionNode),
    Type(AssociatedType),
    Const(ConstNode),
}

/// Associated type
#[derive(Debug, Clone)]
pub struct AssociatedType {
    pub name: String,
    pub bounds: Vec<String>,
    pub default: Option<String>,
}

/// Impl block
#[derive(Debug, Clone)]
pub struct ImplNode {
    pub self_type: Option<String>,
    pub trait_name: Option<String>,
    pub generics: Vec<GenericParam>,
    pub items: Vec<ImplItem>,
    pub is_unsafe: bool,
    pub is_negative: bool,
    pub location: Option<SourceLocation>,
}

/// Item within an impl block
#[derive(Debug, Clone)]
pub enum ImplItem {
    Method(FunctionNode),
    Type(AssociatedType),
    Const(ConstNode),
}

/// Function definition
#[derive(Debug, Clone)]
pub struct FunctionNode {
    pub name: String,
    pub visibility: Visibility,
    pub generics: Vec<GenericParam>,
    pub parameters: Vec<ParameterNode>,
    pub return_type: Option<String>,
    pub is_async: bool,
    pub is_const: bool,
    pub is_unsafe: bool,
    pub is_extern: bool,
    pub abi: Option<String>,
    pub body_calls: Vec<FunctionCall>,
    pub doc_comment: Option<String>,
    pub attributes: Vec<String>,
    pub location: Option<SourceLocation>,
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct ParameterNode {
    pub name: String,
    pub type_annotation: Option<String>,
    pub is_self: bool,
    pub is_mutable: bool,
    pub is_reference: bool,
}

/// Function call within a body
#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub callee: String,
    pub is_method: bool,
    pub location: Option<SourceLocation>,
}

/// Struct/enum field
#[derive(Debug, Clone)]
pub struct FieldNode {
    pub name: String,
    pub type_annotation: Option<String>,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub attributes: Vec<String>,
}

/// Generic parameter
#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: String,
    pub kind: GenericKind,
    pub bounds: Vec<String>,
    pub default: Option<String>,
}

/// Kind of generic parameter
#[derive(Debug, Clone)]
pub enum GenericKind {
    Type,
    Lifetime,
    Const,
}

/// Constant definition
#[derive(Debug, Clone)]
pub struct ConstNode {
    pub name: String,
    pub visibility: Visibility,
    pub type_annotation: Option<String>,
    pub value: Option<String>,
    pub doc_comment: Option<String>,
    pub location: Option<SourceLocation>,
}

/// Static variable
#[derive(Debug, Clone)]
pub struct StaticNode {
    pub name: String,
    pub visibility: Visibility,
    pub type_annotation: Option<String>,
    pub is_mutable: bool,
    pub doc_comment: Option<String>,
    pub location: Option<SourceLocation>,
}

/// Type alias
#[derive(Debug, Clone)]
pub struct TypeAliasNode {
    pub name: String,
    pub visibility: Visibility,
    pub generics: Vec<GenericParam>,
    pub aliased_type: String,
    pub doc_comment: Option<String>,
    pub location: Option<SourceLocation>,
}

/// Macro definition
#[derive(Debug, Clone)]
pub struct MacroNode {
    pub name: String,
    pub is_declarative: bool,
    pub is_proc_macro: bool,
    pub doc_comment: Option<String>,
    pub location: Option<SourceLocation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_parsing() {
        assert_eq!(Visibility::from_tree_sitter(Some("pub")), Visibility::Public);
        assert_eq!(Visibility::from_tree_sitter(Some("pub(crate)")), Visibility::Crate);
        assert_eq!(Visibility::from_tree_sitter(None), Visibility::Private);
    }

    #[test]
    fn ast_node_kind() {
        let node = AstNode::Struct(StructNode {
            name: "Test".to_string(),
            visibility: Visibility::Public,
            generics: vec![],
            fields: vec![],
            is_tuple: false,
            is_unit: false,
            doc_comment: None,
            attributes: vec![],
            location: None,
        });
        assert_eq!(node.kind(), "struct");
        assert_eq!(node.name(), "Test");
    }
}
