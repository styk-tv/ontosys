//! # Tree-sitter Rust Parser
//!
//! Parses Rust source files using tree-sitter-rust and extracts
//! semantic AST nodes for knowledge graph construction.

use super::ast_node::*;
use crate::pipeline::PipelineError;
use std::path::Path;

/// Parser for Rust source code using tree-sitter
pub struct RustParser {
    // In a real implementation, this would hold the tree-sitter parser
    // For now, we use a simplified regex-based approach for demonstration
    _marker: (),
}

impl RustParser {
    /// Create a new Rust parser
    pub fn new() -> Result<Self, PipelineError> {
        // In production, initialize tree-sitter:
        // let mut parser = tree_sitter::Parser::new();
        // parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
        Ok(Self { _marker: () })
    }

    /// Parse a Rust source file
    pub fn parse_file(&self, path: &Path, source: &str) -> Result<Vec<AstNode>, PipelineError> {
        let mut nodes = Vec::new();

        // For demonstration, use a simplified parser
        // In production, this would use tree-sitter:
        //
        // let tree = self.parser.parse(source, None)
        //     .ok_or_else(|| PipelineError::ParseError("Failed to parse".into()))?;
        // let root = tree.root_node();
        // self.visit_node(root, source, path, &mut nodes);

        // Simplified extraction using regex patterns
        self.extract_items(source, path, &mut nodes)?;

        Ok(nodes)
    }

    /// Extract items using pattern matching (simplified tree-sitter alternative)
    fn extract_items(&self, source: &str, path: &Path, nodes: &mut Vec<AstNode>) -> Result<(), PipelineError> {
        let lines: Vec<&str> = source.lines().collect();
        let mut current_doc: Option<String> = None;
        let mut current_attrs: Vec<String> = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Collect doc comments
            if trimmed.starts_with("///") {
                let doc = trimmed.trim_start_matches("///").trim();
                match &mut current_doc {
                    Some(existing) => {
                        existing.push('\n');
                        existing.push_str(doc);
                    }
                    None => current_doc = Some(doc.to_string()),
                }
                continue;
            }

            // Collect attributes
            if trimmed.starts_with("#[") {
                current_attrs.push(trimmed.to_string());
                continue;
            }

            // Parse struct
            if let Some(struct_node) = self.try_parse_struct(trimmed, line_num, path, &current_doc, &current_attrs) {
                nodes.push(AstNode::Struct(struct_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse enum
            if let Some(enum_node) = self.try_parse_enum(trimmed, line_num, path, &current_doc, &current_attrs) {
                nodes.push(AstNode::Enum(enum_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse trait
            if let Some(trait_node) = self.try_parse_trait(trimmed, line_num, path, &current_doc, &current_attrs) {
                nodes.push(AstNode::Trait(trait_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse function
            if let Some(fn_node) = self.try_parse_function(trimmed, line_num, path, &current_doc, &current_attrs) {
                nodes.push(AstNode::Function(fn_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse use statement
            if let Some(use_node) = self.try_parse_use(trimmed, line_num, path) {
                nodes.push(AstNode::Use(use_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse mod declaration
            if let Some(mod_node) = self.try_parse_mod(trimmed, line_num, path, &current_doc) {
                nodes.push(AstNode::Module(mod_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse const
            if let Some(const_node) = self.try_parse_const(trimmed, line_num, path, &current_doc) {
                nodes.push(AstNode::Const(const_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Parse impl block (simplified)
            if let Some(impl_node) = self.try_parse_impl(trimmed, line_num, path) {
                nodes.push(AstNode::Impl(impl_node));
                current_doc = None;
                current_attrs.clear();
                continue;
            }

            // Reset doc/attrs if we hit a non-item line
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                current_doc = None;
                current_attrs.clear();
            }
        }

        Ok(())
    }

    fn try_parse_struct(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        attrs: &[String],
    ) -> Option<StructNode> {
        let (vis, rest) = self.extract_visibility(line);

        if !rest.starts_with("struct ") {
            return None;
        }

        let after_struct = rest.trim_start_matches("struct ");
        let name = after_struct
            .split(|c: char| c == '<' || c == '(' || c == '{' || c == ';' || c.is_whitespace())
            .next()?
            .to_string();

        let is_tuple = after_struct.contains('(') && !after_struct.contains('{');
        let is_unit = after_struct.trim_end().ends_with(';') && !is_tuple;

        // Extract generics (simplified)
        let generics = self.extract_generics(after_struct);

        Some(StructNode {
            name,
            visibility: vis,
            generics,
            fields: vec![], // Would need multi-line parsing
            is_tuple,
            is_unit,
            doc_comment: doc.clone(),
            attributes: attrs.to_vec(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_enum(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        attrs: &[String],
    ) -> Option<EnumNode> {
        let (vis, rest) = self.extract_visibility(line);

        if !rest.starts_with("enum ") {
            return None;
        }

        let after_enum = rest.trim_start_matches("enum ");
        let name = after_enum
            .split(|c: char| c == '<' || c == '{' || c.is_whitespace())
            .next()?
            .to_string();

        let generics = self.extract_generics(after_enum);

        Some(EnumNode {
            name,
            visibility: vis,
            generics,
            variants: vec![], // Would need multi-line parsing
            doc_comment: doc.clone(),
            attributes: attrs.to_vec(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_trait(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        attrs: &[String],
    ) -> Option<TraitNode> {
        let (vis, rest) = self.extract_visibility(line);

        let (is_unsafe, rest) = if rest.starts_with("unsafe ") {
            (true, rest.trim_start_matches("unsafe "))
        } else {
            (false, rest)
        };

        if !rest.starts_with("trait ") {
            return None;
        }

        let after_trait = rest.trim_start_matches("trait ");
        let name = after_trait
            .split(|c: char| c == '<' || c == ':' || c == '{' || c.is_whitespace())
            .next()?
            .to_string();

        let generics = self.extract_generics(after_trait);

        // Extract supertraits (simplified)
        let supertraits = if after_trait.contains(':') {
            after_trait
                .split(':')
                .nth(1)
                .map(|s| s.split('{').next().unwrap_or(""))
                .map(|s| {
                    s.split('+')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };

        Some(TraitNode {
            name,
            visibility: vis,
            generics,
            supertraits,
            items: vec![],
            is_unsafe,
            is_auto: false,
            doc_comment: doc.clone(),
            attributes: attrs.to_vec(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_function(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        attrs: &[String],
    ) -> Option<FunctionNode> {
        let (vis, rest) = self.extract_visibility(line);

        let (is_async, rest) = if rest.starts_with("async ") {
            (true, rest.trim_start_matches("async "))
        } else {
            (false, rest)
        };

        let (is_const, rest) = if rest.starts_with("const ") && rest.contains("fn ") {
            (true, rest.trim_start_matches("const "))
        } else {
            (false, rest)
        };

        let (is_unsafe, rest) = if rest.starts_with("unsafe ") {
            (true, rest.trim_start_matches("unsafe "))
        } else {
            (false, rest)
        };

        let (is_extern, abi, rest) = if rest.starts_with("extern ") {
            let after_extern = rest.trim_start_matches("extern ");
            if after_extern.starts_with('"') {
                let end = after_extern[1..].find('"').unwrap_or(0) + 1;
                (true, Some(after_extern[1..end].to_string()), &after_extern[end + 1..])
            } else {
                (true, Some("C".to_string()), after_extern)
            }
        } else {
            (false, None, rest)
        };

        if !rest.trim().starts_with("fn ") {
            return None;
        }

        let after_fn = rest.trim().trim_start_matches("fn ");
        let name = after_fn
            .split(|c: char| c == '<' || c == '(' || c.is_whitespace())
            .next()?
            .to_string();

        let generics = self.extract_generics(after_fn);

        // Extract parameters (simplified)
        let params = self.extract_parameters(after_fn);

        // Extract return type (simplified)
        let return_type = if after_fn.contains("->") {
            after_fn
                .split("->")
                .nth(1)
                .map(|s| s.split('{').next().unwrap_or(s).trim().to_string())
        } else {
            None
        };

        Some(FunctionNode {
            name,
            visibility: vis,
            generics,
            parameters: params,
            return_type,
            is_async,
            is_const,
            is_unsafe,
            is_extern,
            abi,
            body_calls: vec![], // Would need body parsing
            doc_comment: doc.clone(),
            attributes: attrs.to_vec(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_use(&self, line: &str, line_num: usize, path: &Path) -> Option<UseNode> {
        let (vis, rest) = self.extract_visibility(line);

        if !rest.starts_with("use ") {
            return None;
        }

        let after_use = rest.trim_start_matches("use ");
        let path_end = after_use.find(';').unwrap_or(after_use.len());
        let use_path = after_use[..path_end].trim();

        let is_glob = use_path.ends_with("::*");

        // Check for alias
        let (final_path, alias) = if use_path.contains(" as ") {
            let parts: Vec<&str> = use_path.split(" as ").collect();
            (parts[0].trim().to_string(), Some(parts[1].trim().to_string()))
        } else {
            (use_path.to_string(), None)
        };

        Some(UseNode {
            path: final_path,
            alias,
            is_glob,
            visibility: vis,
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_mod(&self, line: &str, line_num: usize, path: &Path, doc: &Option<String>) -> Option<ModuleNode> {
        let (vis, rest) = self.extract_visibility(line);

        if !rest.starts_with("mod ") {
            return None;
        }

        let after_mod = rest.trim_start_matches("mod ");
        let name = after_mod
            .split(|c: char| c == ';' || c == '{' || c.is_whitespace())
            .next()?
            .to_string();

        let is_inline = after_mod.contains('{');

        Some(ModuleNode {
            name,
            visibility: vis,
            is_inline,
            items: vec![],
            doc_comment: doc.clone(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_const(&self, line: &str, line_num: usize, path: &Path, doc: &Option<String>) -> Option<ConstNode> {
        let (vis, rest) = self.extract_visibility(line);

        if !rest.starts_with("const ") {
            return None;
        }

        // Skip "const fn" which is a function
        if rest.contains("const fn ") {
            return None;
        }

        let after_const = rest.trim_start_matches("const ");
        let name = after_const
            .split(|c: char| c == ':' || c.is_whitespace())
            .next()?
            .to_string();

        let type_annotation = if after_const.contains(':') {
            after_const
                .split(':')
                .nth(1)
                .and_then(|s| s.split('=').next())
                .map(|s| s.trim().to_string())
        } else {
            None
        };

        Some(ConstNode {
            name,
            visibility: vis,
            type_annotation,
            value: None,
            is_variable: false,
            doc_comment: doc.clone(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_impl(&self, line: &str, line_num: usize, path: &Path) -> Option<ImplNode> {
        let rest = line.trim();

        let (is_unsafe, rest) = if rest.starts_with("unsafe ") {
            (true, rest.trim_start_matches("unsafe "))
        } else {
            (false, rest)
        };

        if !rest.starts_with("impl") {
            return None;
        }

        let after_impl = rest.trim_start_matches("impl").trim();

        // Check for trait impl: impl Trait for Type
        let (trait_name, self_type) = if after_impl.contains(" for ") {
            let parts: Vec<&str> = after_impl.split(" for ").collect();
            let trait_part = parts[0].split('<').next().unwrap_or(parts[0]).trim();
            let type_part = parts.get(1)
                .map(|s| s.split('{').next().unwrap_or(s).split('<').next().unwrap_or(s).trim());
            (Some(trait_part.to_string()), type_part.map(|s| s.to_string()))
        } else {
            let type_part = after_impl.split('{').next().unwrap_or(after_impl)
                .split('<').next().unwrap_or(after_impl).trim();
            (None, Some(type_part.to_string()))
        };

        let generics = self.extract_generics(after_impl);

        Some(ImplNode {
            self_type,
            trait_name,
            generics,
            items: vec![],
            is_unsafe,
            is_negative: after_impl.contains("!"),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn extract_visibility<'a>(&self, line: &'a str) -> (Visibility, &'a str) {
        let trimmed = line.trim();

        if trimmed.starts_with("pub(crate)") {
            (Visibility::Crate, trimmed.trim_start_matches("pub(crate)").trim())
        } else if trimmed.starts_with("pub(super)") {
            (Visibility::Super, trimmed.trim_start_matches("pub(super)").trim())
        } else if trimmed.starts_with("pub(in ") {
            if let Some(end) = trimmed.find(')') {
                let path = &trimmed[7..end];
                (Visibility::Restricted(path.to_string()), trimmed[end + 1..].trim())
            } else {
                (Visibility::Private, trimmed)
            }
        } else if trimmed.starts_with("pub ") {
            (Visibility::Public, trimmed.trim_start_matches("pub ").trim())
        } else {
            (Visibility::Private, trimmed)
        }
    }

    fn extract_generics(&self, text: &str) -> Vec<GenericParam> {
        let mut params = Vec::new();

        if let Some(start) = text.find('<') {
            if let Some(end) = text.rfind('>') {
                let generics_str = &text[start + 1..end];

                // Simple split by comma (doesn't handle nested generics well)
                for param in generics_str.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }

                    // Check for lifetime
                    if param.starts_with('\'') {
                        let name = param.split(':').next().unwrap_or(param).trim();
                        params.push(GenericParam {
                            name: name.to_string(),
                            kind: GenericKind::Lifetime,
                            bounds: vec![],
                            default: None,
                        });
                    }
                    // Check for const generic
                    else if param.starts_with("const ") {
                        let name = param.trim_start_matches("const ")
                            .split(':').next().unwrap_or("").trim();
                        params.push(GenericParam {
                            name: name.to_string(),
                            kind: GenericKind::Const,
                            bounds: vec![],
                            default: None,
                        });
                    }
                    // Type parameter
                    else {
                        let name = param.split(':').next().unwrap_or(param).trim();
                        let bounds: Vec<String> = if param.contains(':') {
                            param.split(':').nth(1)
                                .map(|s| s.split('+').map(|b| b.trim().to_string()).collect())
                                .unwrap_or_default()
                        } else {
                            vec![]
                        };

                        params.push(GenericParam {
                            name: name.to_string(),
                            kind: GenericKind::Type,
                            bounds,
                            default: None,
                        });
                    }
                }
            }
        }

        params
    }

    fn extract_parameters(&self, text: &str) -> Vec<ParameterNode> {
        let mut params = Vec::new();

        if let Some(start) = text.find('(') {
            if let Some(end) = text.find(')') {
                let params_str = &text[start + 1..end];

                for (idx, param) in params_str.split(',').enumerate() {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }

                    // Check for self parameter
                    if param == "self" || param == "&self" || param == "&mut self" || param == "mut self" {
                        params.push(ParameterNode {
                            name: "self".to_string(),
                            type_annotation: None,
                            is_self: true,
                            is_mutable: param.contains("mut"),
                            is_reference: param.starts_with('&'),
                        });
                        continue;
                    }

                    // Regular parameter
                    let (is_mutable, param) = if param.starts_with("mut ") {
                        (true, param.trim_start_matches("mut "))
                    } else {
                        (false, param)
                    };

                    let parts: Vec<&str> = param.splitn(2, ':').collect();
                    let name = parts[0].trim().to_string();
                    let type_annotation = parts.get(1).map(|s| s.trim().to_string());

                    params.push(ParameterNode {
                        name,
                        type_annotation,
                        is_self: false,
                        is_mutable,
                        is_reference: false,
                    });
                }
            }
        }

        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_simple_struct() {
        let parser = RustParser::new().unwrap();
        let source = r#"
/// A point in 2D space
#[derive(Debug, Clone)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
"#;

        let nodes = parser.parse_file(Path::new("test.rs"), source).unwrap();

        let struct_node = nodes.iter().find(|n| matches!(n, AstNode::Struct(_)));
        assert!(struct_node.is_some());

        if let Some(AstNode::Struct(s)) = struct_node {
            assert_eq!(s.name, "Point");
            assert!(matches!(s.visibility, Visibility::Public));
            assert!(s.doc_comment.is_some());
        }
    }

    #[test]
    fn parse_function() {
        let parser = RustParser::new().unwrap();
        let source = r#"
pub async fn fetch_data(url: &str) -> Result<String, Error> {
    // implementation
}
"#;

        let nodes = parser.parse_file(Path::new("test.rs"), source).unwrap();

        let fn_node = nodes.iter().find(|n| matches!(n, AstNode::Function(_)));
        assert!(fn_node.is_some());

        if let Some(AstNode::Function(f)) = fn_node {
            assert_eq!(f.name, "fetch_data");
            assert!(f.is_async);
            assert!(matches!(f.visibility, Visibility::Public));
            assert!(f.return_type.is_some());
        }
    }

    #[test]
    fn parse_trait() {
        let parser = RustParser::new().unwrap();
        let source = r#"
pub trait Parser: Clone + Send {
    fn parse(&self, input: &str) -> Result<AST, Error>;
}
"#;

        let nodes = parser.parse_file(Path::new("test.rs"), source).unwrap();

        let trait_node = nodes.iter().find(|n| matches!(n, AstNode::Trait(_)));
        assert!(trait_node.is_some());

        if let Some(AstNode::Trait(t)) = trait_node {
            assert_eq!(t.name, "Parser");
            assert!(!t.supertraits.is_empty());
        }
    }

    #[test]
    fn parse_impl() {
        let parser = RustParser::new().unwrap();
        let source = r#"
impl Parser for JsonParser {
    fn parse(&self, input: &str) -> Result<AST, Error> {
        todo!()
    }
}
"#;

        let nodes = parser.parse_file(Path::new("test.rs"), source).unwrap();

        let impl_node = nodes.iter().find(|n| matches!(n, AstNode::Impl(_)));
        assert!(impl_node.is_some());

        if let Some(AstNode::Impl(i)) = impl_node {
            assert_eq!(i.trait_name, Some("Parser".to_string()));
            assert_eq!(i.self_type, Some("JsonParser".to_string()));
        }
    }
}
