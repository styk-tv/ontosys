//! # TypeScript/JavaScript Parser
//!
//! Tree-sitter based parser for TypeScript and JavaScript source code.

use super::ast_node::*;
use crate::pipeline::PipelineError;
use std::path::Path;

/// Parser for TypeScript/JavaScript source code
pub struct TypeScriptParser {
    _marker: (),
}

impl TypeScriptParser {
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

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Collect JSDoc comments
            if trimmed.starts_with("/**") {
                let mut doc_lines = vec![];
                let mut found_end = false;

                // Check if single-line JSDoc
                if trimmed.ends_with("*/") {
                    let doc = trimmed.trim_start_matches("/**").trim_end_matches("*/").trim();
                    current_doc = Some(doc.to_string());
                    continue;
                }

                // Multi-line JSDoc
                for subsequent in lines[line_num..].iter() {
                    if subsequent.contains("*/") {
                        let clean = subsequent.trim().trim_end_matches("*/").trim_start_matches("*").trim();
                        if !clean.is_empty() {
                            doc_lines.push(clean.to_string());
                        }
                        found_end = true;
                        break;
                    } else {
                        let clean = subsequent.trim().trim_start_matches("/**").trim_start_matches("*").trim();
                        if !clean.is_empty() {
                            doc_lines.push(clean.to_string());
                        }
                    }
                }

                if found_end {
                    current_doc = Some(doc_lines.join("\n"));
                }
                continue;
            }

            // Collect decorators (TypeScript)
            if trimmed.starts_with('@') && !trimmed.contains('(') {
                current_decorators.push(trimmed.to_string());
                continue;
            }
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

            // Parse interface definitions
            if let Some(interface_node) = self.try_parse_interface(trimmed, line_num, path, &current_doc) {
                nodes.push(AstNode::Trait(interface_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse type aliases
            if let Some(type_node) = self.try_parse_type_alias(trimmed, line_num, path, &current_doc) {
                nodes.push(AstNode::TypeAlias(type_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse class definitions
            if let Some(class_node) = self.try_parse_class(trimmed, line_num, path, &current_doc, &current_decorators) {
                nodes.push(AstNode::Struct(class_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse function declarations
            if let Some(fn_node) = self.try_parse_function(trimmed, line_num, path, &current_doc, &current_decorators) {
                nodes.push(AstNode::Function(fn_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse const/let/var declarations (top-level)
            if let Some(const_node) = self.try_parse_const(trimmed, line_num, path, &current_doc) {
                nodes.push(AstNode::Const(const_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Parse enum
            if let Some(enum_node) = self.try_parse_enum(trimmed, line_num, path, &current_doc) {
                nodes.push(AstNode::Enum(enum_node));
                current_doc = None;
                current_decorators.clear();
                continue;
            }

            // Reset doc/decorators on non-comment content
            if !trimmed.is_empty() && !trimmed.starts_with("//") && !trimmed.starts_with("/*") {
                current_doc = None;
                current_decorators.clear();
            }
        }

        Ok(())
    }

    fn try_parse_import(&self, line: &str, line_num: usize, path: &Path) -> Option<UseNode> {
        if !line.starts_with("import ") {
            return None;
        }

        let rest = line.trim_start_matches("import ");

        // Handle different import styles:
        // import x from 'y'
        // import { x } from 'y'
        // import * as x from 'y'
        // import 'y'

        let (import_path, alias) = if let Some(from_idx) = rest.find(" from ") {
            let what = rest[..from_idx].trim();
            let from = rest[from_idx + 6..].trim().trim_matches(|c| c == '\'' || c == '"' || c == ';');

            let alias = if what.starts_with("* as ") {
                Some(what.trim_start_matches("* as ").to_string())
            } else if what.starts_with('{') {
                None // Named imports
            } else {
                Some(what.to_string()) // Default import
            };

            (from.to_string(), alias)
        } else {
            // Side-effect import: import 'module'
            let module = rest.trim().trim_matches(|c| c == '\'' || c == '"' || c == ';');
            (module.to_string(), None)
        };

        let is_glob = rest.contains("* as ");

        Some(UseNode {
            path: import_path,
            alias,
            is_glob,
            visibility: Visibility::Private,
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_interface(&self, line: &str, line_num: usize, path: &Path, doc: &Option<String>) -> Option<TraitNode> {
        let (visibility, rest) = self.extract_visibility(line);

        if !rest.starts_with("interface ") {
            return None;
        }

        let after_interface = rest.trim_start_matches("interface ");

        // Extract name
        let name_end = after_interface.find(|c: char| c == '<' || c == '{' || c.is_whitespace())
            .unwrap_or(after_interface.len());
        let name = after_interface[..name_end].trim().to_string();

        // Extract generics
        let generics = self.extract_generics(after_interface);

        // Extract extends
        let supertraits = if after_interface.contains(" extends ") {
            after_interface
                .split(" extends ")
                .nth(1)
                .map(|s| s.split('{').next().unwrap_or(""))
                .map(|s| {
                    s.split(',')
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
            visibility,
            generics,
            supertraits,
            items: vec![],
            is_unsafe: false,
            is_auto: false,
            doc_comment: doc.clone(),
            attributes: vec![],
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_type_alias(&self, line: &str, line_num: usize, path: &Path, doc: &Option<String>) -> Option<TypeAliasNode> {
        let (visibility, rest) = self.extract_visibility(line);

        if !rest.starts_with("type ") {
            return None;
        }

        let after_type = rest.trim_start_matches("type ");

        // Extract name
        let name_end = after_type.find(|c: char| c == '<' || c == '=' || c.is_whitespace())
            .unwrap_or(after_type.len());
        let name = after_type[..name_end].trim().to_string();

        // Extract aliased type
        let aliased_type = if let Some(eq_idx) = after_type.find('=') {
            after_type[eq_idx + 1..].trim().trim_end_matches(';').trim().to_string()
        } else {
            "unknown".to_string()
        };

        let generics = self.extract_generics(after_type);

        Some(TypeAliasNode {
            name,
            visibility,
            generics,
            aliased_type,
            doc_comment: doc.clone(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_class(
        &self,
        line: &str,
        line_num: usize,
        path: &Path,
        doc: &Option<String>,
        decorators: &[String],
    ) -> Option<StructNode> {
        let (visibility, rest) = self.extract_visibility(line);

        let (is_abstract, rest) = if rest.starts_with("abstract ") {
            (true, rest.trim_start_matches("abstract "))
        } else {
            (false, rest)
        };

        if !rest.starts_with("class ") {
            return None;
        }

        let after_class = rest.trim_start_matches("class ");

        // Extract name
        let name_end = after_class.find(|c: char| c == '<' || c == '{' || c.is_whitespace())
            .unwrap_or(after_class.len());
        let name = after_class[..name_end].trim().to_string();

        let generics = self.extract_generics(after_class);

        let mut attrs = decorators.to_vec();
        if is_abstract {
            attrs.push("abstract".to_string());
        }

        Some(StructNode {
            name,
            visibility,
            generics,
            fields: vec![],
            is_tuple: false,
            is_unit: false,
            doc_comment: doc.clone(),
            attributes: attrs,
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
    ) -> Option<FunctionNode> {
        let (visibility, rest) = self.extract_visibility(line);

        let (is_async, rest) = if rest.starts_with("async ") {
            (true, rest.trim_start_matches("async "))
        } else {
            (false, rest)
        };

        // Check for function keyword or arrow function const
        let is_function_keyword = rest.starts_with("function ");
        let is_arrow = rest.contains("=>") && (rest.starts_with("const ") || rest.starts_with("let "));

        if !is_function_keyword && !is_arrow {
            return None;
        }

        let (name, rest) = if is_function_keyword {
            let after_fn = rest.trim_start_matches("function ");
            let name_end = after_fn.find(|c: char| c == '<' || c == '(')
                .unwrap_or(after_fn.len());
            (after_fn[..name_end].trim().to_string(), after_fn)
        } else {
            // Arrow function: const name = (...) => ...
            let after_const = rest.trim_start_matches("const ").trim_start_matches("let ");
            let name_end = after_const.find(|c: char| c == '=' || c == ':')
                .unwrap_or(after_const.len());
            (after_const[..name_end].trim().to_string(), after_const)
        };

        let generics = self.extract_generics(rest);
        let params = self.extract_ts_params(rest);

        // Extract return type
        let return_type = if rest.contains("): ") {
            rest.split("): ")
                .nth(1)
                .map(|s| s.split(|c| c == '{' || c == '=')
                    .next()
                    .unwrap_or(s)
                    .trim()
                    .to_string())
        } else {
            None
        };

        Some(FunctionNode {
            name,
            visibility,
            generics,
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

    fn try_parse_const(&self, line: &str, line_num: usize, path: &Path, doc: &Option<String>) -> Option<ConstNode> {
        let (visibility, rest) = self.extract_visibility(line);

        // Only match const declarations that don't look like functions
        if !rest.starts_with("const ") {
            return None;
        }

        // Skip arrow functions
        if rest.contains("=>") || rest.contains("function") {
            return None;
        }

        let after_const = rest.trim_start_matches("const ");

        // Extract name
        let name_end = after_const.find(|c: char| c == ':' || c == '=' || c.is_whitespace())
            .unwrap_or(after_const.len());
        let name = after_const[..name_end].trim().to_string();

        // Skip destructuring
        if name.starts_with('{') || name.starts_with('[') {
            return None;
        }

        // Extract type annotation
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
            visibility,
            type_annotation,
            value: None,
            doc_comment: doc.clone(),
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn try_parse_enum(&self, line: &str, line_num: usize, path: &Path, doc: &Option<String>) -> Option<EnumNode> {
        let (visibility, rest) = self.extract_visibility(line);

        let (is_const, rest) = if rest.starts_with("const ") {
            (true, rest.trim_start_matches("const "))
        } else {
            (false, rest)
        };

        if !rest.starts_with("enum ") {
            return None;
        }

        let after_enum = rest.trim_start_matches("enum ");

        // Extract name
        let name_end = after_enum.find(|c: char| c == '{' || c.is_whitespace())
            .unwrap_or(after_enum.len());
        let name = after_enum[..name_end].trim().to_string();

        let mut attrs = vec![];
        if is_const {
            attrs.push("const".to_string());
        }

        Some(EnumNode {
            name,
            visibility,
            generics: vec![],
            variants: vec![],
            doc_comment: doc.clone(),
            attributes: attrs,
            location: Some(SourceLocation::new(path, line_num + 1, 0, line_num + 1, line.len())),
        })
    }

    fn extract_visibility<'a>(&self, line: &'a str) -> (Visibility, &'a str) {
        let trimmed = line.trim();

        if trimmed.starts_with("export default ") {
            (Visibility::Public, trimmed.trim_start_matches("export default "))
        } else if trimmed.starts_with("export ") {
            (Visibility::Public, trimmed.trim_start_matches("export "))
        } else if trimmed.starts_with("private ") {
            (Visibility::Private, trimmed.trim_start_matches("private "))
        } else if trimmed.starts_with("protected ") {
            (Visibility::Super, trimmed.trim_start_matches("protected "))
        } else if trimmed.starts_with("public ") {
            (Visibility::Public, trimmed.trim_start_matches("public "))
        } else {
            (Visibility::Private, trimmed)
        }
    }

    fn extract_generics(&self, text: &str) -> Vec<GenericParam> {
        let mut params = Vec::new();

        if let Some(start) = text.find('<') {
            // Find matching closing bracket
            let mut depth = 1;
            let mut end = start + 1;
            for (i, c) in text[start + 1..].chars().enumerate() {
                match c {
                    '<' => depth += 1,
                    '>' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + 1 + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if depth == 0 {
                let generics_str = &text[start + 1..end];

                for param in generics_str.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }

                    let name = param.split(|c: char| c == ' ' || c == '=')
                        .next()
                        .unwrap_or(param)
                        .trim();

                    // Check for extends constraint
                    let bounds: Vec<String> = if param.contains(" extends ") {
                        param.split(" extends ")
                            .nth(1)
                            .map(|s| s.split('=').next().unwrap_or(s).trim().to_string())
                            .into_iter()
                            .collect()
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

        params
    }

    fn extract_ts_params(&self, text: &str) -> Vec<ParameterNode> {
        let mut params = Vec::new();

        if let Some(start) = text.find('(') {
            // Find matching closing paren
            let mut depth = 1;
            let mut end = start + 1;
            for (i, c) in text[start + 1..].chars().enumerate() {
                match c {
                    '(' | '<' | '{' | '[' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + 1 + i;
                            break;
                        }
                    }
                    '>' | '}' | ']' => depth -= 1,
                    _ => {}
                }
            }

            if depth == 0 {
                let params_str = &text[start + 1..end];

                for param in params_str.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }

                    // Handle optional params (?)
                    let is_optional = param.contains('?');
                    let param = param.replace('?', "");

                    // Handle rest params (...)
                    let is_rest = param.starts_with("...");
                    let param = param.trim_start_matches("...");

                    // Handle default values
                    let param = param.split('=').next().unwrap_or(&param).trim();

                    // Extract name and type
                    let (name, type_annotation) = if param.contains(':') {
                        let parts: Vec<&str> = param.splitn(2, ':').collect();
                        (parts[0].trim().to_string(), Some(parts[1].trim().to_string()))
                    } else {
                        (param.to_string(), None)
                    };

                    if !name.is_empty() {
                        params.push(ParameterNode {
                            name,
                            type_annotation,
                            is_self: false,
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
    fn parse_typescript_interface() {
        let parser = TypeScriptParser::new().unwrap();
        let source = r#"
/**
 * User interface
 */
export interface User<T extends BaseEntity> {
    id: string;
    name: string;
    data: T;
}
"#;

        let nodes = parser.parse_file(Path::new("test.ts"), source).unwrap();

        let interface_node = nodes.iter().find(|n| matches!(n, AstNode::Trait(_)));
        assert!(interface_node.is_some());

        if let Some(AstNode::Trait(t)) = interface_node {
            assert_eq!(t.name, "User");
            assert!(!t.generics.is_empty());
        }
    }

    #[test]
    fn parse_typescript_class() {
        let parser = TypeScriptParser::new().unwrap();
        let source = r#"
@Component({})
export class MyComponent extends BaseComponent {
    constructor(private service: MyService) {}

    async fetchData(id: string): Promise<Data> {
        return this.service.get(id);
    }
}
"#;

        let nodes = parser.parse_file(Path::new("test.ts"), source).unwrap();

        let class_node = nodes.iter().find(|n| matches!(n, AstNode::Struct(_)));
        assert!(class_node.is_some());
    }

    #[test]
    fn parse_javascript_functions() {
        let parser = TypeScriptParser::new().unwrap();
        let source = r#"
export function add(a, b) {
    return a + b;
}

export const multiply = (a, b) => a * b;

async function fetchUser(id) {
    const response = await fetch(`/users/${id}`);
    return response.json();
}
"#;

        let nodes = parser.parse_file(Path::new("test.js"), source).unwrap();

        let fn_nodes: Vec<_> = nodes.iter().filter(|n| matches!(n, AstNode::Function(_))).collect();
        assert!(fn_nodes.len() >= 2);
    }
}
