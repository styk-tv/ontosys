//! C profile. Decision points reproduce the C layer's historical
//! `cx:complexity` exactly: `if`, `for`, `while`, `do`, `?:`, each `case`
//! with a value, `&&`, `||`.

use super::super::engine::{Boundary, CallKind, Cog, Exit, Profile};
use super::super::Grammar;
use super::text;
use tree_sitter::Node;

pub struct CProfile;
pub static C: CProfile = CProfile;

fn logical_op<'a>(n: Node, src: &'a [u8]) -> Option<&'a str> {
    if n.kind() != "binary_expression" {
        return None;
    }
    let op = text(n.child_by_field_name("operator")?, src);
    matches!(op, "&&" | "||").then_some(op)
}

impl Profile for CProfile {
    fn grammar(&self) -> Grammar {
        Grammar::C
    }

    fn decisions(&self, n: Node, src: &[u8]) -> usize {
        match n.kind() {
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_statement"
            | "conditional_expression" => 1,
            "case_statement" if n.child_by_field_name("value").is_some() => 1,
            "binary_expression" if logical_op(n, src).is_some() => 1,
            _ => 0,
        }
    }

    fn is_loop(&self, n: Node) -> bool {
        matches!(
            n.kind(),
            "for_statement" | "while_statement" | "do_statement"
        )
    }

    fn loop_condition_field(&self, n: Node) -> Option<&'static str> {
        self.is_loop(n).then_some("condition")
    }

    fn loop_once_field(&self, n: Node) -> Option<&'static str> {
        (n.kind() == "for_statement").then_some("initializer")
    }

    fn is_conditional_child(&self, parent: Node, field: Option<&str>, src: &[u8]) -> bool {
        match parent.kind() {
            "if_statement" | "conditional_expression" => {
                matches!(field, Some("consequence" | "alternative"))
            }
            "case_statement" => field != Some("value"),
            "binary_expression" => field == Some("right") && logical_op(parent, src).is_some(),
            _ => false,
        }
    }

    fn cognitive(&self, n: Node, parent: Option<Node>, _src: &[u8]) -> Cog {
        match n.kind() {
            "if_statement" if parent.is_some_and(|p| p.kind() == "else_clause") => Cog::ElseIf,
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "conditional_expression" => Cog::Nesting,
            "else_clause" => {
                let mut c = n.walk();
                let only_if = n
                    .named_children(&mut c)
                    .filter(|x| !x.kind().contains("comment"))
                    .all(|x| x.kind() == "if_statement");
                if only_if && n.named_child_count() > 0 {
                    Cog::None
                } else {
                    Cog::Flat
                }
            }
            "goto_statement" => Cog::FlatLeaf,
            _ => Cog::None,
        }
    }

    fn bool_op(&self, n: Node, src: &[u8]) -> Option<String> {
        logical_op(n, src).map(str::to_string)
    }

    fn boundary(&self, n: Node) -> Boundary {
        if n.kind() == "function_definition" {
            Boundary::Named
        } else {
            Boundary::None
        }
    }

    fn call<'t>(
        &self,
        n: Node<'t>,
        src: &[u8],
    ) -> Option<(CallKind, String, Option<String>, Option<Node<'t>>)> {
        if n.kind() != "call_expression" {
            return None;
        }
        let func = n.child_by_field_name("function")?;
        let args = n.child_by_field_name("arguments");
        if func.kind() == "identifier" {
            Some((CallKind::Direct, text(func, src).to_string(), None, args))
        } else {
            Some((CallKind::Indirect, super::short_text(func, src), None, args))
        }
    }

    fn is_statement(&self, n: Node) -> bool {
        let k = n.kind();
        k.ends_with("_statement") && k != "compound_statement"
    }

    fn exit(&self, n: Node, src: &[u8]) -> Exit {
        match n.kind() {
            "return_statement" => Exit::Return,
            "break_statement" => Exit::Break(None),
            "goto_statement" => match n.child_by_field_name("label") {
                Some(l) => Exit::Goto(text(l, src).to_string()),
                None => Exit::Goto(String::new()),
            },
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
        Some(text(n.child_by_field_name("label")?, src).to_string())
    }

    fn definition_start(&self, def: Node) -> usize {
        def.start_byte()
    }
}
