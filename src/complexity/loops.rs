//! # Loop bounds
//!
//! Each loop gets one bound class from its condition (or iterable) and the
//! assignments in its body. Halving shapes are recognised first; then the
//! shape check: the controls may change only by a constant step and the
//! bound's identifiers not at all, or the loop is `Unknown`. Syntactic, not a
//! proof: every classified loop records `LoopShape` as an assumption.

use super::engine::LoopSite;
use super::Grammar;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum BoundClass {
    Constant,
    Size,
    Sentinel,
    Halving,
    Unbounded,
    Unknown,
}

/// What a bound rests on, beyond the text (spec §3.6; closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum Assumption {
    /// An ALL_CAPS name is a constant.
    ConstantName,
    /// A classified loop runs at most its bound.
    LoopShape,
    /// The catalog's amortized or average cost was used.
    CatalogCase,
    /// A project overlay caps a size.
    OverlayCap,
    /// A method's receiver is a built-in type or a repository class.
    BuiltinReceiver,
    /// Several definitions share one IRI; bounds combine over all of them.
    MergedDefinitions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopFact {
    pub site: LoopSite,
    pub class: BoundClass,
    /// The bound as written (at most 200 characters).
    pub bound_expression: String,
    pub controls: Vec<String>,
    pub assumptions: Vec<Assumption>,
}

/// Classify the loops the engine found (`sites`) in the tree under `root`.
pub fn classify_all(root: Node, src: &[u8], grammar: Grammar, sites: &[LoopSite]) -> Vec<LoopFact> {
    let profile = grammar.profile();
    sites
        .iter()
        .map(|s| {
            let found = loop_node(root, s, profile);
            let c = match found {
                Some(n) => classify(n, src),
                None => Classified::unknown(String::new()),
            };
            let mut assumptions = Vec::new();
            if c.constant_name {
                assumptions.push(Assumption::ConstantName);
            }
            if !matches!(c.class, BoundClass::Unbounded | BoundClass::Unknown) {
                assumptions.push(Assumption::LoopShape);
            }
            LoopFact {
                site: s.clone(),
                class: c.class,
                bound_expression: clip(&c.bound),
                controls: c.controls,
                assumptions,
            }
        })
        .collect()
}

fn loop_node<'t>(
    root: Node<'t>,
    s: &LoopSite,
    profile: Option<&dyn super::engine::Profile>,
) -> Option<Node<'t>> {
    let mut n = root.descendant_for_byte_range(s.start_byte, s.end_byte)?;
    loop {
        let exact = n.start_byte() == s.start_byte && n.end_byte() == s.end_byte;
        if exact && profile.is_none_or(|p| p.is_loop(n)) {
            return Some(n);
        }
        n = n.parent()?;
        if n.start_byte() < s.start_byte || n.end_byte() > s.end_byte {
            return None;
        }
    }
}

struct Classified {
    class: BoundClass,
    bound: String,
    controls: Vec<String>,
    constant_name: bool,
}

impl Classified {
    fn of(class: BoundClass, bound: String, controls: Vec<String>) -> Self {
        Classified {
            class,
            bound,
            controls,
            constant_name: false,
        }
    }
    fn unknown(bound: String) -> Self {
        Self::of(BoundClass::Unknown, bound, Vec::new())
    }
}

fn clip(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

fn txt<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.start_byte()..n.end_byte()]).unwrap_or("")
}

fn squash(n: Node, src: &[u8]) -> String {
    txt(n, src).chars().filter(|c| !c.is_whitespace()).collect()
}

fn unparen(mut n: Node) -> Node {
    while n.kind() == "parenthesized_expression" {
        match n.named_child(0) {
            Some(c) => n = c,
            None => break,
        }
    }
    n
}

fn is_number(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_digit()) || (s.starts_with('\'') && s.ends_with('\''))
}

fn is_all_caps(s: &str) -> bool {
    s.len() >= 2
        && s.starts_with(|c: char| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

const SENTINELS: &[&str] = &["NULL", "null", "None", "nullptr", "undefined", "'\\0'", "0"];

/// Kinds that iterate an iterable: (kind, iterable field, loop-variable field).
fn iterable_of<'t>(n: Node<'t>) -> Option<(Node<'t>, Option<Node<'t>>)> {
    let (it, var) = match n.kind() {
        "for_expression" => ("value", "pattern"),
        "for_statement" | "for_in_clause" if n.child_by_field_name("right").is_some() => {
            ("right", "left")
        }
        "for_in_statement" => ("right", "left"),
        _ => return None,
    };
    Some((n.child_by_field_name(it)?, n.child_by_field_name(var)))
}

fn classify(n: Node, src: &[u8]) -> Classified {
    use BoundClass::*;
    if n.kind() == "loop_expression" {
        return Classified::of(Unbounded, String::new(), Vec::new());
    }
    let body = n.child_by_field_name("body");
    if let Some((iterable, var)) = iterable_of(n) {
        let controls: Vec<String> = var
            .map(|v| vec![txt(v, src).to_string()])
            .unwrap_or_default();
        return classify_iterable(iterable, body, src, controls);
    }
    let cond = n
        .child_by_field_name("condition")
        .filter(|c| !matches!(c.kind(), ";" | "empty_statement"));
    let Some(cond) = cond.map(unparen) else {
        return Classified::of(Unbounded, String::new(), Vec::new());
    };
    let cond_text = txt(cond, src).trim();
    if matches!(cond_text, "1" | "true" | "True") {
        return Classified::of(Unbounded, String::new(), Vec::new());
    }
    // `do { … } while (0)`: the body runs once (a `while (0)` never).
    if matches!(cond_text, "0" | "false" | "False") {
        return Classified::of(Constant, cond_text.to_string(), Vec::new());
    }
    if matches!(cond.kind(), "let_condition" | "let_chain") {
        return Classified::of(Sentinel, cond_text.to_string(), Vec::new());
    }
    let mut scopes: Vec<Node> = body.into_iter().collect();
    if let Some(update) = n
        .child_by_field_name("update")
        .or_else(|| n.child_by_field_name("increment"))
    {
        scopes.push(update);
    }
    let assigns = assignments(&scopes, src);
    match comparison(cond, src) {
        Some((op, l, r)) => classify_comparison(op, l, r, cond_text, &assigns, src),
        None => {
            let negated = matches!(cond.kind(), "unary_expression" | "not_operator")
                && cond_text.starts_with(['!', 'n']);
            let combined = matches!(cond.kind(), "boolean_operator")
                || (cond.kind() == "binary_expression"
                    && matches!(
                        cond.child_by_field_name("operator").map(|o| txt(o, src)),
                        Some("&&" | "||")
                    ));
            if negated
                || combined
                || matches!(cond.kind(), "binary_expression" | "comparison_operator")
            {
                Classified::unknown(cond_text.to_string())
            } else {
                Classified::of(Sentinel, cond_text.to_string(), Vec::new())
            }
        }
    }
}

fn comparison<'t>(n: Node<'t>, src: &[u8]) -> Option<(String, Node<'t>, Node<'t>)> {
    match n.kind() {
        "binary_expression" => {
            let op = txt(n.child_by_field_name("operator")?, src).to_string();
            matches!(op.as_str(), "<" | "<=" | ">" | ">=" | "!=" | "!==").then(|| {
                Some((
                    op,
                    n.child_by_field_name("left")?,
                    n.child_by_field_name("right")?,
                ))
            })?
        }
        "comparison_operator" => {
            let mut c = n.walk();
            let named: Vec<Node> = n.named_children(&mut c).collect();
            if named.len() != 2 {
                return None;
            }
            let op = txt(n, src)
                [named[0].end_byte() - n.start_byte()..named[1].start_byte() - n.start_byte()]
                .trim()
                .to_string();
            matches!(op.as_str(), "<" | "<=" | ">" | ">=" | "!=")
                .then_some((op, named[0], named[1]))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Step,
    Halving,
    Other,
}

struct Assign {
    target: String,
    change: Change,
}

/// Every assignment in `scopes`, each judged as a step, a halving or other.
fn assignments(scopes: &[Node], src: &[u8]) -> Vec<Assign> {
    let mut raw: Vec<(String, String, String)> = Vec::new(); // (target, op, rhs squashed)
    let mut defs: Vec<(String, String)> = Vec::new(); // (name, rhs squashed) for mid detection
    for &scope in scopes {
        let mut stack = vec![scope];
        while let Some(n) = stack.pop() {
            let f = |name: &str| n.child_by_field_name(name);
            match n.kind() {
                "update_expression" => {
                    if let Some(a) = f("argument") {
                        raw.push((squash(a, src), "++".into(), String::new()));
                    }
                }
                "assignment_expression"
                | "augmented_assignment_expression"
                | "compound_assignment_expr"
                | "assignment"
                | "augmented_assignment" => {
                    if let (Some(l), Some(r)) = (f("left"), f("right")) {
                        let op = f("operator")
                            .map(|o| txt(o, src).to_string())
                            .unwrap_or_else(|| "=".into());
                        let targets: Vec<String> =
                            if matches!(l.kind(), "pattern_list" | "tuple_pattern" | "tuple") {
                                let mut c = l.walk();
                                l.named_children(&mut c).map(|x| squash(x, src)).collect()
                            } else {
                                vec![squash(l, src)]
                            };
                        let single = targets.len() == 1;
                        for t in targets {
                            let rhs = if single { squash(r, src) } else { "?".into() };
                            if op == "=" {
                                defs.push((t.clone(), rhs.clone()));
                            }
                            raw.push((t, op.clone(), rhs));
                        }
                    }
                }
                "init_declarator" | "variable_declarator" | "let_declaration" => {
                    let name = f("declarator")
                        .or_else(|| f("name"))
                        .or_else(|| f("pattern"));
                    if let (Some(name), Some(v)) = (name, f("value")) {
                        defs.push((squash(name, src), squash(v, src)));
                    }
                }
                _ => {}
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }
    let is_mid = |rhs: &str| {
        rhs.contains('+') && (rhs.contains("/2") || rhs.contains(">>1") || rhs.contains("//2"))
    };
    let mids: Vec<&str> = defs
        .iter()
        .filter(|(_, r)| is_mid(r))
        .map(|(n, _)| n.as_str())
        .collect();
    raw.iter()
        .map(|(t, op, rhs)| {
            let num = |x: &str| is_number(x);
            let change = match op.as_str() {
                "++" => Change::Step,
                "+=" | "-=" if num(rhs) => Change::Step,
                "/=" | "//=" | "*=" if rhs == "2" => Change::Halving,
                ">>=" | "<<=" if rhs == "1" => Change::Halving,
                "=" => {
                    let stepped = [format!("{t}+"), format!("{t}-")]
                        .iter()
                        .any(|p| rhs.strip_prefix(p.as_str()).is_some_and(num));
                    let halved = [
                        format!("{t}/2"),
                        format!("{t}>>1"),
                        format!("{t}//2"),
                        format!("{t}*2"),
                        format!("{t}<<1"),
                    ]
                    .contains(rhs);
                    let to_mid = mids.iter().any(|m| {
                        rhs == m
                            || [format!("{m}+"), format!("{m}-")]
                                .iter()
                                .any(|p| rhs.strip_prefix(p.as_str()).is_some_and(num))
                    });
                    if stepped {
                        Change::Step
                    } else if halved || to_mid {
                        Change::Halving
                    } else {
                        Change::Other
                    }
                }
                _ => Change::Other,
            };
            Assign {
                target: t.clone(),
                change,
            }
        })
        .collect()
}

fn identifiers(n: Node, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        if matches!(
            x.kind(),
            "identifier" | "field_identifier" | "property_identifier"
        ) {
            out.push(txt(x, src).to_string());
        }
        let mut c = x.walk();
        stack.extend(x.children(&mut c));
    }
    out
}

fn bound_class(bound: Node, src: &[u8], controls: Vec<String>) -> Classified {
    let text = txt(bound, src).trim().to_string();
    if is_number(&text) || text.starts_with("sizeof") {
        return Classified::of(BoundClass::Constant, text, controls);
    }
    if is_all_caps(&text) {
        return Classified {
            constant_name: true,
            ..Classified::of(BoundClass::Constant, text, controls)
        };
    }
    Classified::of(BoundClass::Size, text, controls)
}

fn classify_comparison(
    op: String,
    l: Node,
    r: Node,
    cond_text: &str,
    assigns: &[Assign],
    src: &[u8],
) -> Classified {
    let (lt, rt) = (squash(l, src), squash(r, src));
    if op.starts_with("!=")
        && (SENTINELS.contains(&lt.as_str()) || SENTINELS.contains(&rt.as_str()))
    {
        return Classified::of(BoundClass::Sentinel, cond_text.to_string(), Vec::new());
    }
    let assigned = |x: &str| assigns.iter().any(|a| a.target == x);
    let mut controls = Vec::new();
    if assigned(&lt) {
        controls.push(lt.clone());
    }
    if assigned(&rt) {
        controls.push(rt.clone());
    }
    if controls.is_empty() {
        return Classified::unknown(cond_text.to_string());
    }
    let changes: Vec<Change> = assigns
        .iter()
        .filter(|a| controls.contains(&a.target))
        .map(|a| a.change)
        .collect();
    if changes.iter().all(|c| *c == Change::Halving) {
        return Classified::of(BoundClass::Halving, cond_text.to_string(), controls);
    }
    if controls.len() > 1 || changes.iter().any(|c| *c != Change::Step) {
        return Classified::unknown(cond_text.to_string());
    }
    let bound = if controls[0] == lt { r } else { l };
    if identifiers(bound, src).iter().any(|i| assigned(i)) {
        return Classified::unknown(cond_text.to_string());
    }
    bound_class(bound, src, controls)
}

const GROWING: &[&str] = &[
    "append",
    "extend",
    "insert",
    "push",
    "push_back",
    "add",
    "unshift",
    "splice",
    "push_front",
];

fn classify_iterable(
    iterable: Node,
    body: Option<Node>,
    src: &[u8],
    controls: Vec<String>,
) -> Classified {
    let it = unparen(iterable);
    let it_text = squash(it, src);
    if let Some(body) = body {
        let mut stack = vec![body];
        while let Some(x) = stack.pop() {
            let call_on_it = match x.kind() {
                "call" | "call_expression" => x.child_by_field_name("function").is_some_and(|f| {
                    let recv = f
                        .child_by_field_name("object")
                        .or_else(|| f.child_by_field_name("value"));
                    let name = f
                        .child_by_field_name("attribute")
                        .or_else(|| f.child_by_field_name("property"))
                        .or_else(|| f.child_by_field_name("field"));
                    recv.is_some_and(|r| squash(r, src) == it_text)
                        && name.is_some_and(|n| GROWING.contains(&txt(n, src)))
                }),
                _ => false,
            };
            if call_on_it {
                return Classified::unknown(txt(it, src).to_string());
            }
            let mut c = x.walk();
            stack.extend(x.children(&mut c));
        }
    }
    match it.kind() {
        "call"
            if it
                .child_by_field_name("function")
                .is_some_and(|f| txt(f, src) == "range") =>
        {
            let args = it.child_by_field_name("arguments");
            let named: Vec<Node> = match args {
                Some(a) => {
                    let mut c = a.walk();
                    a.named_children(&mut c).collect()
                }
                None => Vec::new(),
            };
            match named.len() {
                1 => bound_class(named[0], src, controls),
                n if n >= 2 => bound_class(named[1], src, controls),
                _ => Classified::unknown(txt(it, src).to_string()),
            }
        }
        "range_expression" => {
            let mut c = it.walk();
            let named: Vec<Node> = it.named_children(&mut c).collect();
            let starts_open = txt(it, src).trim_start().starts_with("..");
            match (named.len(), starts_open) {
                (2, _) | (1, true) => bound_class(*named.last().unwrap(), src, controls),
                _ => Classified::of(BoundClass::Unbounded, String::new(), controls),
            }
        }
        _ => Classified::of(BoundClass::Size, txt(it, src).trim().to_string(), controls),
    }
}

#[cfg(test)]
mod tests {
    use super::BoundClass::*;
    use super::*;
    use crate::complexity::engine::analyze;
    use tree_sitter::Parser;

    /// The bound classes of every loop in function `name`, in source order.
    fn classes(g: Grammar, src: &str, name: &str) -> Vec<(BoundClass, String)> {
        let mut p = Parser::new();
        p.set_language(&g.language().unwrap()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            let named = n
                .child_by_field_name("name")
                .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                == Some(name);
            let decl = n
                .child_by_field_name("declarator")
                .and_then(|d| d.child_by_field_name("declarator"))
                .and_then(|x| x.utf8_text(src.as_bytes()).ok())
                == Some(name);
            if (named || decl) && n.child_by_field_name("body").is_some() {
                let a = analyze(
                    n.child_by_field_name("body").unwrap(),
                    src.as_bytes(),
                    name,
                    g.profile().unwrap(),
                );
                return classify_all(tree.root_node(), src.as_bytes(), g, &a.loops)
                    .into_iter()
                    .map(|f| (f.class, f.bound_expression))
                    .collect();
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
        panic!("no function {name}");
    }

    fn only(v: Vec<(BoundClass, String)>) -> Vec<BoundClass> {
        v.into_iter().map(|x| x.0).collect()
    }

    #[test]
    fn bounds_c() {
        let src = "void f(int n, char *s, struct node *p, int *a) {
    for (int i = 0; i < n; i++) {}
    for (int i = 0; i < 10; i++) {}
    for (int i = 0; i < MAXN; i++) {}
    while (p) { p = p->next; }
    for (int lo = 0, hi = n - 1; lo <= hi; ) { int mid = (lo + hi) / 2; if (a[mid] < 0) lo = mid + 1; else hi = mid - 1; }
    for (;;) { if (n) break; }
    while (1) { if (n) break; }
    for (int i = 0; i < n; i++) { n++; }
    for (int i = 0; i < strlen(s); i++) {}
    while (n > 0) { n /= 2; }
    int k = 0; while (k < n) { k++; }
    while (!done(p)) { p = step(p); }
    while (p != NULL) { p = p->next; }
}
";
        let got = classes(Grammar::C, src, "f");
        assert_eq!(
            only(got.clone()),
            vec![
                Size, Constant, Constant, Sentinel, Halving, Unbounded, Unbounded, Unknown, Size,
                Halving, Size, Unknown, Sentinel
            ]
        );
        assert_eq!(got[0].1, "n");
        assert_eq!(got[8].1, "strlen(s)");
    }

    #[test]
    fn bounds_rust() {
        let src = "fn f(v: &[i32], n: usize, mut it: I) {
    for i in 0..n {}
    for i in 0..10 {}
    for x in v {}
    while let Some(x) = it.next() {}
    loop { break; }
    let mut k = n; while k > 1 { k /= 2; }
    let mut j = 0; while j < n { j += 1; }
}
";
        let got = classes(Grammar::Rust, src, "f");
        assert_eq!(
            only(got.clone()),
            vec![Size, Constant, Size, Sentinel, Unbounded, Halving, Size]
        );
        assert_eq!(got[0].1, "n");
        assert_eq!(got[2].1, "v");
    }

    #[test]
    fn bounds_python() {
        let src = "def f(xs, n, node):
    for i in range(n):
        pass
    for i in range(10):
        pass
    for x in xs:
        pass
    while node:
        node = node.next
    while True:
        break
    lo, hi = 0, n
    while lo < hi:
        mid = (lo + hi) // 2
        if xs[mid] < 0:
            lo = mid + 1
        else:
            hi = mid
    for x in xs:
        xs.append(x)
    ys = [y for y in xs]
";
        let got = classes(Grammar::Python, src, "f");
        assert_eq!(
            only(got.clone()),
            vec![Size, Constant, Size, Sentinel, Unbounded, Halving, Unknown, Size]
        );
        assert_eq!(got[0].1, "n");
    }

    #[test]
    fn bounds_js() {
        let src = "function f(xs, n, node) {
  for (let i = 0; i < n; i++) {}
  for (let i = 0; i < 10; i++) {}
  for (const x of xs) {}
  while (node) { node = node.next; }
  for (;;) { break; }
  while (true) { break; }
  for (let i = 0; i < n; i++) { n--; }
}
";
        assert_eq!(
            only(classes(Grammar::JavaScript, src, "f")),
            vec![Size, Constant, Size, Sentinel, Unbounded, Unbounded, Unknown]
        );
        assert_eq!(
            only(classes(
                Grammar::TypeScript,
                &src.replace("(xs, n, node)", "(xs: number[], n: number, node: N)"),
                "f"
            ))
            .len(),
            7
        );
    }

    #[test]
    fn classified_loops_record_their_assumptions() {
        let src = "void f(int n) { for (int i = 0; i < MAXN; i++) {} for (int i = 0; i < n; i++) {} for (;;) { break; } }";
        let mut p = Parser::new();
        p.set_language(&Grammar::C.language().unwrap()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let body = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let a = analyze(body, src.as_bytes(), "f", Grammar::C.profile().unwrap());
        let facts = classify_all(tree.root_node(), src.as_bytes(), Grammar::C, &a.loops);
        assert_eq!(
            facts[0].assumptions,
            vec![Assumption::ConstantName, Assumption::LoopShape]
        );
        assert_eq!(facts[1].assumptions, vec![Assumption::LoopShape]);
        assert!(
            facts[2].assumptions.is_empty(),
            "nothing is assumed about an unbounded loop"
        );
        assert_eq!(facts[1].controls, vec!["i".to_string()]);
    }
}
