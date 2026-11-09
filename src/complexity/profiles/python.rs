//! Python profile. Decision points: `if`, `elif`, `for`, `while`, `except`,
//! each `case` except a wildcard `case _`, conditional expressions,
//! comprehension `for`/`if`, `and`, `or`.

use super::super::engine::{Boundary, CallKind, Cog, Exit, Profile};
use super::super::Grammar;
use super::{children_with_fields, short_text, text};
use tree_sitter::Node;

pub struct PythonProfile;
pub static PYTHON: PythonProfile = PythonProfile;

fn is_wildcard_case(n: Node, src: &[u8]) -> bool {
    let mut c = n.walk();
    let found = n
        .named_children(&mut c)
        .find(|x| x.kind() == "case_pattern");
    found.is_some_and(|p| text(p, src).trim() == "_")
}

impl Profile for PythonProfile {
    fn grammar(&self) -> Grammar {
        Grammar::Python
    }

    fn decisions(&self, n: Node, src: &[u8]) -> usize {
        match n.kind() {
            "if_statement"
            | "elif_clause"
            | "for_statement"
            | "while_statement"
            | "except_clause"
            | "conditional_expression"
            | "for_in_clause"
            | "if_clause"
            | "boolean_operator" => 1,
            "case_clause" if !is_wildcard_case(n, src) => 1,
            _ => 0,
        }
    }

    fn is_loop(&self, n: Node) -> bool {
        matches!(
            n.kind(),
            "for_statement" | "while_statement" | "for_in_clause"
        )
    }

    fn loop_condition_field(&self, n: Node) -> Option<&'static str> {
        (n.kind() == "while_statement").then_some("condition")
    }

    fn loop_once_field(&self, n: Node) -> Option<&'static str> {
        matches!(n.kind(), "for_statement" | "for_in_clause").then_some("right")
    }

    fn is_conditional_child(&self, parent: Node, field: Option<&str>, _src: &[u8]) -> bool {
        match parent.kind() {
            "if_statement" => matches!(field, Some("consequence" | "alternative")),
            "elif_clause" => field == Some("consequence"),
            "for_statement" | "while_statement" => field == Some("alternative"),
            // Conservative: the condition is counted as conditional too.
            "conditional_expression" | "except_clause" | "case_clause" => true,
            "boolean_operator" => field == Some("right"),
            _ => false,
        }
    }

    fn cognitive(&self, n: Node, _parent: Option<Node>, _src: &[u8]) -> Cog {
        match n.kind() {
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "except_clause"
            | "conditional_expression"
            | "match_statement" => Cog::Nesting,
            "elif_clause" => Cog::ElseIf,
            "else_clause" => Cog::Flat,
            _ => Cog::None,
        }
    }

    fn bool_op(&self, n: Node, src: &[u8]) -> Option<String> {
        if n.kind() != "boolean_operator" {
            return None;
        }
        n.child_by_field_name("operator")
            .map(|o| text(o, src).to_string())
    }

    fn boundary(&self, n: Node) -> Boundary {
        match n.kind() {
            "function_definition" | "class_definition" => Boundary::Named,
            "lambda" => Boundary::Anonymous,
            _ => Boundary::None,
        }
    }

    fn call<'t>(
        &self,
        n: Node<'t>,
        src: &[u8],
    ) -> Option<(CallKind, String, Option<String>, Option<Node<'t>>)> {
        match n.kind() {
            "call" => {
                let f = n.child_by_field_name("function")?;
                let args = n.child_by_field_name("arguments");
                Some(match f.kind() {
                    "identifier" => (CallKind::Direct, text(f, src).to_string(), None, args),
                    "attribute" => {
                        let attr = f
                            .child_by_field_name("attribute")
                            .map(|x| text(x, src).to_string())
                            .unwrap_or_default();
                        let recv = f.child_by_field_name("object").map(|x| short_text(x, src));
                        (CallKind::Method, attr, recv, args)
                    }
                    _ => (CallKind::Indirect, short_text(f, src), None, args),
                })
            }
            "comparison_operator" => {
                let kids = children_with_fields(n);
                let at = kids.iter().position(|(k, field)| {
                    *field == Some("operators")
                        && matches!(
                            text(*k, src)
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .as_slice(),
                            ["in"] | ["not", "in"]
                        )
                })?;
                let recv = kids[at + 1..]
                    .iter()
                    .find(|(k, _)| k.is_named())
                    .map(|(k, _)| short_text(*k, src));
                Some((CallKind::Operator, "__contains__".to_string(), recv, None))
            }
            _ => None,
        }
    }

    fn is_statement(&self, n: Node) -> bool {
        n.kind().ends_with("_statement")
    }

    fn exit(&self, n: Node, _src: &[u8]) -> Exit {
        match n.kind() {
            "return_statement" => Exit::Return,
            "raise_statement" => Exit::Throw,
            "break_statement" => Exit::Break(None),
            "continue_statement" => Exit::Continue(None),
            _ => Exit::None,
        }
    }

    fn is_switch(&self, _n: Node) -> bool {
        false
    }

    fn label(&self, _n: Node, _src: &[u8]) -> Option<String> {
        None
    }

    fn comprehension(&self, n: Node) -> bool {
        matches!(
            n.kind(),
            "list_comprehension"
                | "set_comprehension"
                | "dictionary_comprehension"
                | "generator_expression"
        )
    }

    fn defers(&self, n: Node) -> bool {
        n.kind() == "yield"
    }

    /// A decorated function's span starts at its first decorator.
    fn definition_start(&self, def: Node) -> usize {
        match def.parent() {
            Some(p) if p.kind() == "decorated_definition" => p.start_byte(),
            _ => def.start_byte(),
        }
    }
}
