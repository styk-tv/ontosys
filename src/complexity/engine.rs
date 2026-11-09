//! # The engine
//!
//! One explicit-stack walk of a function body computes its structure (Tier
//! A) and records every call and loop site with the loops around it, for
//! Tiers B and C. The walk knows no grammar: a [`Profile`] answers every
//! grammar question.

use super::Grammar;
use tree_sitter::Node;

/// How a nested function relates to the function being analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// Not a function.
    None,
    /// A named function: its own declaration, not counted in its parent.
    Named,
    /// A closure, lambda, arrow or function expression: counted inside.
    Anonymous,
}

/// A node's cognitive-complexity role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cog {
    None,
    /// +1 plus the nesting level; raises nesting for its children except its
    /// alternative (`else`), which stays at this level.
    Nesting,
    /// `else if` / `elif`: +1 flat; otherwise like `Nesting`.
    ElseIf,
    /// `else`: +1 flat; raises nesting for its children.
    Flat,
    /// `goto`, labelled `break`/`continue`: +1 flat, nothing nested.
    FlatLeaf,
    /// Anonymous function: no increment; raises nesting.
    NestsOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CallKind {
    Direct,
    Method,
    Indirect,
    Operator,
    Macro,
}

/// A node that may leave the code around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    None,
    Return,
    /// `break`; a label means it can leave enclosing loops too.
    Break(Option<String>),
    Goto(String),
    Throw,
    /// May leave the function on any path (Rust `?`).
    Maybe,
}

/// Grammar knowledge the engine asks for. Implemented once per grammar.
pub trait Profile: Sync {
    fn grammar(&self) -> Grammar;
    /// Decision points this node adds to the cyclomatic number.
    fn decisions(&self, n: Node, src: &[u8]) -> usize;
    fn is_loop(&self, n: Node) -> bool;
    /// The loop's condition field (runs every iteration).
    fn loop_condition_field(&self, n: Node) -> Option<&'static str>;
    /// The loop's field evaluated once, before iterating (C `for` initializer,
    /// the iterable of a `for … in`).
    fn loop_once_field(&self, n: Node) -> Option<&'static str>;
    /// Whether the child in `field` of `parent` runs only on some paths.
    fn is_conditional_child(&self, parent: Node, field: Option<&str>, src: &[u8]) -> bool;
    /// `parent` is the node's parent in the walk (`Node::parent` re-walks
    /// the tree from the root, which is quadratic on deep bodies).
    fn cognitive(&self, n: Node, parent: Option<Node>, src: &[u8]) -> Cog;
    /// The operator of a boolean-operator node (`&&`, `or`, `??`).
    fn bool_op(&self, n: Node, src: &[u8]) -> Option<String>;
    fn boundary(&self, n: Node) -> Boundary;
    /// `(kind, callee, receiver, arguments node)` for a call node.
    fn call<'t>(
        &self,
        n: Node<'t>,
        src: &[u8],
    ) -> Option<(CallKind, String, Option<String>, Option<Node<'t>>)>;
    fn is_statement(&self, n: Node) -> bool;
    fn exit(&self, n: Node, src: &[u8]) -> Exit;
    /// A construct a plain `break` leaves instead of the loop (C/JS `switch`).
    fn is_switch(&self, n: Node) -> bool;
    /// The label a labelled statement defines (goto targets).
    fn label(&self, n: Node, src: &[u8]) -> Option<String>;
    /// First byte of the definition, including attached decorators/attributes.
    fn definition_start(&self, def: Node) -> usize;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    pub kind: CallKind,
    pub callee: String,
    pub receiver: Option<String>,
    pub args: Vec<String>,
    pub line: usize,
    pub start_byte: usize,
    pub args_range: (usize, usize),
    /// Indices into [`TierA::loops`], outermost first.
    pub loop_path: Vec<u32>,
    pub conditional: bool,
    /// The loop whose condition this call is in.
    pub in_condition_of: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopSite {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub depth: u32,
    pub parent: Option<u32>,
    pub early_exit: bool,
    pub conditional: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TierA {
    pub statements: usize,
    pub cyclomatic: usize,
    pub cognitive: usize,
    pub max_nesting: u32,
    pub loop_count: usize,
    pub max_loop_nesting: u32,
    pub self_calls: usize,
    pub error_nodes: usize,
    /// Lines of `goto`s whose label comes before them.
    pub backward_gotos: Vec<usize>,
    pub calls: Vec<CallSite>,
    pub loops: Vec<LoopSite>,
    /// A cache guards the recursion: a cache decorator, or a check reading a
    /// memo/cache before the first self-call. Set by [`detect_memo`].
    pub memoized: bool,
}

/// Whether a recursive function is memoised: a cache decorator above it
/// (`@lru_cache`, `@cache`, `@functools.cache`), or an `if` mentioning a
/// memo or cache between the start of its body and its first self-call.
pub fn detect_memo(
    def: Node,
    body: Node,
    src: &[u8],
    name: &str,
    a: &TierA,
    profile: &dyn Profile,
) -> bool {
    let head_start = profile.definition_start(def).min(body.start_byte());
    let head = String::from_utf8_lossy(&src[head_start..body.start_byte()]);
    if ["lru_cache", "@cache", "functools.cache"]
        .iter()
        .any(|d| head.contains(d))
    {
        return true;
    }
    let Some(first) = a
        .calls
        .iter()
        .filter(|c| c.callee == name)
        .map(|c| c.start_byte)
        .min()
    else {
        return false;
    };
    let prefix = String::from_utf8_lossy(&src[body.start_byte()..first.max(body.start_byte())])
        .to_lowercase();
    prefix
        .lines()
        .any(|l| l.contains("if") && (l.contains("memo") || l.contains("cache")))
}

/// What a plain `break` at this point leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Breakable {
    Nothing,
    Loop(u32),
    Switch,
}

struct Frame<'t> {
    node: Node<'t>,
    parent: Option<Node<'t>>,
    /// Cognitive-complexity nesting level.
    nesting: u32,
    /// Enclosing control structures (anonymous functions excluded).
    depth: u32,
    loop_path: Vec<u32>,
    in_branch: bool,
    /// Anonymous functions entered.
    anon: u32,
    /// Index into `loop_path` from which exits still leave loops (a `return`
    /// inside a closure leaves the closure, not the loops around it).
    exit_scope: usize,
    cond_of: Option<u32>,
    breakable: Breakable,
    parent_bool: Option<String>,
}

/// Analyse one function body. `name` identifies self-calls.
pub fn analyze(body: Node, src: &[u8], name: &str, profile: &dyn Profile) -> TierA {
    use super::profiles::{children_with_fields, short_text};
    let mut a = TierA::default();
    let mut labels: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut gotos: Vec<(String, usize, usize)> = Vec::new();
    let mut first_conditional_exit: Option<usize> = None;
    let mut cyclomatic = 1usize;
    let mut stack = vec![Frame {
        node: body,
        parent: None,
        nesting: 0,
        depth: 0,
        loop_path: Vec::new(),
        in_branch: false,
        anon: 0,
        exit_scope: 0,
        cond_of: None,
        breakable: Breakable::Nothing,
        parent_bool: None,
    }];
    while let Some(f) = stack.pop() {
        let n = f.node;
        if n.is_error() || n.is_missing() {
            a.error_nodes += 1;
        }
        let kind = n.kind();
        if kind.contains("comment") {
            continue;
        }
        if f.parent.is_some() && profile.boundary(n) == Boundary::Named {
            continue;
        }
        a.max_nesting = a.max_nesting.max(f.depth);
        let conditional = f.in_branch || first_conditional_exit.is_some_and(|x| n.start_byte() > x);

        cyclomatic += profile.decisions(n, src);
        if profile.is_statement(n) {
            a.statements += 1;
        }
        let bool_op = profile.bool_op(n, src);
        if let Some(op) = &bool_op {
            if f.parent_bool.as_ref() != Some(op) {
                a.cognitive += 1;
            }
        }
        let anonymous = profile.boundary(n) == Boundary::Anonymous;
        let cog = if anonymous {
            Cog::NestsOnly
        } else {
            profile.cognitive(n, f.parent, src)
        };
        match cog {
            Cog::Nesting => a.cognitive += 1 + f.nesting as usize,
            Cog::ElseIf | Cog::Flat | Cog::FlatLeaf => a.cognitive += 1,
            Cog::NestsOnly | Cog::None => {}
        }

        match profile.exit(n, src) {
            Exit::None => {}
            Exit::Break(None) => {
                if let Breakable::Loop(i) = f.breakable {
                    a.loops[i as usize].early_exit = true;
                }
            }
            exit => {
                for &i in &f.loop_path[f.exit_scope..] {
                    a.loops[i as usize].early_exit = true;
                }
                if let Exit::Goto(label) = &exit {
                    if f.anon == 0 {
                        gotos.push((label.clone(), n.start_byte(), n.start_position().row + 1));
                    }
                }
                let leaves_function = matches!(exit, Exit::Return | Exit::Throw | Exit::Goto(_));
                let conditional_exit = exit == Exit::Maybe || (leaves_function && f.in_branch);
                if conditional_exit && f.anon == 0 && first_conditional_exit.is_none() {
                    first_conditional_exit = Some(n.start_byte());
                }
            }
        }
        if let Some(l) = profile.label(n, src) {
            labels.entry(l).or_insert(n.start_byte());
        }

        if let Some((kind, callee, receiver, args)) = profile.call(n, src) {
            let is_self = callee == name
                && (kind == CallKind::Direct
                    || (kind == CallKind::Method
                        && matches!(receiver.as_deref(), Some("self" | "this" | "Self"))));
            if is_self && f.anon == 0 {
                a.self_calls += 1;
            }
            let (arg_texts, args_range) = match args {
                Some(args) => {
                    let mut c = args.walk();
                    let texts = args
                        .named_children(&mut c)
                        .filter(|x| !x.kind().contains("comment"))
                        .map(|x| short_text(x, src))
                        .collect();
                    (texts, (args.start_byte(), args.end_byte()))
                }
                None => (Vec::new(), (n.end_byte(), n.end_byte())),
            };
            a.calls.push(CallSite {
                kind,
                callee,
                receiver,
                args: arg_texts,
                line: n.start_position().row + 1,
                start_byte: n.start_byte(),
                args_range,
                loop_path: f.loop_path.clone(),
                conditional,
                in_condition_of: f.cond_of,
            });
        }

        let is_loop = profile.is_loop(n);
        let mut this_loop = None;
        if is_loop {
            let idx = a.loops.len() as u32;
            let depth = f.loop_path.len() as u32 + 1;
            a.max_loop_nesting = a.max_loop_nesting.max(depth);
            a.loops.push(LoopSite {
                line: n.start_position().row + 1,
                start_byte: n.start_byte(),
                end_byte: n.end_byte(),
                depth,
                parent: f.loop_path.last().copied(),
                early_exit: false,
                conditional,
            });
            this_loop = Some(idx);
        }
        let once = if is_loop {
            profile.loop_once_field(n)
        } else {
            None
        };
        let cond_field = if is_loop {
            profile.loop_condition_field(n)
        } else {
            None
        };
        let is_switch = profile.is_switch(n);

        let kids = children_with_fields(n);
        for (child, field) in kids.into_iter().rev() {
            let alternative = field == Some("alternative");
            let (nesting, depth) = match cog {
                Cog::Nesting | Cog::ElseIf if alternative => (f.nesting, f.depth),
                Cog::Nesting | Cog::ElseIf | Cog::Flat => (f.nesting + 1, f.depth + 1),
                Cog::NestsOnly => (f.nesting + 1, f.depth),
                Cog::FlatLeaf | Cog::None => (f.nesting, f.depth),
            };
            let mut loop_path = f.loop_path.clone();
            let mut cond_of = f.cond_of;
            let mut breakable = f.breakable;
            if let Some(idx) = this_loop {
                if once.is_none() || field != once {
                    loop_path.push(idx);
                    breakable = Breakable::Loop(idx);
                }
                cond_of = if cond_field.is_some() && field == cond_field {
                    Some(idx)
                } else {
                    None
                };
            } else if is_switch {
                breakable = Breakable::Switch;
            }
            let (anon, exit_scope) = if anonymous {
                breakable = Breakable::Nothing;
                (f.anon + 1, loop_path.len())
            } else {
                (f.anon, f.exit_scope)
            };
            stack.push(Frame {
                node: child,
                parent: Some(n),
                nesting,
                depth,
                in_branch: f.in_branch || profile.is_conditional_child(n, field, src),
                loop_path,
                anon,
                exit_scope,
                cond_of,
                breakable,
                parent_bool: bool_op.clone(),
            });
        }
    }
    a.cyclomatic = cyclomatic;
    a.loop_count = a.loops.len();
    if a.self_calls > 0 {
        a.cognitive += 1;
    }
    for (label, at, line) in gotos {
        if labels.get(&label).is_some_and(|&l| l < at) {
            a.backward_gotos.push(line);
        }
    }
    a.backward_gotos.sort_unstable();
    a
}

/// sha256 (lowercase hex) of the definition's bytes as they are in the file.
pub fn definition_sha256(def: Node, original: &[u8], profile: &dyn Profile) -> String {
    use sha2::{Digest, Sha256};
    let start = profile.definition_start(def).min(def.end_byte());
    let end = def.end_byte().min(original.len());
    Sha256::digest(&original[start.min(end)..end])
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tree_sitter::Parser;

    /// Parse `src` as C and analyse its first function definition.
    fn c(src: &str) -> TierA {
        with_c_function(src, |def, body, name, b| {
            analyze(body, b, name, Grammar::C.profile().unwrap())
        })
    }

    fn with_c_function<T>(src: &str, f: impl FnOnce(Node, Node, &str, &[u8]) -> T) -> T {
        let mut p = Parser::new();
        p.set_language(&Grammar::C.language().unwrap()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let root = tree.root_node();
        let mut c = root.walk();
        let def = root
            .named_children(&mut c)
            .find(|n| n.kind() == "function_definition")
            .expect("a function");
        let body = def.child_by_field_name("body").unwrap();
        let mut d = def.child_by_field_name("declarator").unwrap();
        while d.kind() != "function_declarator" {
            d = d.child_by_field_name("declarator").unwrap();
        }
        let name_node = d.child_by_field_name("declarator").unwrap();
        let name = name_node.utf8_text(src.as_bytes()).unwrap().to_string();
        f(def, body, &name, src.as_bytes())
    }

    const SUM_OF_PRIMES: &str = "int sum_of_primes(int max) {
    int total = 0;
    for (int i = 1; i <= max; ++i) {
        for (int j = 2; j < i; ++j) {
            if (i % j == 0) {
                goto next;
            }
        }
        total += i;
    next:;
    }
    return total;
}
";

    #[test]
    fn sum_of_primes_c() {
        let a = c(SUM_OF_PRIMES);
        assert_eq!((a.cyclomatic, a.cognitive, a.max_nesting), (4, 7, 3));
        assert_eq!((a.loop_count, a.max_loop_nesting, a.self_calls), (2, 2, 0));
        assert!(a.backward_gotos.is_empty(), "the label follows the goto");
    }

    #[test]
    fn get_words_switch_c() {
        let a = c("const char *get_words(int number) {
    switch (number) {
    case 1: return \"one\";
    case 2: return \"a couple\";
    case 3: return \"a few\";
    default: return \"lots\";
    }
}
");
        assert_eq!((a.cyclomatic, a.cognitive, a.max_nesting), (4, 1, 1));
    }

    #[test]
    fn boolean_sequences_c() {
        let a =
            c("int f(int a, int b, int c, int d) { if (a && b && c || d) return 1; return 0; }");
        assert_eq!((a.cyclomatic, a.cognitive), (5, 3));
    }

    #[test]
    fn else_if_chain_c() {
        let a = c(
            "int sign(int x) { if (x > 0) return 1; else if (x < 0) return -1; else return 0; }",
        );
        assert_eq!((a.cyclomatic, a.cognitive, a.max_nesting), (3, 3, 1));
    }

    #[test]
    fn direct_recursion_c() {
        let a = c("int fact(int n) { return n <= 1 ? 1 : n * fact(n - 1); }");
        assert_eq!((a.cyclomatic, a.cognitive, a.self_calls), (2, 2, 1));
        assert_eq!(a.calls.len(), 1);
        assert_eq!(a.calls[0].callee, "fact");
        assert_eq!(a.calls[0].args, vec!["n - 1".to_string()]);
        assert_eq!(a.calls[0].kind, CallKind::Direct);
        assert!(a.calls[0].conditional, "one arm of ?:");
    }

    #[test]
    fn call_sites_carry_loop_paths_c() {
        let a = c("void g(int *a, int n) {
    for (int i = 0; i < n; i++) {
        while (h(a[i])) {
            k(i);
        }
    }
    m();
}
");
        let site = |name: &str| a.calls.iter().find(|c| c.callee == name).unwrap().clone();
        assert_eq!(site("h").loop_path, vec![0, 1]);
        assert_eq!(site("h").in_condition_of, Some(1));
        assert_eq!(site("k").loop_path, vec![0, 1]);
        assert_eq!(site("k").in_condition_of, None);
        assert!(site("m").loop_path.is_empty());
        assert_eq!(a.loops.len(), 2);
        assert_eq!(a.loops[1].parent, Some(0));
        assert_eq!((a.loops[0].depth, a.loops[1].depth), (1, 2));
        assert_eq!(site("k").line, 4);
    }

    #[test]
    fn a_for_initializer_runs_once_outside_its_loop() {
        let a = c("void g(int n) { for (int i = start(); i < n; i = next(i)) { body(i); } }");
        let site = |name: &str| a.calls.iter().find(|c| c.callee == name).unwrap().clone();
        assert!(site("start").loop_path.is_empty());
        assert_eq!(site("next").loop_path, vec![0]);
        assert_eq!(site("body").loop_path, vec![0]);
    }

    #[test]
    fn conditional_and_early_exit_c() {
        let a = c("void f(int *a, int n, int x) { if (x) return; for (int i = 0; i < n; i++) { if (a[i]) break; } }");
        assert!(a.loops[0].conditional, "after a conditional return");
        assert!(a.loops[0].early_exit);
        let b = c("int s(int *a, int n) { int t = 0; for (int i = 0; i < n; i++) { switch (a[i]) { case 0: break; default: t++; } } return t; }");
        assert!(!b.loops[0].conditional);
        assert!(
            !b.loops[0].early_exit,
            "a break inside a switch leaves the switch, not the loop"
        );
    }

    #[test]
    fn backward_goto_is_found_c() {
        let a = c("void f(int x) {\nagain:\n    x++;\n    if (x < 3)\n        goto again;\n}\n");
        assert_eq!(a.backward_gotos, vec![5]);
    }

    #[test]
    fn function_pointer_call_is_indirect_c() {
        let a = c("void f(void (*fp)(int), struct s *s) { (*fp)(1); s->cb(2); g(3); }");
        let kinds: Vec<CallKind> = a.calls.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![CallKind::Indirect, CallKind::Indirect, CallKind::Direct]
        );
    }

    #[test]
    fn c_counts_through_nested_definitions() {
        // Standard C has no nested functions; a function_definition inside a
        // body is GNU C or parse recovery, and the C layer always counted it.
        let a = c("void f(void) { int g(int x) { if (x) return 1; return 0; } if (1) g(1); }");
        assert_eq!(a.cyclomatic, 3);
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let depth = 5000;
        let mut src = String::from("void deep(int x) {\n");
        for _ in 0..depth {
            src.push_str("if (x) {\n");
        }
        src.push_str("x++;\n");
        for _ in 0..depth {
            src.push_str("}\n");
        }
        src.push_str("}\n");
        let t = std::time::Instant::now();
        let a = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || c(&src))
            .unwrap()
            .join()
            .expect("no stack overflow on a small stack");
        assert_eq!(a.max_nesting, depth);
        assert_eq!(a.cyclomatic, depth as usize + 1);
        assert!(t.elapsed().as_secs_f64() < 2.0, "linear: {:?}", t.elapsed());
    }

    #[test]
    fn definition_span_c() {
        let src = "/* doc */\nstatic int add(int a, int b)\r\n{\r\n    return a + b;\r\n}\r\n";
        let sha = with_c_function(src, |def, _, _, b| {
            definition_sha256(def, b, Grammar::C.profile().unwrap())
        });
        let start = src.find("static").unwrap();
        let end = src.rfind('}').unwrap() + 1;
        let expected = hex::encode(Sha256::digest(&src.as_bytes()[start..end]));
        assert_eq!(sha, expected);
    }

    mod hex {
        pub fn encode(bytes: impl AsRef<[u8]>) -> String {
            bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
        }
    }
}
