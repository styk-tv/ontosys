//! # Findings
//!
//! Shapes worth a person's attention, each with its evidence: a linear call
//! in a loop condition, list membership or string concatenation inside a
//! loop, a sort inside a loop, two nested loops over one collection, and
//! recursion that recomputes (several decrementing self-calls, no cache).

use super::catalog::Catalog;
use super::engine::LoopSite;
use super::model::DefRec;
use super::Grammar;
use std::collections::BTreeSet;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum FindingKind {
    LinearCallInLoopCondition,
    ListMembershipInLoop,
    ConcatInLoop,
    SortInLoop,
    SameCollectionNestedLoops,
    UnmemoizedMultiRecursion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub line: usize,
    pub text: String,
}

/// Syntax the findings need that the model does not keep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    /// Names bound to a list in the function (Python `[...]`/`list(...)`,
    /// Rust `vec![...]`/`Vec::new()`/`collect::<Vec<_>>()`).
    pub list_names: BTreeSet<String>,
    /// String concatenations inside a loop: (line, text).
    pub concat_in_loops: Vec<(usize, String)>,
}

fn txt<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.start_byte()..n.end_byte()]).unwrap_or("")
}

fn clip(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

const STRING_KINDS: &[&str] = &[
    "string",
    "concatenated_string",
    "template_string",
    "string_literal",
];

/// Collect the evidence from a function body while its tree is alive.
pub fn evidence(body: Node, src: &[u8], grammar: Grammar, loops: &[LoopSite]) -> Evidence {
    let mut ev = Evidence::default();
    let in_loop = |at: usize| loops.iter().any(|l| at > l.start_byte && at < l.end_byte);
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        let f = |name: &str| n.child_by_field_name(name);
        match (grammar, n.kind()) {
            (Grammar::Python, "assignment") => {
                if let (Some(l), Some(r)) = (f("left"), f("right")) {
                    let listy = matches!(r.kind(), "list" | "list_comprehension")
                        || (r.kind() == "call"
                            && r.child_by_field_name("function")
                                .is_some_and(|x| txt(x, src) == "list"));
                    if l.kind() == "identifier" && listy {
                        ev.list_names.insert(txt(l, src).to_string());
                    }
                    let concat = r.kind() == "binary_operator"
                        && r.child_by_field_name("left")
                            .is_some_and(|x| txt(x, src) == txt(l, src))
                        && r.child_by_field_name("right")
                            .is_some_and(|x| STRING_KINDS.contains(&x.kind()));
                    if concat && in_loop(n.start_byte()) {
                        ev.concat_in_loops
                            .push((n.start_position().row + 1, clip(txt(n, src))));
                    }
                }
            }
            (Grammar::Rust, "let_declaration") => {
                if let (Some(p), Some(v)) = (f("pattern"), f("value")) {
                    let vt: String = txt(v, src).chars().filter(|c| !c.is_whitespace()).collect();
                    let listy = vt.starts_with("vec!")
                        || vt.contains("Vec::new")
                        || vt.contains("Vec::with_capacity")
                        || vt.contains("collect::<Vec");
                    if listy {
                        ev.list_names
                            .insert(txt(p, src).trim_start_matches("mut ").trim().to_string());
                    }
                }
            }
            (Grammar::Python, "augmented_assignment")
            | (
                Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx,
                "augmented_assignment_expression",
            ) => {
                let plus = f("operator").is_some_and(|o| txt(o, src) == "+=");
                if plus
                    && f("right").is_some_and(|r| STRING_KINDS.contains(&r.kind()))
                    && in_loop(n.start_byte())
                {
                    ev.concat_in_loops
                        .push((n.start_position().row + 1, clip(txt(n, src))));
                }
            }
            _ => {}
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    ev.concat_in_loops.sort();
    ev
}

const SORTS: &[&str] = &[
    "sort",
    "sorted",
    "qsort",
    "sort_by",
    "sort_by_key",
    "sort_unstable",
    "sort_unstable_by",
    "sort_unstable_by_key",
    "toSorted",
];

/// Plain names in an expression: not called names, not properties.
fn plain_names(expr: &str) -> BTreeSet<String> {
    let b = expr.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let after = expr[i..].trim_start();
            let before = expr[..start].trim_end();
            if !after.starts_with('(') && !before.ends_with('.') && !before.ends_with("->") {
                out.insert(expr[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

fn decremented(args: &[String]) -> bool {
    args.iter().any(|a| {
        let a: String = a.chars().filter(|c| !c.is_whitespace()).collect();
        let tail = a
            .rsplit_once('-')
            .map(|(l, r)| !l.is_empty() && !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()));
        tail == Some(true) || a.ends_with("[1:]")
    })
}

/// The findings of one definition.
pub fn findings(
    d: &DefRec,
    ev: &Evidence,
    cat: &Catalog,
    grammar: Grammar,
    name: &str,
) -> Vec<Finding> {
    use FindingKind::*;
    let mut out = Vec::new();
    let a = &d.tier_a;
    let mut add = |kind, line, text: String| {
        out.push(Finding {
            kind,
            line,
            text: clip(&text),
        })
    };
    for c in &a.calls {
        let call = format!("{}({})", c.callee, c.args.join(", "));
        if let Some(l) = c.in_condition_of {
            let linear = cat
                .lookup(grammar, &c.callee)
                .iter()
                .any(|e| e.worst.degree >= 1 || e.worst.exp_base.is_some());
            if linear {
                let line = a.loops.get(l as usize).map_or(c.line, |x| x.line);
                add(
                    LinearCallInLoopCondition,
                    c.line,
                    format!("{call} runs on every iteration of the loop at line {line}"),
                );
            }
        }
        if c.loop_path.is_empty() {
            continue;
        }
        let recv = c.receiver.as_deref().unwrap_or("");
        let membership = match grammar {
            Grammar::Python => c.callee == "__contains__" && ev.list_names.contains(recv),
            Grammar::Rust => c.callee == "contains" && ev.list_names.contains(recv),
            Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
                c.receiver.is_some()
                    && matches!(c.callee.as_str(), "includes" | "indexOf" | "lastIndexOf")
            }
            Grammar::C => false,
        };
        if membership {
            add(
                ListMembershipInLoop,
                c.line,
                format!("linear search of {recv} inside a loop"),
            );
        }
        if SORTS.contains(&c.callee.as_str()) {
            add(
                SortInLoop,
                c.line,
                format!(
                    "{} inside a loop",
                    if recv.is_empty() {
                        call
                    } else {
                        format!("{recv}.{}()", c.callee)
                    }
                ),
            );
        }
    }
    for (line, text) in &ev.concat_in_loops {
        add(ConcatInLoop, *line, text.clone());
    }
    for l in &d.loops {
        let Some(p) = l.site.parent.and_then(|p| d.loops.get(p as usize)) else {
            continue;
        };
        let controls: BTreeSet<String> = l.controls.iter().chain(&p.controls).cloned().collect();
        let shared: Vec<String> = plain_names(&l.bound_expression)
            .intersection(&plain_names(&p.bound_expression))
            .filter(|n| !controls.contains(*n))
            .cloned()
            .collect();
        if !shared.is_empty() {
            add(
                SameCollectionNestedLoops,
                l.site.line,
                format!("nested loops both over {}", shared.join(", ")),
            );
        }
    }
    let selfs: Vec<_> = a
        .calls
        .iter()
        .filter(|c| c.callee == name && c.loop_path.is_empty())
        .collect();
    if selfs.len() >= 2 && !a.memoized && selfs.iter().all(|c| decremented(&c.args)) {
        add(
            UnmemoizedMultiRecursion,
            selfs[0].line,
            format!("{} self-calls on n - k without a cache", selfs.len()),
        );
    }
    out.sort_by(|x, y| (x.line, x.kind).cmp(&(y.line, y.kind)));
    out
}

#[cfg(test)]
mod tests {
    use super::FindingKind::*;
    use super::*;
    use crate::complexity::catalog::shipped;
    use crate::complexity::scan::scan_file;

    fn kinds(g: Grammar, src: &str, name: &str) -> Vec<FindingKind> {
        let facts = scan_file("f", src.as_bytes(), g).0;
        let f = facts
            .into_iter()
            .find(|f| f.name == name)
            .expect("function");
        let d = DefRec {
            tier_a: f.tier_a,
            loops: f.loops,
            evidence: f.evidence.clone(),
            definition_sha256: None,
            targets: vec![],
            stub: false,
        };
        findings(&d, &f.evidence, &shipped(), g, name)
            .into_iter()
            .map(|x| x.kind)
            .collect()
    }

    fn c_kinds(src: &str, name: &str) -> Vec<FindingKind> {
        let file = crate::lang_c::extract(&mut crate::lang_c::new_parser(), "f.c", src, None);
        let f = file.functions.into_iter().find(|f| f.name == name).unwrap();
        let d = DefRec {
            tier_a: f.tier_a,
            loops: f.loops,
            evidence: f.evidence.clone(),
            definition_sha256: None,
            targets: vec![],
            stub: false,
        };
        findings(&d, &f.evidence, &shipped(), Grammar::C, name)
            .into_iter()
            .map(|x| x.kind)
            .collect()
    }

    #[test]
    fn linear_call_in_loop_condition() {
        assert_eq!(
            c_kinds(
                "int f(char *s) { int c = 0; for (int i = 0; i < strlen(s); i++) c++; return c; }",
                "f"
            ),
            vec![LinearCallInLoopCondition]
        );
        assert!(
            c_kinds(
                "int g(int n) { int c = 0; for (int i = 0; i < abs(n); i++) c++; return c; }",
                "g"
            )
            .is_empty(),
            "abs is O(1)"
        );
    }

    #[test]
    fn list_membership_in_loop() {
        assert_eq!(kinds(Grammar::Python, "def f(xs):\n    seen = []\n    for x in xs:\n        if x in seen:\n            pass\n", "f"), vec![ListMembershipInLoop]);
        assert!(
            kinds(
                Grammar::Python,
                "def g(xs, seen):\n    for x in xs:\n        if x in seen:\n            pass\n",
                "g"
            )
            .is_empty(),
            "no evidence seen is a list"
        );
        assert_eq!(
            kinds(
                Grammar::JavaScript,
                "function f(xs, ys) { for (const x of xs) { if (ys.includes(x)) {} } }",
                "f"
            ),
            vec![ListMembershipInLoop]
        );
        assert_eq!(
            kinds(
                Grammar::Rust,
                "fn f(xs: &[i32]) { let v = vec![1, 2]; for x in xs { if v.contains(x) {} } }",
                "f"
            ),
            vec![ListMembershipInLoop]
        );
    }

    #[test]
    fn concat_in_loop() {
        assert_eq!(
            kinds(
                Grammar::Python,
                "def f(xs):\n    s = ''\n    for x in xs:\n        s += f'{x},'\n    return s\n",
                "f"
            ),
            vec![ConcatInLoop]
        );
        assert_eq!(
            kinds(
                Grammar::JavaScript,
                "function f(xs) { let s = ''; for (const x of xs) { s += `${x},`; } return s; }",
                "f"
            ),
            vec![ConcatInLoop]
        );
        assert!(kinds(
            Grammar::Python,
            "def g(xs):\n    n = 0\n    for x in xs:\n        n += 1\n    return n\n",
            "g"
        )
        .is_empty());
    }

    #[test]
    fn sort_in_loop() {
        assert_eq!(
            kinds(
                Grammar::Python,
                "def f(rows):\n    for r in rows:\n        r.sort()\n",
                "f"
            ),
            vec![SortInLoop]
        );
        assert_eq!(c_kinds("void g(int **r, int n, int m) { for (int i = 0; i < n; i++) qsort(r[i], m, 4, cmp); }", "g"), vec![SortInLoop]);
    }

    #[test]
    fn same_collection_nested_loops() {
        assert_eq!(
            kinds(
                Grammar::Python,
                "def f(xs):\n    for x in xs:\n        for y in xs:\n            pass\n",
                "f"
            ),
            vec![SameCollectionNestedLoops]
        );
        assert!(kinds(
            Grammar::Python,
            "def g(xs, ys):\n    for x in xs:\n        for y in ys:\n            pass\n",
            "g"
        )
        .is_empty());
    }

    #[test]
    fn unmemoized_multi_recursion() {
        assert_eq!(kinds(Grammar::Python, "def fib(n):\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n", "fib"), vec![UnmemoizedMultiRecursion]);
    }

    #[test]
    fn memo_fib_has_no_unmemoized_finding() {
        assert!(kinds(Grammar::Python, "@lru_cache(maxsize=None)\ndef fib(n):\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n", "fib").is_empty());
        assert!(c_kinds("long m(int n, long *memo) { if (memo[n]) return memo[n]; return memo[n] = m(n - 1, memo) + m(n - 2, memo); }", "m").is_empty());
    }
}
