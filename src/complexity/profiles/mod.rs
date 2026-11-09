//! Per-grammar profiles: which node kinds are decisions, loops, branches,
//! calls and exits. The engine itself knows no grammar.

pub mod c;

use tree_sitter::Node;

/// `(child, field name)` for every child of `n`, in order.
pub(crate) fn children_with_fields<'t>(n: Node<'t>) -> Vec<(Node<'t>, Option<&'t str>)> {
    let mut out = Vec::with_capacity(n.child_count() as usize);
    let mut c = n.walk();
    if c.goto_first_child() {
        loop {
            out.push((c.node(), c.field_name()));
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
    out
}

/// Source text of a node.
pub(crate) fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.start_byte()..n.end_byte()]).unwrap_or("")
}

/// Text with every whitespace run collapsed to one space, cut at 200 chars.
pub(crate) fn short_text(n: Node, src: &[u8]) -> String {
    clip(
        &text(n, src)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// At most 200 characters, cut on a char boundary.
pub(crate) fn clip(s: &str) -> String {
    s.chars().take(200).collect()
}
