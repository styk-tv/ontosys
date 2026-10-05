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
        let starts = statement_starts(&lines);
        let mut current_doc: Option<String> = None;
        let mut current_decorators: Vec<String> = Vec::new();
        let mut in_class: Option<(String, usize)> = None; // (name, indent)

        for (line_num, line) in lines.iter().enumerate() {
            // Lines inside a string, brackets or a backslash continuation are
            // part of an earlier statement, never a definition of their own.
            if !starts[line_num] {
                continue;
            }
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

            // Module-level assignments (constants and variables)
            if indent == 0 {
                let assigned = module_assignments(line, line_num, statement_end(&starts, line_num), path);
                if !assigned.is_empty() {
                    nodes.extend(assigned.into_iter().map(AstNode::Const));
                    current_doc = None;
                    current_decorators.clear();
                    continue;
                }
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
            if let Some(paren_end) = after_class[paren_start..].find(')').map(|i| paren_start + i) {
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

/// For each line, whether it begins a new statement: not inside a string,
/// open brackets or a backslash continuation of the line before.
fn statement_starts(lines: &[&str]) -> Vec<bool> {
    const TRIPLE_DOUBLE: &str = "\"\"\"";
    const TRIPLE_SINGLE: &str = "'''";
    let mut starts = Vec::with_capacity(lines.len());
    let mut quote: Option<&str> = None; // open string delimiter
    let mut depth: usize = 0;
    let mut continued = false;
    for line in lines {
        starts.push(quote.is_none() && depth == 0 && !continued);
        continued = false;
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if let Some(q) = quote {
                if b[i] == b'\\' {
                    if i + 1 == b.len() {
                        continued = true;
                    }
                    i += 2;
                    continue;
                }
                if b[i..].starts_with(q.as_bytes()) {
                    i += q.len();
                    quote = None;
                    continue;
                }
                i += 1;
                continue;
            }
            match b[i] {
                b'#' => break,
                b'"' | b'\'' => {
                    let triple = if b[i] == b'"' { TRIPLE_DOUBLE } else { TRIPLE_SINGLE };
                    let q = if b[i..].starts_with(triple.as_bytes()) {
                        triple
                    } else if b[i] == b'"' {
                        "\""
                    } else {
                        "'"
                    };
                    quote = Some(q);
                    i += q.len();
                    continue;
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b'\\' if i + 1 == b.len() => continued = true,
                _ => {}
            }
            i += 1;
        }
        // A single-quoted string ends with its line unless the line continues.
        if matches!(quote, Some("\"") | Some("'")) && !continued {
            quote = None;
        }
    }
    starts
}

/// Last line (1-based) of the statement that starts at `line_num` (0-based).
fn statement_end(starts: &[bool], line_num: usize) -> usize {
    let mut end = line_num;
    while end + 1 < starts.len() && !starts[end + 1] {
        end += 1;
    }
    end + 1
}

/// Python's `str.isupper()`: at least one cased character, none lowercase.
fn python_is_upper(name: &str) -> bool {
    name.chars().any(|c| c.is_uppercase()) && !name.chars().any(|c| c.is_lowercase())
}

fn is_identifier(s: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal",
        "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
    ];
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&s)
}

/// The names a module-level statement assigns: each target of `A = B = …`,
/// or `NAME: T = …`. Tuple, attribute and subscript targets, augmented
/// assignment, comparisons and bare annotations assign nothing here.
fn module_assignments(line: &str, line_num: usize, end_line: usize, path: &Path) -> Vec<ConstNode> {
    // Split the first line at top-level `=` signs (not `==`, `<=`, `+=`, `:=`, …).
    let b = line.as_bytes();
    let mut parts: Vec<&str> = Vec::new();
    let (mut start, mut depth, mut i) = (0, 0usize, 0);
    let mut quote: Option<u8> = None;
    let mut code_end = b.len();
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'#' => {
                code_end = i;
                break;
            }
            b'"' | b'\'' => quote = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => {
                if b.get(i + 1) == Some(&b'=') {
                    i += 2; // comparison
                    continue;
                }
                if i > 0 && b"=!<>:+-*/%&|^@".contains(&b[i - 1]) {
                    if parts.is_empty() {
                        return Vec::new(); // augmented assignment, walrus or comparison first
                    }
                    i += 1;
                    continue;
                }
                parts.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if parts.is_empty() {
        return Vec::new();
    }
    let value = line[start.min(code_end)..code_end].trim();
    let single = parts.len() == 1;
    parts
        .into_iter()
        .filter_map(|target| {
            let target = target.trim();
            let (name, annotation) = match target.split_once(':') {
                Some((n, t)) if single => (n.trim(), Some(t.trim().to_string())),
                Some(_) => return None,
                None => (target, None),
            };
            is_identifier(name).then(|| ConstNode {
                name: name.to_string(),
                visibility: Visibility::Public,
                type_annotation: annotation.filter(|t| !t.is_empty()),
                value: (!value.is_empty()).then(|| value.chars().take(200).collect()),
                is_variable: !python_is_upper(name),
                doc_comment: None,
                location: Some(SourceLocation::new(path, line_num + 1, 0, end_line, line.len())),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Module-level assignments, by the rule Python's own `ast` gives: direct
    /// module children only; each target of `A = B = …`; `NAME: T = …` but not a
    /// bare `NAME: T`; no tuple, attribute, subscript or augmented targets; nothing
    /// inside strings, brackets, continuations or indented blocks.
    #[test]
    fn module_assignments_follow_python_rules() {
        let source = r#""""Module doc.
FAKE_IN_DOC = 1
class FakeInDoc:
"""
import os
TAXONOMY_VERSION = "legacy-9-v1"
LEVEL2_LABELS = (
    "a",
)
CONFIG = dict(
FOO_KWARG=1,
)
A = B = 3
logger: Logger = get_logger()
bare: int
x, y = 1, 2
obj.attr = 3
items[0] = 1
COUNT += 1
EQ == 3
if True:
    NESTED = 1
s = """
IN_STRING = 2
"""
long_value = 1 + \
2
AFTER = 'x'  # comment = not a target
LAMBDA = lambda v: v == 1
def f():
    INNER = 1
"#;
        let parser = PythonParser::new().unwrap();
        let nodes = parser.parse_file(Path::new("pkg/mod.py"), source).unwrap();
        let found: Vec<(String, bool, usize)> = nodes
            .iter()
            .filter_map(|n| match n {
                AstNode::Const(c) => Some((c.name.clone(), c.is_variable, c.location.as_ref().unwrap().start_line)),
                _ => None,
            })
            .collect();
        let expect: Vec<(String, bool, usize)> = [
            ("TAXONOMY_VERSION", false, 6), ("LEVEL2_LABELS", false, 7), ("CONFIG", false, 10),
            ("A", false, 13), ("B", false, 13), ("logger", true, 14), ("s", true, 23),
            ("long_value", true, 26), ("AFTER", false, 28), ("LAMBDA", false, 29),
        ].iter().map(|(n, v, l)| (n.to_string(), *v, *l)).collect();
        assert_eq!(found, expect);
        let annotated = nodes.iter().find_map(|n| match n { AstNode::Const(c) if c.name == "logger" => c.type_annotation.clone(), _ => None });
        assert_eq!(annotated.as_deref(), Some("Logger"));
        assert!(!nodes.iter().any(|n| matches!(n, AstNode::Struct(s) if s.name == "FakeInDoc")), "nothing inside a docstring is a definition");
        assert!(nodes.iter().any(|n| matches!(n, AstNode::Function(f) if f.name == "f")), "definitions after strings are still found");
    }

    #[test]
    fn non_ascii_text_in_strings_is_scanned_by_byte_safely() {
        let source = "MSG = \"\u{2713} done\"\nDOC = \"\"\"\n\u{2713}\u{2713}\"\"\"\nNEXT = '\u{e9}'\n";
        let parser = PythonParser::new().unwrap();
        let nodes = parser.parse_file(Path::new("m.py"), source).unwrap();
        let names: Vec<&str> = nodes.iter().filter_map(|n| match n { AstNode::Const(c) => Some(c.name.as_str()), _ => None }).collect();
        assert_eq!(names, vec!["MSG", "DOC", "NEXT"]);
    }

    #[test]
    fn python_is_upper_matches_str_isupper() {
        for (name, upper) in [("TAXONOMY_VERSION", true), ("_PRIVATE", true), ("T1", true), ("__all__", false),
                              ("logger", false), ("Mixed", false), ("_1", false)] {
            assert_eq!(python_is_upper(name), upper, "{}", name);
        }
    }

    #[test]
    fn prose_line_starting_with_class_does_not_panic() {
        // A docstring line beginning with "class " whose ")" precedes its "(".
        let parser = PythonParser::new().unwrap();
        let source = "\"\"\"\nclass with the same cache). Per panel: prompts (canonical)\n\"\"\"\n";
        assert!(parser.parse_file(Path::new("test.py"), source).is_ok());
    }

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
