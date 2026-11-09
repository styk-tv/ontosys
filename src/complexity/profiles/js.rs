//! JavaScript, TypeScript and TSX profile (one profile, three grammars).
//! Decision points: `if`, `for`, `for…in/of`, `while`, `do`, each `case`
//! with a value, `catch`, `?:`, `&&`, `||`, `??`.

use super::super::engine::{Boundary, CallKind, Cog, Exit, Profile};
use super::super::Grammar;
use super::{short_text, text};
use tree_sitter::Node;

pub struct JsProfile {
    grammar: Grammar,
}
pub static JAVASCRIPT: JsProfile = JsProfile {
    grammar: Grammar::JavaScript,
};
pub static TYPESCRIPT: JsProfile = JsProfile {
    grammar: Grammar::TypeScript,
};
pub static TSX: JsProfile = JsProfile {
    grammar: Grammar::Tsx,
};

fn logical_op<'a>(n: Node, src: &'a [u8]) -> Option<&'a str> {
    if n.kind() != "binary_expression" {
        return None;
    }
    let op = text(n.child_by_field_name("operator")?, src);
    matches!(op, "&&" | "||" | "??").then_some(op)
}

impl Profile for JsProfile {
    fn grammar(&self) -> Grammar {
        self.grammar
    }

    fn decisions(&self, n: Node, src: &[u8]) -> usize {
        match n.kind() {
            "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
            | "do_statement" | "catch_clause" | "ternary_expression" => 1,
            "switch_case" if n.child_by_field_name("value").is_some() => 1,
            "binary_expression" if logical_op(n, src).is_some() => 1,
            // `a ||= b`, `a &&= b`, `a ??= b` short-circuit like their operators
            "augmented_assignment_expression"
                if n.child_by_field_name("operator").is_some_and(|o| matches!(text(o, src), "||=" | "&&=" | "??=")) =>
            {
                1
            }
            _ => 0,
        }
    }

    fn is_loop(&self, n: Node) -> bool {
        matches!(
            n.kind(),
            "for_statement" | "for_in_statement" | "while_statement" | "do_statement"
        )
    }

    fn loop_condition_field(&self, n: Node) -> Option<&'static str> {
        matches!(
            n.kind(),
            "for_statement" | "while_statement" | "do_statement"
        )
        .then_some("condition")
    }

    fn loop_once_field(&self, n: Node) -> Option<&'static str> {
        match n.kind() {
            "for_statement" => Some("initializer"),
            "for_in_statement" => Some("right"),
            _ => None,
        }
    }

    fn is_conditional_child(&self, parent: Node, field: Option<&str>, src: &[u8]) -> bool {
        match parent.kind() {
            "if_statement" | "ternary_expression" => {
                matches!(field, Some("consequence" | "alternative"))
            }
            "switch_case" | "switch_default" => field != Some("value"),
            "catch_clause" => field == Some("body"),
            "binary_expression" => field == Some("right") && logical_op(parent, src).is_some(),
            _ => false,
        }
    }

    fn cognitive(&self, n: Node, parent: Option<Node>, _src: &[u8]) -> Cog {
        match n.kind() {
            "if_statement" if parent.is_some_and(|p| p.kind() == "else_clause") => Cog::ElseIf,
            "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
            | "do_statement" | "switch_statement" | "catch_clause" | "ternary_expression" => {
                Cog::Nesting
            }
            "else_clause" => {
                let mut c = n.walk();
                let kids: Vec<Node> = n.named_children(&mut c).collect();
                if kids.len() == 1 && kids[0].kind() == "if_statement" {
                    Cog::None
                } else {
                    Cog::Flat
                }
            }
            "break_statement" | "continue_statement"
                if n.child_by_field_name("label").is_some() =>
            {
                Cog::FlatLeaf
            }
            _ => Cog::None,
        }
    }

    fn bool_op(&self, n: Node, src: &[u8]) -> Option<String> {
        logical_op(n, src).map(str::to_string)
    }

    fn boundary(&self, n: Node) -> Boundary {
        match n.kind() {
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "class_declaration"
            | "class"
            | "abstract_class_declaration" => Boundary::Named,
            "arrow_function" | "function_expression" | "function" | "generator_function" => {
                Boundary::Anonymous
            }
            _ => Boundary::None,
        }
    }

    fn call<'t>(
        &self,
        n: Node<'t>,
        src: &[u8],
    ) -> Option<(CallKind, String, Option<String>, Option<Node<'t>>)> {
        match n.kind() {
            "call_expression" => {
                let f = n.child_by_field_name("function")?;
                let args = n.child_by_field_name("arguments");
                Some(match f.kind() {
                    "identifier" => (CallKind::Direct, text(f, src).to_string(), None, args),
                    "member_expression" => {
                        let prop = f
                            .child_by_field_name("property")
                            .map(|x| text(x, src).to_string())
                            .unwrap_or_default();
                        let recv = f.child_by_field_name("object").map(|x| short_text(x, src));
                        (CallKind::Method, prop, recv, args)
                    }
                    _ => (CallKind::Indirect, short_text(f, src), None, args),
                })
            }
            "new_expression" => {
                let c = n.child_by_field_name("constructor")?;
                Some((
                    CallKind::Direct,
                    short_text(c, src),
                    None,
                    n.child_by_field_name("arguments"),
                ))
            }
            _ => None,
        }
    }

    fn is_statement(&self, n: Node) -> bool {
        let k = n.kind();
        (k.ends_with("_statement") && k != "statement_block")
            || matches!(k, "lexical_declaration" | "variable_declaration")
    }

    fn exit(&self, n: Node, src: &[u8]) -> Exit {
        match n.kind() {
            "return_statement" => Exit::Return,
            "throw_statement" => Exit::Throw,
            "break_statement" => Exit::Break(
                n.child_by_field_name("label")
                    .map(|l| text(l, src).to_string()),
            ),
            _ => Exit::None,
        }
    }

    fn is_switch(&self, n: Node) -> bool {
        n.kind() == "switch_statement"
    }

    fn label(&self, n: Node, src: &[u8]) -> Option<String> {
        if n.kind() != "labeled_statement" {
            return None;
        }
        n.child_by_field_name("label")
            .map(|l| text(l, src).to_string())
    }

    /// TypeScript puts a method's decorators before it in the class body;
    /// JavaScript makes them its children (already inside its span).
    fn definition_start(&self, def: Node) -> usize {
        let mut start = def.start_byte();
        let mut prev = def.prev_sibling();
        while let Some(p) = prev {
            if p.kind() != "decorator" {
                break;
            }
            start = p.start_byte();
            prev = p.prev_sibling();
        }
        start
    }
}
