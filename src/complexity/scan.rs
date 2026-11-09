//! # Scan
//!
//! Tier A for Rust, Python and JavaScript/TypeScript: a tree-sitter pass over
//! a file, reporting every named function that has a body. Until the
//! line-based parsers are replaced, these facts are attached to their
//! declarations by [`super::join`].

use super::engine::{analyze, definition_sha256, Boundary, TierA};
use super::profiles::text;
use super::Grammar;
use tree_sitter::{Node, Parser};

/// One named function with a body, as the scan saw it.
#[derive(Debug, Clone)]
pub struct FunctionFacts {
    pub name: String,
    /// Repository-relative path of the file.
    pub file: String,
    /// 1-based line of the function's name (not of its first byte).
    pub name_line: usize,
    pub grammar: Grammar,
    pub tier_a: TierA,
    pub definition_sha256: Option<String>,
    /// Bound class of every loop in `tier_a.loops`, same order.
    pub loops: Vec<super::loops::LoopFact>,
}

/// Facts for every named function with a body, in document order, and the
/// file's error nodes. The file is parsed from its bytes, so every hash is
/// over the bytes as they are.
pub fn scan_file(rel_path: &str, bytes: &[u8], grammar: Grammar) -> (Vec<FunctionFacts>, usize) {
    let (Some(lang), Some(profile)) = (grammar.language(), grammar.profile()) else {
        return (Vec::new(), 0);
    };
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return (Vec::new(), 1);
    }
    let Some(tree) = parser.parse(bytes, None) else {
        return (Vec::new(), 1);
    };
    let root = tree.root_node();
    let errors = if root.has_error() {
        count_errors(root)
    } else {
        0
    };
    let mut out = Vec::new();
    let mut stack = vec![(root, false)];
    while let Some((n, inside_fn)) = stack.pop() {
        if let Some((name_node, def, body)) = named_function(n, grammar, inside_fn) {
            let name = text(name_node, bytes).to_string();
            let mut tier_a = analyze(body, bytes, &name, profile);
            tier_a.memoized = super::engine::detect_memo(def, body, bytes, &name, &tier_a, profile);
            out.push(FunctionFacts {
                loops: super::loops::classify_all(body, bytes, grammar, &tier_a.loops),
                tier_a,
                definition_sha256: Some(definition_sha256(def, bytes, profile)),
                name_line: name_node.start_position().row + 1,
                file: rel_path.to_string(),
                name,
                grammar,
            });
        }
        let enters_function =
            profile.boundary(n) != Boundary::None && !CONTAINERS.contains(&n.kind());
        let mut c = n.walk();
        let kids: Vec<Node> = n.children(&mut c).collect();
        for k in kids.into_iter().rev() {
            stack.push((k, inside_fn || enters_function));
        }
    }
    (out, errors)
}

/// Named boundaries that hold functions rather than being one.
const CONTAINERS: &[&str] = &[
    "class_declaration",
    "class",
    "abstract_class_declaration",
    "class_definition",
    "impl_item",
    "trait_item",
    "mod_item",
];

const JS_FUNCTION_VALUES: &[&str] = &[
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
];

/// `(name node, definition node, body node)` when `n` is a named function
/// with a body. A JavaScript arrow or function expression is named only when
/// bound outside any function (module or class level, or an object
/// property); inside a function it is anonymous and counted in its parent.
fn named_function<'t>(
    n: Node<'t>,
    g: Grammar,
    inside_fn: bool,
) -> Option<(Node<'t>, Node<'t>, Node<'t>)> {
    let field = |x: Node<'t>, f: &str| x.child_by_field_name(f);
    match (g, n.kind()) {
        (Grammar::Rust, "function_item") | (Grammar::Python, "function_definition") => {
            Some((field(n, "name")?, n, field(n, "body")?))
        }
        (Grammar::C, _) | (Grammar::Rust, _) | (Grammar::Python, _) => None,
        (_, "function_declaration" | "generator_function_declaration" | "method_definition") => {
            Some((field(n, "name")?, n, field(n, "body")?))
        }
        (_, "variable_declarator" | "pair" | "public_field_definition" | "field_definition")
            if !inside_fn =>
        {
            let value = field(n, "value")?;
            if !JS_FUNCTION_VALUES.contains(&value.kind()) {
                return None;
            }
            let name = field(n, "name")
                .or_else(|| field(n, "key"))
                .or_else(|| field(n, "property"))?;
            Some((name, value, field(value, "body")?))
        }
        _ => None,
    }
}

fn count_errors(root: Node) -> usize {
    let mut n_err = 0;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.is_error() || n.is_missing() {
            n_err += 1;
        }
        if n.has_error() {
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }
    n_err
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(g: Grammar, src: &str) -> Vec<(String, usize)> {
        scan_file("x", src.as_bytes(), g)
            .0
            .into_iter()
            .map(|f| (f.name, f.name_line))
            .collect()
    }

    #[test]
    fn named_functions_with_bodies_are_found_on_their_name_line() {
        let py = "@dec\ndef top(a):\n    def inner():\n        return 1\n    return inner()\n\nclass K:\n    def m(self):\n        return lambda x: x\n";
        assert_eq!(
            names(Grammar::Python, py),
            vec![("top".into(), 2), ("inner".into(), 3), ("m".into(), 8)]
        );
        let rs = "/// d\n#[inline]\nfn a() {}\nimpl S {\n    pub fn\n    b(&self) {}\n}\ntrait T { fn c(&self); fn d(&self) {} }\n";
        assert_eq!(
            names(Grammar::Rust, rs),
            vec![("a".into(), 3), ("b".into(), 6), ("d".into(), 8)]
        );
    }

    #[test]
    fn bodyless_declarations_get_no_facts() {
        let ts = "function over(a: string): void;\nfunction over(a: any) { return a; }\ndeclare function amb(): void;\nabstract class A { abstract m(): void; n() { return 1; } }\ninterface I { q(): void }\n";
        assert_eq!(
            names(Grammar::TypeScript, ts),
            vec![("over".into(), 2), ("n".into(), 4)]
        );
        let py = "def stub(): ...\n";
        let f = &scan_file("x", py.as_bytes(), Grammar::Python).0[0];
        assert_eq!(
            (f.name.as_str(), f.tier_a.cyclomatic),
            ("stub", 1),
            "a `...` body is a body"
        );
    }

    #[test]
    fn js_named_arrows_at_module_and_class_level_only() {
        let js = "const a = () => 1;\nexport const b = function () { const c = () => 2; return c(); };\nconst o = { d: () => 3 };\nclass K { e = () => 4; f() {} }\n";
        assert_eq!(
            names(Grammar::JavaScript, js),
            vec![
                ("a".into(), 1),
                ("b".into(), 2),
                ("d".into(), 3),
                ("e".into(), 4),
                ("f".into(), 4)
            ]
        );
    }

    #[test]
    fn hashes_cover_the_exact_bytes_and_errors_are_counted() {
        let (facts, errors) = scan_file(
            "x",
            b"def f(:\n    return 1\n\ndef g():\n    return 2\n",
            Grammar::Python,
        );
        assert!(errors > 0);
        let g = facts.iter().find(|f| f.name == "g").unwrap();
        assert_eq!(g.definition_sha256.as_deref().map(str::len), Some(64));
    }
}
