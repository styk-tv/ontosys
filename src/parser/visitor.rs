//! # AST Visitor Pattern
//!
//! Visitor trait for traversing AST nodes with type-safe callbacks.

use super::ast_node::*;

/// Visitor trait for traversing AST nodes
pub trait AstVisitor {
    /// Visit any AST node
    fn visit(&mut self, node: &AstNode) {
        match node {
            AstNode::Module(n) => self.visit_module(n),
            AstNode::Use(n) => self.visit_use(n),
            AstNode::Struct(n) => self.visit_struct(n),
            AstNode::Enum(n) => self.visit_enum(n),
            AstNode::Trait(n) => self.visit_trait(n),
            AstNode::Impl(n) => self.visit_impl(n),
            AstNode::Function(n) => self.visit_function(n),
            AstNode::Const(n) => self.visit_const(n),
            AstNode::Static(n) => self.visit_static(n),
            AstNode::TypeAlias(n) => self.visit_type_alias(n),
            AstNode::Macro(n) => self.visit_macro(n),
        }
    }

    fn visit_module(&mut self, node: &ModuleNode) {
        // Visit child items
        for item in &node.items {
            self.visit(item);
        }
    }

    fn visit_use(&mut self, _node: &UseNode) {}

    fn visit_struct(&mut self, node: &StructNode) {
        for field in &node.fields {
            self.visit_field(field);
        }
    }

    fn visit_enum(&mut self, node: &EnumNode) {
        for variant in &node.variants {
            self.visit_enum_variant(variant);
        }
    }

    fn visit_trait(&mut self, node: &TraitNode) {
        for item in &node.items {
            match item {
                TraitItem::Method(f) => self.visit_function(f),
                TraitItem::Type(t) => self.visit_associated_type(t),
                TraitItem::Const(c) => self.visit_const(c),
            }
        }
    }

    fn visit_impl(&mut self, node: &ImplNode) {
        for item in &node.items {
            match item {
                ImplItem::Method(f) => self.visit_function(f),
                ImplItem::Type(t) => self.visit_associated_type(t),
                ImplItem::Const(c) => self.visit_const(c),
            }
        }
    }

    fn visit_function(&mut self, node: &FunctionNode) {
        for param in &node.parameters {
            self.visit_parameter(param);
        }
    }

    fn visit_const(&mut self, _node: &ConstNode) {}
    fn visit_static(&mut self, _node: &StaticNode) {}
    fn visit_type_alias(&mut self, _node: &TypeAliasNode) {}
    fn visit_macro(&mut self, _node: &MacroNode) {}
    fn visit_field(&mut self, _node: &FieldNode) {}
    fn visit_enum_variant(&mut self, _node: &EnumVariant) {}
    fn visit_parameter(&mut self, _node: &ParameterNode) {}
    fn visit_associated_type(&mut self, _node: &AssociatedType) {}
}

/// Mutable visitor for transforming AST nodes
pub trait AstVisitorMut {
    fn visit_mut(&mut self, node: &mut AstNode) {
        match node {
            AstNode::Module(n) => self.visit_module_mut(n),
            AstNode::Use(n) => self.visit_use_mut(n),
            AstNode::Struct(n) => self.visit_struct_mut(n),
            AstNode::Enum(n) => self.visit_enum_mut(n),
            AstNode::Trait(n) => self.visit_trait_mut(n),
            AstNode::Impl(n) => self.visit_impl_mut(n),
            AstNode::Function(n) => self.visit_function_mut(n),
            AstNode::Const(n) => self.visit_const_mut(n),
            AstNode::Static(n) => self.visit_static_mut(n),
            AstNode::TypeAlias(n) => self.visit_type_alias_mut(n),
            AstNode::Macro(n) => self.visit_macro_mut(n),
        }
    }

    fn visit_module_mut(&mut self, node: &mut ModuleNode) {
        for item in &mut node.items {
            self.visit_mut(item);
        }
    }

    fn visit_use_mut(&mut self, _node: &mut UseNode) {}
    fn visit_struct_mut(&mut self, _node: &mut StructNode) {}
    fn visit_enum_mut(&mut self, _node: &mut EnumNode) {}
    fn visit_trait_mut(&mut self, _node: &mut TraitNode) {}
    fn visit_impl_mut(&mut self, _node: &mut ImplNode) {}
    fn visit_function_mut(&mut self, _node: &mut FunctionNode) {}
    fn visit_const_mut(&mut self, _node: &mut ConstNode) {}
    fn visit_static_mut(&mut self, _node: &mut StaticNode) {}
    fn visit_type_alias_mut(&mut self, _node: &mut TypeAliasNode) {}
    fn visit_macro_mut(&mut self, _node: &mut MacroNode) {}
}

/// A visitor that collects statistics about the AST
#[derive(Debug, Default)]
pub struct StatsVisitor {
    pub modules: usize,
    pub uses: usize,
    pub structs: usize,
    pub enums: usize,
    pub traits: usize,
    pub impls: usize,
    pub functions: usize,
    pub consts: usize,
    pub statics: usize,
    pub type_aliases: usize,
    pub macros: usize,
    pub fields: usize,
    pub parameters: usize,
}

impl AstVisitor for StatsVisitor {
    fn visit_module(&mut self, node: &ModuleNode) {
        self.modules += 1;
        for item in &node.items {
            self.visit(item);
        }
    }

    fn visit_use(&mut self, _node: &UseNode) {
        self.uses += 1;
    }

    fn visit_struct(&mut self, node: &StructNode) {
        self.structs += 1;
        for field in &node.fields {
            self.visit_field(field);
        }
    }

    fn visit_enum(&mut self, _node: &EnumNode) {
        self.enums += 1;
    }

    fn visit_trait(&mut self, _node: &TraitNode) {
        self.traits += 1;
    }

    fn visit_impl(&mut self, _node: &ImplNode) {
        self.impls += 1;
    }

    fn visit_function(&mut self, node: &FunctionNode) {
        self.functions += 1;
        for param in &node.parameters {
            self.visit_parameter(param);
        }
    }

    fn visit_const(&mut self, _node: &ConstNode) {
        self.consts += 1;
    }

    fn visit_static(&mut self, _node: &StaticNode) {
        self.statics += 1;
    }

    fn visit_type_alias(&mut self, _node: &TypeAliasNode) {
        self.type_aliases += 1;
    }

    fn visit_macro(&mut self, _node: &MacroNode) {
        self.macros += 1;
    }

    fn visit_field(&mut self, _node: &FieldNode) {
        self.fields += 1;
    }

    fn visit_parameter(&mut self, _node: &ParameterNode) {
        self.parameters += 1;
    }
}

/// A visitor that collects all function names
#[derive(Debug, Default)]
pub struct FunctionCollector {
    pub functions: Vec<String>,
}

impl AstVisitor for FunctionCollector {
    fn visit_function(&mut self, node: &FunctionNode) {
        self.functions.push(node.name.clone());
    }
}

/// A visitor that finds all imports
#[derive(Debug, Default)]
pub struct ImportCollector {
    pub imports: Vec<String>,
}

impl AstVisitor for ImportCollector {
    fn visit_use(&mut self, node: &UseNode) {
        self.imports.push(node.path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_visitor() {
        let nodes = vec![
            AstNode::Struct(StructNode {
                name: "Point".to_string(),
                visibility: Visibility::Public,
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                is_unit: false,
                doc_comment: None,
                attributes: vec![],
                location: None,
            }),
            AstNode::Function(FunctionNode {
                name: "main".to_string(),
                visibility: Visibility::Private,
                generics: vec![],
                parameters: vec![],
                return_type: None,
                is_async: false,
                is_const: false,
                is_unsafe: false,
                is_extern: false,
                abi: None,
                body_calls: vec![],
                doc_comment: None,
                attributes: vec![],
                location: None,
            }),
        ];

        let mut stats = StatsVisitor::default();
        for node in &nodes {
            stats.visit(node);
        }

        assert_eq!(stats.structs, 1);
        assert_eq!(stats.functions, 1);
    }

    #[test]
    fn function_collector() {
        let nodes = vec![
            AstNode::Function(FunctionNode {
                name: "foo".to_string(),
                visibility: Visibility::Private,
                generics: vec![],
                parameters: vec![],
                return_type: None,
                is_async: false,
                is_const: false,
                is_unsafe: false,
                is_extern: false,
                abi: None,
                body_calls: vec![],
                doc_comment: None,
                attributes: vec![],
                location: None,
            }),
            AstNode::Function(FunctionNode {
                name: "bar".to_string(),
                visibility: Visibility::Public,
                generics: vec![],
                parameters: vec![],
                return_type: None,
                is_async: true,
                is_const: false,
                is_unsafe: false,
                is_extern: false,
                abi: None,
                body_calls: vec![],
                doc_comment: None,
                attributes: vec![],
                location: None,
            }),
        ];

        let mut collector = FunctionCollector::default();
        for node in &nodes {
            collector.visit(node);
        }

        assert_eq!(collector.functions, vec!["foo", "bar"]);
    }
}
