//! # Python Parser
//!
//! Tree-sitter based parser for Python source code.

use super::ast_node::*;
use crate::pipeline::PipelineError;
use std::path::Path;

/// Parser for Python source code
pub struct PythonParser {
    _marker: (),
}

impl PythonParser {
    pub fn new() -> Result<Self, PipelineError> {
        Ok(Self { _marker: () })
    }

    pub fn parse_file(&self, path: &Path, source: &str) -> Result<Vec<AstNode>, PipelineError> {
        let mut nodes = Vec::new();
        self.extract_items(source, path, &mut nodes)?;
        Ok(nodes)
    }

    fn extract_items(&self, source: &str, path: &Path, nodes: &mut Vec<AstNode>) -> Result<(), PipelineError> {
        let lines: Vec<&str> = source.lines().collect();
        let mut current_doc: Option<String> = None;
        let mut current_decorators: Vec<String> = Vec::new();
        let mut in_class: Option<(String, usize)> = None; // (name, indent)

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let indent = line.len() - line.trim_start().len();

            // Reset class context if we're back at module level
            if let Some((_, class_indent)) = &in_class {
                if indent <= *class_indent && !trimmed.is_empty() && !trimmed.starts_with('#') {
                    in_class = None;
                }
            }

            // Collect docstrings (triple-quoted at start)
            if trimmed.starts_with(r#"""""#) || trimmed.starts_with("'''") {
                let quote = if trimmed.starts_with(r#"""""#) { r#"""""# } else { "'''" };
                if trimmed.len() > 3 && trimmed[3..].contains(quote) {
                    // Single-line docstring
                    let doc = trimmed[3..].trim_end_matches(quote).trim();
                    current_doc = Some(doc.to_string());
                } else {
                    // Multi-line docstring - collect until closing quotes
                    let mut doc_lines = vec![trimmed[3..].to_string()];
                    for subsequent in lines[line_num + 1..].iter() {
                        if subsequent.contains(quote) {
                            let end_idx = subsequent.find(quote).unwrap();
                            doc_lines.push(subsequent[..end_idx].to_string());
                            break;
                        }
                        doc_lines.push(subsequent.to_string());
                    }
                    current_doc = Some(doc_lines.join("\n").trim().to_string());
                }
                continue;
            }

            // Collect decorators
            if trimmed.starts_with('@') {
                current_decorators.push(trimmed.to_string());
                continue;
            }

            // Parse import statements
            if let Some(import_node) = self.try_parse_import(trimmed, line_num, path) {
                nodes.push(AstNode::Use(import_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse class definitions
            if let Some(class_node) = self.try_parse_class(trimmed, line_num, path, &current_doc, &current_decorators) {
                in_class = Some((class_node.name.clone(), indent));
                nodes.push(AstNode::Struct(class_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse function/method definitions
            if let Some(fn_node) = self.try_parse_function(trimmed, line_num, path, &current_doc, &current_decorators, &in_class) {
                nodes.push(AstNode::Function(fn_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Reset doc/decorators on other content
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                current_doc = None;
                current_decorators.clear();
            }
        }

        Ok(())
    }

    fn try_parse_import(&self, line: &str, line_num: usize, path: &Path) -> Option<UseNode> {
        if line.starts_with("import ") {
            let module = line.trim_start_matches("import ").trim();

            // Handle "import x as y"
            let (path_str, alias) = if module.contains(" as ") {
                let parts: Vec<&str> = module.split(" as ").collect();
                (parts[0].trim().to_string(), Some(parts[1].trim().to_string()))
            } else {
                (module.to_string(), None)
            };

            return Some(UseNode {
                path: path_str,
                alias,
                is_glob: false,
                visibility: Visibility::Private,
                location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
            });
        }

        if line.starts_with("from ") {
            // "from x import y" or "from x import *"
            let rest = line.trim_start_matches("from ");
            if let Some(import_idx) = rest.find(" import ") {
                let module = rest[..import_idx].trim();
                let imports = rest[import_idx + 8..].trim();

                let is_glob = imports == "*";
                let full_path = if is_glob {
                    format!("{}.*", module)
                } else {
                    format!("{}.{}", module, imports.split(',').next().unwrap_or("").trim())
                };

                return Some(UseNode {
                    path: full_path,
                    alias: None,
                    is_glob,
                    visibility: Visibility::Private,
                    location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
                });
            }
        }

        None
    }

    fn try_parse_class(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        decorators: &[String],
    ) -> Option<StructNode> {
        if !line.starts_with("class ") {
            return None;
        }

        let after_class = line.trim_start_matches("class ");

        // Extract name and bases
        let name_end = after_class.find(|c: char| c == '(' || c == ':').unwrap_or(after_class.len());
        let name = after_class[..name_end].trim().to_string();

        // Check for bases (inheritance)
        let generics = if let Some(paren_start) = after_class.find('(') {
            if let Some(paren_end) = after_class.find(')') {
                after_class[paren_start + 1..paren_end]
                    .split(',')
                    .map(|b| GenericParam {
                        name: b.trim().to_string(),
                        kind: GenericKind::Type,  // Treat bases as type params for now
                        bounds: vec![],
                        default: None,
                    })
                    .collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Check for dataclass decorator
        let is_dataclass = decorators.iter().any(|d| d.contains("dataclass"));

        Some(StructNode {
            name,
            visibility: Visibility::Public,  // Python classes are public by default
            generics,
            fields: vec![],
            is_tuple: false,
            is_unit: false,
            doc_comment: doc.clone(),
            attributes: decorators.to_vec(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_function(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        decorators: &[String],
        in_class: &Option<(String, usize)>,
    ) -> Option<FunctionNode> {
        let is_async = line.starts_with("async ");
        let line = if is_async {
            line.trim_start_matches("async ")
        } else {
            line
        };

        if !line.starts_with("def ") {
            return None;
        }

        let after_def = line.trim_start_matches("def ");

        // Extract name
        let name_end = after_def.find('(').unwrap_or(after_def.len());
        let name = after_def[..name_end].trim().to_string();

        // Determine visibility
        let visibility = if name.starts_with("__") && name.ends_with("__") {
            Visibility::Public  // Dunder methods are special
        } else if name.starts_with("__") {
            Visibility::Private  // Name mangling
        } else if name.starts_with('_') {
            Visibility::Crate   // Convention for internal
        } else {
            Visibility::Public
        };

        // Extract parameters
        let params = self.extract_python_params(after_def);

        // Extract return type
        let return_type = if after_def.contains("->") {
            after_def
                .split("->")
                .nth(1)
                .map(|s| s.trim_end_matches(':').trim().to_string())
        } else {
            None
        };

        // Check decorators for special markers
        let is_static = decorators.iter().any(|d| d.contains("staticmethod"));
        let is_classmethod = decorators.iter().any(|d| d.contains("classmethod"));
        let is_property = decorators.iter().any(|d| d.contains("property"));

        Some(FunctionNode {
            name,
            visibility,
            generics: vec![],
            parameters: params,
            return_type,
            is_async,
            is_const: false,
            is_unsafe: false,
            is_extern: false,
            abi: None,
            body_calls: vec![],
            doc_comment: doc.clone(),
            attributes: decorators.to_vec(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn extract_python_params(&self, text: &str) -> Vec<ParameterNode> {
        let mut params = Vec::new();

        if let Some(start) = text.find('(') {
            if let Some(end) = text.find(')') {
                let params_str = &text[start + 1..end];

                for (idx, param) in params_str.split(',').enumerate() {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }

                    // Handle default values
                    let (param, _default) = if param.contains('=') {
                        let parts: Vec<&str> = param.splitn(2, '=').collect();
                        (parts[0].trim(), Some(parts[1].trim()))
                    } else {
                        (param, None)
                    };

                    // Handle type annotations
                    let (name, type_annotation) = if param.contains(':') {
                        let parts: Vec<&str> = param.splitn(2, ':').collect();
                        (parts[0].trim().to_string(), Some(parts[1].trim().to_string()))
                    } else {
                        (param.to_string(), None)
                    };

                    // Check for *args and **kwargs
                    let is_variadic = name.starts_with('*');

                    // Check for self/cls
                    let is_self = name == "self" || name == "cls";

                    if !name.is_empty() {
                        params.push(ParameterNode {
                            name: name.trim_start_matches('*').to_string(),
                            type_annotation,
                            is_self,
                            is_mutable: false,
                            is_reference: false,
                        });
                    }
                }
            }
        }

        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_python_class() {
        let parser = PythonParser::new().unwrap();
        let source = r#"
class MyClass(BaseClass):
    """A sample class."""

    def __init__(self, name: str) -> None:
        self.name = name

    def greet(self) -> str:
        return f"Hello, {self.name}"

    async def fetch_data(self, url: str) -> dict:
        pass
"#;

        let nodes = parser.parse_file(Path::new("test.py"), source).unwrap();

        let class_node = nodes.iter().find(|n| matches!(n, AstNode::Struct(_)));
        assert!(class_node.is_some());

        let fn_nodes: Vec<_> = nodes.iter().filter(|n| matches!(n, AstNode::Function(_))).collect();
        assert_eq!(fn_nodes.len(), 3);
    }

    #[test]
    fn parse_python_imports() {
        let parser = PythonParser::new().unwrap();
        let source = r#"
import os
import json as j
from typing import List, Dict
from pathlib import Path
from collections import *
"#;

        let nodes = parser.parse_file(Path::new("test.py"), source).unwrap();

        let imports: Vec<_> = nodes.iter().filter(|n| matches!(n, AstNode::Use(_))).collect();
        assert_eq!(imports.len(), 5);
    }
}
