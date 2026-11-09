//! Per-grammar profiles: which node kinds are decisions, loops, branches,
//! calls and exits. The engine itself knows no grammar.

pub mod c;
pub mod js;
pub mod python;
pub mod rust;

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

#[cfg(test)]
mod tests {
    use super::super::engine::{analyze, definition_sha256, CallKind, TierA};
    use super::super::Grammar;
    use tree_sitter::{Node, Parser};

    const FUNCTION_KINDS: &[&str] = &[
        "function_definition",
        "function_item",
        "function_declaration",
        "generator_function_declaration",
        "method_definition",
    ];

    fn name_of<'a>(n: Node, src: &'a str) -> Option<&'a str> {
        n.child_by_field_name("name")
            .and_then(|x| x.utf8_text(src.as_bytes()).ok())
    }

    /// Parse `src` with `g`, find the function called `name`, and hand its
    /// definition and body to `f`.
    fn with_fn<T>(g: Grammar, src: &str, name: &str, f: impl FnOnce(Node, Node, &[u8]) -> T) -> T {
        let mut p = Parser::new();
        p.set_language(&g.language().expect("grammar linked"))
            .unwrap();
        let tree = p.parse(src, None).unwrap();
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            if FUNCTION_KINDS.contains(&n.kind()) && name_of(n, src) == Some(name) {
                return f(n, n.child_by_field_name("body").unwrap(), src.as_bytes());
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
        panic!("no function {name}");
    }

    fn tier_a(g: Grammar, src: &str, name: &str) -> TierA {
        with_fn(g, src, name, |_, body, b| {
            analyze(body, b, name, g.profile().expect("profile"))
        })
    }

    fn sha_start(g: Grammar, src: &str, name: &str) -> String {
        with_fn(g, src, name, |def, _, b| {
            definition_sha256(def, b, g.profile().unwrap())
        })
    }

    fn sha_of(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn grammars_load_under_runtime_0_27() {
        for g in [
            Grammar::C,
            Grammar::Rust,
            Grammar::Python,
            Grammar::JavaScript,
            Grammar::TypeScript,
            Grammar::Tsx,
        ] {
            let mut p = Parser::new();
            p.set_language(&g.language().unwrap_or_else(|| panic!("{g:?} not linked")))
                .expect("ABI compatible");
            assert!(g.profile().is_some(), "{g:?} has a profile");
        }
    }

    const CLASSIFY_RS: &str = "fn classify(v: &[i32]) -> Result<u32, E> {
    let mut n = 0;
    for x in v {
        match x {
            0 => n += 1,
            1 | 2 => n += 2,
            _ => {}
        }
    }
    let y = parse()?;
    if n > 3 && y { Ok(n) } else { Ok(0) }
}
";

    #[test]
    fn classify_rust() {
        let a = tier_a(Grammar::Rust, CLASSIFY_RS, "classify");
        assert_eq!(
            (a.cyclomatic, a.cognitive, a.max_nesting, a.loop_count),
            (7, 6, 2, 1)
        );
        assert!(a.calls.iter().any(|c| c.callee == "parse"));
    }

    #[test]
    fn js_logical_assignments_are_decisions() {
        let a = tier_a(
            Grammar::JavaScript,
            "function f(e, r) { e.kind ||= r.kind; e.at ??= r.at; e.ok &&= r.ok; e.n += 1; }",
            "f",
        );
        assert_eq!(a.cyclomatic, 4);
    }

    #[test]
    fn match_guards_are_decisions() {
        let a = tier_a(
            Grammar::Rust,
            "fn f(c: u8, n: usize) -> u8 { match c { b'%' if n > 2 => 1, b'+' => 2, _ => 3 } }",
            "f",
        );
        assert_eq!(a.cyclomatic, 4, "two arms past the first, plus one guard");
        let p = tier_a(Grammar::Python, "def f(x):\n    match x:\n        case [a] if a > 0:\n            return 1\n        case _:\n            return 0\n", "f");
        assert_eq!(p.cyclomatic, 3, "one non-wildcard case, plus one guard");
    }

    #[test]
    fn rust_let_else_is_a_decision() {
        let a = tier_a(
            Grammar::Rust,
            "fn f(o: Option<u8>) -> u8 { let Some(x) = o else { return 0; }; x }",
            "f",
        );
        assert_eq!(a.cyclomatic, 2);
    }

    #[test]
    fn rust_question_mark_is_a_conditional_exit() {
        let a = tier_a(
            Grammar::Rust,
            "fn f(v: &[u8]) -> R { let x = g()?; for b in v { h(b); } Ok(()) }",
            "f",
        );
        assert!(a.loops[0].conditional, "after a `?` that may return");
        let b = tier_a(
            Grammar::Rust,
            "fn f(v: &[u8]) -> R { for b in v { h(b)?; } Ok(()) }",
            "f",
        );
        assert!(b.loops[0].early_exit, "a `?` inside the loop can leave it");
    }

    #[test]
    fn rust_scoped_calls_are_not_self_calls_unless_self() {
        let a = tier_a(
            Grammar::Rust,
            "fn new() -> S { inner::new(); Self::new(); x.new(); S {} }",
            "new",
        );
        assert_eq!(a.self_calls, 1, "only Self::new");
        let kinds: Vec<CallKind> = a.calls.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![CallKind::Direct, CallKind::Direct, CallKind::Method]
        );
    }

    #[test]
    fn rust_attributes_start_the_span_doc_comments_do_not() {
        let src = "/// doc\n#[inline]\n#[must_use]\nfn f() -> u8 { 1 }\n";
        let start = src.find("#[inline]").unwrap();
        assert_eq!(
            sha_start(Grammar::Rust, src, "f"),
            sha_of(src[start..].trim_end().as_bytes())
        );
        let plain = "/// doc\nfn g() -> u8 { 2 }\n";
        assert_eq!(
            sha_start(Grammar::Rust, plain, "g"),
            sha_of(plain[plain.find("fn").unwrap()..].trim_end().as_bytes())
        );
    }

    const FIB_PY: &str = "@lru_cache(maxsize=None)\ndef fib(n):\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n";

    #[test]
    fn fib_python_decorated() {
        let a = tier_a(Grammar::Python, FIB_PY, "fib");
        assert_eq!((a.cyclomatic, a.cognitive, a.self_calls), (2, 2, 2));
        assert_eq!(
            sha_start(Grammar::Python, FIB_PY, "fib"),
            sha_of(FIB_PY.trim_end().as_bytes()),
            "the span starts at @"
        );
    }

    const PICK_PY: &str = "def pick(items, k):
    out = [x for x in items if x > k]
    for i in range(len(items)):
        if items[i] in out and i % 2:
            continue
        elif i > 10:
            break
    try:
        pass
    except ValueError:
        return None
    return out
";

    #[test]
    fn pick_python() {
        let a = tier_a(Grammar::Python, PICK_PY, "pick");
        assert_eq!((a.cyclomatic, a.cognitive, a.max_nesting), (8, 6, 2));
        assert_eq!((a.loop_count, a.max_loop_nesting), (2, 1));
    }

    #[test]
    fn python_in_is_a_contains_call() {
        let a = tier_a(
            Grammar::Python,
            "def f(x, seen):\n    if x in seen:\n        return 1\n    return 0\n",
            "f",
        );
        let c = a
            .calls
            .iter()
            .find(|c| c.kind == CallKind::Operator)
            .expect("an operator call");
        assert_eq!(
            (c.callee.as_str(), c.receiver.as_deref()),
            ("__contains__", Some("seen"))
        );
    }

    #[test]
    fn python_iterable_is_evaluated_once() {
        let a = tier_a(
            Grammar::Python,
            "def f(xs):\n    for i in range(len(xs)):\n        g(i)\n",
            "f",
        );
        let site = |n: &str| {
            a.calls
                .iter()
                .find(|c| c.callee == n)
                .unwrap()
                .loop_path
                .clone()
        };
        assert!(site("range").is_empty() && site("len").is_empty());
        assert_eq!(site("g"), vec![0]);
    }

    const SCAN_JS: &str = "function scan(xs, t) {
  for (const x of xs) {
    if (x === t) return true;
  }
  const f = (y) => y ?? 0;
  return xs.length > 0 ? f(xs[0]) : false;
}
";

    #[test]
    fn scan_js() {
        let a = tier_a(Grammar::JavaScript, SCAN_JS, "scan");
        assert_eq!((a.cyclomatic, a.cognitive, a.max_nesting), (5, 5, 2));
    }

    #[test]
    fn scan_ts_and_tsx() {
        let ts = "function scan(xs: number[], t: number): boolean {
  for (const x of xs) {
    if (x === t) return true;
  }
  const f = (y?: number): number => y ?? 0;
  return xs.length > 0 ? f(xs[0]) > 0 : false;
}
";
        let a = tier_a(Grammar::TypeScript, ts, "scan");
        assert_eq!((a.cyclomatic, a.cognitive, a.max_nesting), (5, 5, 2));
        let b = tier_a(Grammar::Tsx, ts, "scan");
        assert_eq!((b.cyclomatic, b.cognitive, b.max_nesting), (5, 5, 2));
        let jsx = "function View(p: P) { return <div>{p.on && <A/>}</div>; }";
        assert_eq!(tier_a(Grammar::Tsx, jsx, "View").cyclomatic, 2);
    }

    #[test]
    fn js_member_calls_are_methods_and_new_is_direct() {
        let a = tier_a(
            Grammar::JavaScript,
            "function f(a) { a.push(1); new Map(); g()(); }",
            "f",
        );
        let got: Vec<(CallKind, &str, Option<&str>)> = a
            .calls
            .iter()
            .map(|c| (c.kind, c.callee.as_str(), c.receiver.as_deref()))
            .collect();
        assert!(got.contains(&(CallKind::Method, "push", Some("a"))));
        assert!(got.contains(&(CallKind::Direct, "Map", None)));
        assert!(got.iter().any(|c| c.0 == CallKind::Indirect), "{got:?}");
    }

    #[test]
    fn ts_decorators_start_a_method_span() {
        let src = "class C {\n  @memo()\n  @log\n  run(n: number) { return n; }\n}\n";
        let start = src.find("@memo").unwrap();
        let end = src.find("return n; }").unwrap() + "return n; }".len();
        assert_eq!(
            sha_start(Grammar::TypeScript, src, "run"),
            sha_of(&src.as_bytes()[start..end])
        );
    }

    /// A comprehension's body and later clauses run inside its loops
    /// (final review C4).
    #[test]
    fn python_comprehension_scopes_nest() {
        let a = tier_a(
            Grammar::Python,
            "def f(xs):\n    return [g(x) for x in xs for y in k(x) if h(y)]\n",
            "f",
        );
        assert_eq!((a.loop_count, a.max_loop_nesting), (2, 2));
        assert_eq!(a.loops[1].parent, Some(0));
        let site = |n: &str| a.calls.iter().find(|c| c.callee == n).unwrap().clone();
        assert_eq!(site("g").loop_path, vec![0, 1]);
        assert!(
            site("g").conditional,
            "the body runs only when the filter holds"
        );
        assert_eq!(
            site("k").loop_path,
            vec![0],
            "a later iterable runs once per outer item"
        );
        assert_eq!(site("h").loop_path, vec![0, 1]);
        assert!(!site("h").conditional);
    }

    /// `continue` and `break` make the rest of their loop's body conditional;
    /// an exit inside a loop body is a conditional exit (final review C9).
    #[test]
    fn continue_break_and_returns_in_loops_cut_lower_bounds() {
        let r = tier_a(
            Grammar::Rust,
            "fn f(v: &[i32]) { for x in v { if *x > 0 { continue; } g(); } }",
            "f",
        );
        assert!(
            r.calls
                .iter()
                .find(|c| c.callee == "g")
                .unwrap()
                .conditional
        );
        let p = tier_a(
            Grammar::Python,
            "def f(xs):\n    for x in xs:\n        if x:\n            continue\n        g()\n",
            "f",
        );
        assert!(
            p.calls
                .iter()
                .find(|c| c.callee == "g")
                .unwrap()
                .conditional
        );
        let j = tier_a(
            Grammar::JavaScript,
            "function f(xs) { for (const x of xs) { if (x) continue; g(); } }",
            "f",
        );
        assert!(
            j.calls
                .iter()
                .find(|c| c.callee == "g")
                .unwrap()
                .conditional
        );
    }

    /// A call inside a lambda runs only if something invokes the lambda
    /// (final review C5, C9).
    #[test]
    fn calls_inside_anonymous_functions_are_conditional() {
        let a = tier_a(
            Grammar::JavaScript,
            "function f(xs) { return xs.map(x => g(x)); }",
            "f",
        );
        let g = a.calls.iter().find(|c| c.callee == "g").unwrap();
        assert!(g.conditional);
        assert!(g.in_anonymous.is_some());
        let map = a.calls.iter().find(|c| c.callee == "map").unwrap();
        assert!(map.in_anonymous.is_none());
    }

    /// Generators and async functions defer their bodies (final review C9).
    #[test]
    fn generators_are_deferred() {
        let a = tier_a(
            Grammar::Python,
            "def f(xs):\n    for x in xs:\n        yield x\n",
            "f",
        );
        assert!(a.deferred);
        let b = tier_a(
            Grammar::Python,
            "def f(xs):\n    g = lambda: (yield 1)\n    return xs\n",
            "f",
        );
        assert!(!b.deferred, "a yield inside a nested function is not f's");
        let j = tier_a(
            Grammar::JavaScript,
            "function* f(xs) { for (const x of xs) yield x; }",
            "f",
        );
        assert!(j.deferred);
    }

    /// Calls inside a Rust macro's arguments are seen (final review C7).
    #[test]
    fn rust_macro_arguments_are_reparsed_for_calls() {
        let a = tier_a(
            Grammar::Rust,
            "fn f(v: &Vec<i32>, x: i32) {\n    assert!(v.contains(&x));\n    let s = format!(\"{}\", heavy(v));\n    for y in v { assert_eq!(compute(*y), 1); }\n}\n",
            "f",
        );
        let site = |n: &str| a.calls.iter().find(|c| c.callee == n).cloned();
        let contains = site("contains").expect("a call inside assert!");
        assert_eq!(contains.line, 2);
        assert_eq!(contains.receiver.as_deref(), Some("v"));
        assert!(site("heavy").is_some());
        assert_eq!(site("compute").expect("inside a loop").loop_path, vec![0]);
        let src = "fn f(v: &Vec<i32>, x: i32) {\n    assert!(v.contains(&x));\n}\n";
        let c = tier_a(Grammar::Rust, src, "f");
        let at = c
            .calls
            .iter()
            .find(|c| c.callee == "contains")
            .unwrap()
            .start_byte;
        assert_eq!(
            &src[at..at + 10],
            "v.contains",
            "byte offsets point into the file"
        );
    }
}
