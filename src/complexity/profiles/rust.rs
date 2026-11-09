//! Rust profile. Decision points: `if` (and `if let`), `while` (and
//! `while let`), `for`, `loop`, each `match` arm except the last, `?`, `&&`,
//! `||`.

use super::super::engine::{Boundary, CallKind, Cog, Exit, Profile};
use super::super::Grammar;
use super::{short_text, text};
use tree_sitter::Node;

pub struct RustProfile;
pub static RUST: RustProfile = RustProfile;

fn logical_op<'a>(n: Node, src: &'a [u8]) -> Option<&'a str> {
    if n.kind() != "binary_expression" {
        return None;
    }
    let op = text(n.child_by_field_name("operator")?, src);
    matches!(op, "&&" | "||").then_some(op)
}

fn label_of(n: Node, src: &[u8]) -> Option<String> {
    let mut c = n.walk();
    let found = n.children(&mut c).find(|x| x.kind() == "label");
    found.map(|l| text(l, src).to_string())
}

impl Profile for RustProfile {
    fn grammar(&self) -> Grammar {
        Grammar::Rust
    }

    fn decisions(&self, n: Node, src: &[u8]) -> usize {
        match n.kind() {
            "if_expression" | "while_expression" | "for_expression" | "loop_expression"
            | "try_expression" => 1,
            "match_block" => {
                let mut c = n.walk();
                let arms = n
                    .named_children(&mut c)
                    .filter(|x| x.kind() == "match_arm")
                    .count();
                arms.saturating_sub(1)
            }
            "binary_expression" if logical_op(n, src).is_some() => 1,
            _ => 0,
        }
    }

    fn is_loop(&self, n: Node) -> bool {
        matches!(
            n.kind(),
            "for_expression" | "while_expression" | "loop_expression"
        )
    }

    fn loop_condition_field(&self, n: Node) -> Option<&'static str> {
        (n.kind() == "while_expression").then_some("condition")
    }

    fn loop_once_field(&self, n: Node) -> Option<&'static str> {
        (n.kind() == "for_expression").then_some("value")
    }

    fn is_conditional_child(&self, parent: Node, field: Option<&str>, src: &[u8]) -> bool {
        match parent.kind() {
            "if_expression" => matches!(field, Some("consequence" | "alternative")),
            "match_arm" => field == Some("value"),
            "binary_expression" => field == Some("right") && logical_op(parent, src).is_some(),
            _ => false,
        }
    }

    fn cognitive(&self, n: Node, parent: Option<Node>, src: &[u8]) -> Cog {
        match n.kind() {
            "if_expression" if parent.is_some_and(|p| p.kind() == "else_clause") => Cog::ElseIf,
            "if_expression" | "for_expression" | "while_expression" | "loop_expression"
            | "match_expression" => Cog::Nesting,
            "else_clause" => {
                let mut c = n.walk();
                let kids: Vec<Node> = n.named_children(&mut c).collect();
                if kids.len() == 1 && kids[0].kind() == "if_expression" {
                    Cog::None
                } else {
                    Cog::Flat
                }
            }
            "break_expression" | "continue_expression" if label_of(n, src).is_some() => {
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
            "function_item" | "impl_item" | "trait_item" | "mod_item" => Boundary::Named,
            "closure_expression" => Boundary::Anonymous,
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
                let mut f = n.child_by_field_name("function")?;
                if f.kind() == "generic_function" {
                    f = f.child_by_field_name("function")?;
                }
                let args = n.child_by_field_name("arguments");
                Some(match f.kind() {
                    "identifier" => (CallKind::Direct, text(f, src).to_string(), None, args),
                    "scoped_identifier" => {
                        let path = f
                            .child_by_field_name("path")
                            .map(|p| text(p, src))
                            .unwrap_or("");
                        let name = f
                            .child_by_field_name("name")
                            .map(|p| text(p, src))
                            .unwrap_or("");
                        if matches!(path, "Self" | "self") {
                            (CallKind::Direct, name.to_string(), None, args)
                        } else {
                            (CallKind::Direct, short_text(f, src), None, args)
                        }
                    }
                    "field_expression" => {
                        let field = f
                            .child_by_field_name("field")
                            .map(|x| text(x, src).to_string())
                            .unwrap_or_default();
                        let recv = f.child_by_field_name("value").map(|x| short_text(x, src));
                        (CallKind::Method, field, recv, args)
                    }
                    _ => (CallKind::Indirect, short_text(f, src), None, args),
                })
            }
            "macro_invocation" => {
                let m = n.child_by_field_name("macro")?;
                let mut c = n.walk();
                let tokens = n.children(&mut c).find(|x| x.kind() == "token_tree");
                Some((CallKind::Macro, format!("{}!", text(m, src)), None, tokens))
            }
            _ => None,
        }
    }

    fn is_statement(&self, n: Node) -> bool {
        matches!(n.kind(), "expression_statement" | "let_declaration")
    }

    fn exit(&self, n: Node, src: &[u8]) -> Exit {
        match n.kind() {
            "return_expression" => Exit::Return,
            "break_expression" => Exit::Break(label_of(n, src)),
            "try_expression" => Exit::Maybe,
            _ => Exit::None,
        }
    }

    fn is_switch(&self, _n: Node) -> bool {
        false
    }

    fn label(&self, _n: Node, _src: &[u8]) -> Option<String> {
        None
    }

    /// Attribute items directly above the function (nothing between them,
    /// so a doc comment ends the run) start its span.
    fn definition_start(&self, def: Node) -> usize {
        let mut start = def.start_byte();
        let mut prev = def.prev_sibling();
        while let Some(p) = prev {
            if p.kind() != "attribute_item" {
                break;
            }
            start = p.start_byte();
            prev = p.prev_sibling();
        }
        start
    }
}
