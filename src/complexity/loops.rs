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
    /// The repository's overlay states a function's or macro's cost.
    OverlayCost,
}

/// How many times a loop's contents run at least, on every path: what its
/// lower bound may count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum MinTrips {
    /// Possibly never.
    Zero,
    /// At least once.
    One,
    /// As many times as the bound says.
    Bound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopFact {
    pub site: LoopSite,
    pub class: BoundClass,
    /// The bound as written (at most 200 characters).
    pub bound_expression: String,
    pub controls: Vec<String>,
    pub assumptions: Vec<Assumption>,
    pub min_trips: MinTrips,
}

/// Classify the loops the engine found (`sites`) in the tree under `root`.
pub fn classify_all(root: Node, src: &[u8], grammar: Grammar, sites: &[LoopSite]) -> Vec<LoopFact> {
    let profile = grammar.profile();
    sites
        .iter()
        .map(|s| {
            let c = match loop_node(root, s, profile) {
                Some(n) => classify(n, src),
                None => Classified::unknown(String::new()),
            };
            let (class, bound_expression, controls, assumptions, min_trips) = c.finish();
            LoopFact {
                site: s.clone(),
                class,
                bound_expression,
                controls,
                assumptions,
                min_trips,
            }
        })
        .collect()
}

/// The class of the first loop in a C function `source` (used for a loop
/// macro, re-read from its own expanded header and the body at its use).
pub fn classify_first_c_loop(source: &str) -> Option<LoopClass> {
    let mut p = tree_sitter::Parser::new();
    p.set_language(&Grammar::C.language()?).ok()?;
    let tree = p.parse(source, None)?;
    let profile = Grammar::C.profile()?;
    let mut stack = vec![tree.root_node()];
    let mut first: Option<Node> = None;
    while let Some(n) = stack.pop() {
        if profile.is_loop(n) {
            first = Some(n);
            break;
        }
        let mut c = n.walk();
        let kids: Vec<Node> = n.children(&mut c).collect();
        stack.extend(kids.into_iter().rev());
    }
    // A part the parser could not read (a type as a macro argument) may sit
    // in the initializer; the condition, update and body must be clean.
    let first = first?;
    let clean = ["condition", "update", "body"]
        .iter()
        .all(|f| first.child_by_field_name(f).is_none_or(|x| !x.has_error()));
    if first.is_error() || first.is_missing() || !clean {
        return None;
    }
    Some(classify(first, source.as_bytes()).finish())
}

/// A loop's classification without its site.
pub type LoopClass = (BoundClass, String, Vec<String>, Vec<Assumption>, MinTrips);

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
    min: MinTrips,
}

impl Classified {
    fn of(class: BoundClass, bound: String, controls: Vec<String>) -> Self {
        let min = match class {
            BoundClass::Unknown => MinTrips::Zero,
            BoundClass::Constant | BoundClass::Unbounded => MinTrips::One,
            _ => MinTrips::Bound,
        };
        Classified {
            class,
            bound,
            controls,
            constant_name: false,
            min,
        }
    }
    fn unknown(bound: String) -> Self {
        Self::of(BoundClass::Unknown, bound, Vec::new())
    }
    fn min(mut self, m: MinTrips) -> Self {
        self.min = m;
        self
    }
    fn finish(self) -> LoopClass {
        let mut assumptions = Vec::new();
        if self.constant_name {
            assumptions.push(Assumption::ConstantName);
        }
        if !matches!(self.class, BoundClass::Unbounded | BoundClass::Unknown) {
            assumptions.push(Assumption::LoopShape);
        }
        let min = if self.class == BoundClass::Unknown {
            MinTrips::Zero
        } else {
            self.min
        };
        (
            self.class,
            clip(&self.bound),
            self.controls,
            assumptions,
            min,
        )
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

/// The integer a literal spells (`10`, `0x1f`, `10usize`, `-1`), if it is one.
fn literal_value(s: &str) -> Option<i128> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r.trim_start()),
        None => (false, s),
    };
    let (radix, digits) = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => (16, h),
        None => (10, s),
    };
    let end = digits
        .find(|c: char| !(c.is_digit(radix) || c == '_'))
        .unwrap_or(digits.len());
    let (num, suffix) = digits.split_at(end);
    if num.is_empty() || !suffix.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if radix == 10 && suffix.chars().any(|c| matches!(c, 'e' | 'E')) {
        return None;
    }
    let v = i128::from_str_radix(&num.replace('_', ""), radix).ok()?;
    Some(if neg { -v } else { v })
}

fn is_all_caps(s: &str) -> bool {
    s.len() >= 2
        && s.starts_with(|c: char| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Whether an expression is a compile-time constant as a whole: literals,
/// `sizeof`, operators, and ALL_CAPS names (`Some(true)` when it rests on
/// one). Any other name, call or member access makes it a runtime value.
fn const_kind(n: Node, src: &[u8]) -> Option<bool> {
    let mut caps = false;
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        match x.kind() {
            "sizeof_expression" | "alignof_expression" | "offsetof_expression" => continue,
            "call_expression"
            | "call"
            | "macro_invocation"
            | "field_expression"
            | "member_expression"
            | "attribute"
            | "subscript_expression"
            | "subscript"
            | "index_expression"
            | "self"
            | "this" => return None,
            "identifier" | "field_identifier" | "property_identifier" => {
                if is_all_caps(txt(x, src)) {
                    caps = true;
                } else {
                    return None;
                }
            }
            _ => {}
        }
        let mut c = x.walk();
        stack.extend(x.children(&mut c));
    }
    Some(caps)
}

const SENTINELS: &[&str] = &[
    "NULL",
    "null",
    "None",
    "nullptr",
    "undefined",
    "'\\0'",
    "0",
    "nil",
    "NIL",
];

/// Methods that take an element off their receiver (a sentinel loop's advance).
const SHRINKING: &[&str] = &[
    "pop",
    "popleft",
    "popitem",
    "pop_front",
    "pop_back",
    "pop_first",
    "pop_last",
    "shift",
    "next",
    "dequeue",
    "poll",
    "remove",
];

/// Methods that add to their receiver.
const GROWING: &[&str] = &[
    "append",
    "appendleft",
    "extend",
    "extendleft",
    "insert",
    "push",
    "push_back",
    "push_front",
    "push_str",
    "add",
    "unshift",
    "splice",
    "enqueue",
    "offer",
    "extend_from_slice",
];

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
    let mut c = classify_inner(n, src);
    if n.kind() == "do_statement" && c.min == MinTrips::Zero && c.class != BoundClass::Unknown {
        c.min = MinTrips::One;
    }
    // A generator expression runs only as far as its consumer reads.
    if n.kind() == "for_in_clause"
        && n.parent()
            .is_some_and(|p| p.kind() == "generator_expression")
    {
        c.min = MinTrips::Zero;
    }
    c
}

fn classify_inner(n: Node, src: &[u8]) -> Classified {
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
    // `do { … } while (0)` runs its body once, `while (0)` never.
    if matches!(cond_text, "0" | "false" | "False") {
        return Classified::of(Constant, cond_text.to_string(), Vec::new()).min(MinTrips::Zero);
    }
    let mut scopes: Vec<Node> = body.into_iter().collect();
    if let Some(update) = n
        .child_by_field_name("update")
        .or_else(|| n.child_by_field_name("increment"))
    {
        scopes.push(update);
    }
    scopes.push(cond);
    let facts = Facts::of(&scopes, src);
    let cond = as_truth(cond, src);
    let cond_text = txt(cond, src).trim();
    if matches!(cond.kind(), "let_condition" | "let_chain") {
        let mut names = Vec::new();
        let mut stack = vec![cond];
        while let Some(x) = stack.pop() {
            if x.kind() == "let_condition" {
                if let Some(v) = x.child_by_field_name("value") {
                    names.extend(sentinel_names(v, src));
                }
            } else {
                let mut c = x.walk();
                stack.extend(x.children(&mut c));
            }
        }
        return sentinel(names, &facts, cond_text);
    }
    // `A && B` ends no later than any conjunct that bounds the loop alone;
    // it may end sooner, so its contents need not run.
    let conjuncts = conjuncts(cond, src);
    if conjuncts.len() > 1 {
        let best = conjuncts
            .iter()
            .map(|c| classify_condition(*c, n, &facts, src))
            .filter(|c| !matches!(c.class, BoundClass::Unknown | BoundClass::Unbounded))
            .min_by_key(|c| match c.class {
                Constant => 0,
                Halving => 1,
                Size => 2,
                _ => 3,
            });
        return match best {
            Some(c) => c.min(MinTrips::Zero),
            None => Classified::unknown(cond_text.to_string()),
        };
    }
    classify_condition(cond, n, &facts, src)
}

/// One loop condition: a comparison, or a value the loop runs while.
fn classify_condition(cond: Node, n: Node, facts: &Facts, src: &[u8]) -> Classified {
    let cond = as_truth(unparen(cond), src);
    let cond_text = txt(cond, src).trim();
    match comparison(cond, src) {
        Some((op, l, r)) => classify_comparison(&op, l, r, n, cond_text, facts, src),
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
                || matches!(
                    cond.kind(),
                    "binary_expression"
                        | "comparison_operator"
                        | "conditional_expression"
                        | "ternary_expression"
                )
            {
                Classified::unknown(cond_text.to_string())
            } else {
                sentinel(sentinel_names(cond, src), facts, cond_text)
            }
        }
    }
}

/// The operands of a `&&`/`and` chain (one element when it is not one).
fn conjuncts<'t>(cond: Node<'t>, src: &[u8]) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut stack = vec![cond];
    while let Some(x) = stack.pop() {
        let x = unparen(x);
        let op = x.child_by_field_name("operator").map(|o| txt(o, src));
        if matches!(x.kind(), "binary_expression" | "boolean_operator")
            && matches!(op, Some("&&" | "and"))
        {
            if let (Some(l), Some(r)) = (
                x.child_by_field_name("left"),
                x.child_by_field_name("right"),
            ) {
                stack.push(r);
                stack.push(l);
                continue;
            }
        }
        out.push(x);
    }
    out
}

/// `c ? (…, true) : (…, false)` is as true as `c`: the condition it tests.
fn as_truth<'t>(cond: Node<'t>, src: &[u8]) -> Node<'t> {
    if !matches!(cond.kind(), "conditional_expression" | "ternary_expression") {
        return cond;
    }
    let last = |n: Option<Node>| -> String {
        let Some(mut n) = n.map(unparen) else {
            return String::new();
        };
        while n.kind() == "comma_expression" {
            match n.child_by_field_name("right") {
                Some(r) => n = unparen(r),
                None => break,
            }
        }
        txt(n, src).trim().to_string()
    };
    let yes = last(cond.child_by_field_name("consequence"));
    let no = last(cond.child_by_field_name("alternative"));
    match cond.child_by_field_name("condition") {
        Some(c) if matches!(yes.as_str(), "1" | "true") && matches!(no.as_str(), "0" | "false") => {
            unparen(c)
        }
        _ => cond,
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

/// How an assignment moves its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    StepUp,
    StepDown,
    /// `/= 2`, `>>= 1`, `v = v / 2`.
    HalveDown,
    /// `*= 2`, `<<= 1`, `v = v * 2`.
    DoubleUp,
    /// `v = mid ± k` for a midpoint `mid` (binary search).
    Mid,
    Other,
}

struct Assign {
    target: String,
    change: Change,
    /// Right-hand side, without whitespace (empty for `++`/`--`).
    rhs: String,
}

/// What a loop's body (with its condition and update) does to names.
struct Facts {
    assigns: Vec<Assign>,
    /// `(receiver, method)` of every method call.
    calls: Vec<(String, String)>,
}

impl Facts {
    fn of(scopes: &[Node], src: &[u8]) -> Facts {
        Facts {
            assigns: assignments(scopes, src),
            calls: method_calls(scopes, src),
        }
    }
    fn assigned(&self, x: &str) -> bool {
        self.assigns.iter().any(|a| a.target == x)
    }
    fn grows(&self, x: &str) -> bool {
        self.calls
            .iter()
            .any(|(r, m)| r == x && GROWING.contains(&m.as_str()))
    }
}

fn method_calls(scopes: &[Node], src: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for &scope in scopes {
        let mut stack = vec![scope];
        while let Some(x) = stack.pop() {
            if matches!(x.kind(), "call" | "call_expression") {
                if let Some(f) = x.child_by_field_name("function") {
                    let recv = f
                        .child_by_field_name("object")
                        .or_else(|| f.child_by_field_name("value"));
                    let name = f
                        .child_by_field_name("attribute")
                        .or_else(|| f.child_by_field_name("property"))
                        .or_else(|| f.child_by_field_name("field"));
                    if let (Some(r), Some(m)) = (recv, name) {
                        out.push((squash(r, src), txt(m, src).to_string()));
                    }
                }
            }
            let mut c = x.walk();
            stack.extend(x.children(&mut c));
        }
    }
    out
}

/// Every assignment in `scopes`, each judged by how it moves its target.
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
                        let op = if txt(n, src).contains("--") {
                            "--"
                        } else {
                            "++"
                        };
                        raw.push((squash(a, src), op.into(), String::new()));
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
    let positive = |x: &str| literal_value(x).is_some_and(|v| v > 0);
    raw.iter()
        .map(|(t, op, rhs)| {
            let change = match op.as_str() {
                "++" => Change::StepUp,
                "--" => Change::StepDown,
                "+=" if positive(rhs) => Change::StepUp,
                "-=" if positive(rhs) => Change::StepDown,
                "/=" | "//=" if rhs == "2" => Change::HalveDown,
                ">>=" if rhs == "1" => Change::HalveDown,
                "*=" if rhs == "2" => Change::DoubleUp,
                "<<=" if rhs == "1" => Change::DoubleUp,
                "=" => {
                    let after = |p: String| {
                        rhs.strip_prefix(p.as_str())
                            .filter(|r| positive(r))
                            .is_some()
                    };
                    if after(format!("{t}+")) {
                        Change::StepUp
                    } else if after(format!("{t}-")) {
                        Change::StepDown
                    } else if [format!("{t}/2"), format!("{t}>>1"), format!("{t}//2")].contains(rhs)
                    {
                        Change::HalveDown
                    } else if [format!("{t}*2"), format!("{t}<<1")].contains(rhs) {
                        Change::DoubleUp
                    } else if mids.iter().any(|m| {
                        rhs == m
                            || [format!("{m}+"), format!("{m}-")]
                                .iter()
                                .any(|p| rhs.strip_prefix(p.as_str()).is_some_and(positive))
                    }) {
                        Change::Mid
                    } else {
                        Change::Other
                    }
                }
                _ => Change::Other,
            };
            Assign {
                target: t.clone(),
                change,
                rhs: rhs.clone(),
            }
        })
        .collect()
}

/// The names an expression reads: identifiers, and whole member accesses
/// (`p->next`, `self.queue`) along with their base; not called names,
/// fields alone, or ALL_CAPS constants.
fn sentinel_names(n: Node, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        match x.kind() {
            "identifier" => {
                let t = txt(x, src);
                if !is_all_caps(t) && !SENTINELS.contains(&t) {
                    out.push(t.to_string());
                }
            }
            "field_expression" | "member_expression" | "attribute" => {
                out.push(squash(x, src));
                if let Some(base) = x
                    .child_by_field_name("argument")
                    .or_else(|| x.child_by_field_name("object"))
                    .or_else(|| x.child_by_field_name("value"))
                {
                    stack.push(base);
                }
            }
            "call" | "call_expression" => {
                if let Some(f) = x.child_by_field_name("function") {
                    // a method's receiver is read; a called name is not
                    if let Some(r) = f
                        .child_by_field_name("object")
                        .or_else(|| f.child_by_field_name("value"))
                    {
                        stack.push(r);
                    }
                }
                if let Some(a) = x.child_by_field_name("arguments") {
                    stack.push(a);
                }
            }
            _ => {
                let mut c = x.walk();
                stack.extend(x.children(&mut c));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Whether `rhs` (whitespace removed) reads a field or an element of `name`
/// (`p->next`, `node.left`, `xs[1:]`): a step along a structure. A call
/// such as `step(p)` is not one; what it returns is not known.
fn traverses(rhs: &str, name: &str) -> bool {
    ["->", ".", "["].iter().any(|op| {
        let pat = format!("{name}{op}");
        let mut from = 0;
        while let Some(i) = rhs[from..].find(&pat) {
            let at = from + i;
            let before = rhs[..at].chars().next_back();
            if !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '>') {
                return true;
            }
            from = at + pat.len();
        }
        false
    })
}

/// A loop that runs while `names` hold a value: bounded by the data only if
/// the body advances one of them (`p = p->next`, `s++`, `stack.pop()`), and
/// never resets or grows any of them.
fn sentinel(names: Vec<String>, f: &Facts, cond_text: &str) -> Classified {
    if names.is_empty() {
        return Classified::unknown(cond_text.to_string());
    }
    let mut advanced = Vec::new();
    for name in &names {
        if f.grows(name) {
            return Classified::unknown(cond_text.to_string());
        }
        for a in f.assigns.iter().filter(|a| &a.target == name) {
            let advances = match a.change {
                Change::StepUp | Change::StepDown | Change::HalveDown | Change::DoubleUp => true,
                _ => traverses(&a.rhs, name),
            };
            if !advances {
                return Classified::unknown(cond_text.to_string());
            }
            advanced.push(name.clone());
        }
        if f.calls
            .iter()
            .any(|(r, m)| r == name && SHRINKING.contains(&m.as_str()))
        {
            advanced.push(name.clone());
        }
    }
    if advanced.is_empty() {
        return Classified::unknown(cond_text.to_string());
    }
    advanced.sort();
    advanced.dedup();
    Classified::of(BoundClass::Sentinel, cond_text.to_string(), advanced)
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
        if matches!(
            x.kind(),
            "field_expression" | "member_expression" | "attribute"
        ) {
            out.push(squash(x, src));
        }
        let mut c = x.walk();
        stack.extend(x.children(&mut c));
    }
    out
}

/// The value a statement before the loop (or its initializer) gives
/// `control`: `int i = n`, `let mut i = n;`, `lo, hi = 0, n`. `None` when
/// the start is not visible or set in some nested way.
fn find_start<'t>(control: &str, n: Node<'t>, src: &[u8]) -> Option<Node<'t>> {
    if let Some(init) = n.child_by_field_name("initializer") {
        if let Some(v) = value_given(control, init, src) {
            return v;
        }
    }
    let mut at = n;
    while at.prev_named_sibling().is_none() {
        match at.parent() {
            Some(p) if p.kind() == "expression_statement" => at = p,
            _ => return None,
        }
    }
    let mut prev = at.prev_named_sibling();
    while let Some(p) = prev {
        if let Some(v) = value_given(control, p, src) {
            return v;
        }
        prev = p.prev_named_sibling();
    }
    None
}

/// `Some(Some(value))` when `stmt` sets `control` plainly, `Some(None)` when
/// it changes it some other way, `None` when it leaves it alone.
fn value_given<'t>(control: &str, stmt: Node<'t>, src: &[u8]) -> Option<Option<Node<'t>>> {
    let mut found: Option<Option<Node<'t>>> = None;
    let mut stack = vec![stmt];
    while let Some(x) = stack.pop() {
        let f = |name: &str| x.child_by_field_name(name);
        let pair = match x.kind() {
            "init_declarator" | "variable_declarator" => {
                f("declarator").or_else(|| f("name")).zip(f("value"))
            }
            "let_declaration" => f("pattern").zip(f("value")),
            "assignment_expression" | "assignment" => f("left").zip(f("right")),
            "update_expression"
            | "augmented_assignment_expression"
            | "compound_assignment_expr"
            | "augmented_assignment" => {
                let target = f("argument").or_else(|| f("left"));
                if target.is_some_and(|t| squash(t, src) == control) {
                    return Some(None);
                }
                None
            }
            _ => None,
        };
        if let Some((target, value)) = pair {
            let target_text = squash(target, src);
            let target_text = target_text.strip_prefix("mut").unwrap_or(&target_text);
            if target_text == control {
                found = Some(Some(value));
            } else if matches!(target.kind(), "pattern_list" | "tuple_pattern" | "tuple") {
                let mut c = target.walk();
                let parts: Vec<String> = target
                    .named_children(&mut c)
                    .map(|p| {
                        let t = squash(p, src);
                        t.strip_prefix("mut").map(str::to_string).unwrap_or(t)
                    })
                    .collect();
                if let Some(k) = parts.iter().position(|p| p == control) {
                    let mut c = value.walk();
                    let vals: Vec<Node> = value.named_children(&mut c).collect();
                    let tuple_like = matches!(
                        value.kind(),
                        "expression_list" | "tuple_expression" | "tuple"
                    );
                    found = Some(if tuple_like && vals.len() == parts.len() {
                        Some(vals[k])
                    } else {
                        None
                    });
                }
            }
        }
        let mut c = x.walk();
        stack.extend(x.children(&mut c));
    }
    found
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
}

/// Where a counted loop starts.
#[derive(Clone, Copy)]
enum Start<'t> {
    At(Node<'t>),
    /// `range(n)`: from the constant 0.
    Zero,
    /// Not visible (a parameter, or set in some nested way): named by the control.
    Unseen,
}

/// A counted loop from `start` towards `bound` in direction `dir`.
fn counted(
    dir: Dir,
    start: Start,
    start_name: &str,
    bound: Node,
    inclusive: bool,
    src: &[u8],
    controls: Vec<String>,
) -> Classified {
    use BoundClass::*;
    let b_text = txt(bound, src).trim().to_string();
    let (s_text, s_const, s_value) = match start {
        Start::At(s) => (
            txt(s, src).trim().to_string(),
            const_kind(s, src),
            literal_value(txt(s, src)),
        ),
        Start::Zero => ("0".to_string(), Some(false), Some(0)),
        Start::Unseen => (start_name.to_string(), None, None),
    };
    let b_const = const_kind(bound, src);
    match (s_const, b_const) {
        (Some(sc), Some(bc)) => {
            let values = s_value.zip(literal_value(&b_text));
            let runs = values.is_some_and(|(s, b)| match (dir, inclusive) {
                (Dir::Up, false) => s < b,
                (Dir::Up, true) => s <= b,
                (Dir::Down, false) => s > b,
                (Dir::Down, true) => s >= b,
            });
            Classified {
                constant_name: sc || bc,
                ..Classified::of(Constant, b_text, controls)
            }
            .min(if runs { MinTrips::One } else { MinTrips::Zero })
        }
        // From a constant towards a size: as many runs as the size.
        (Some(_), None) if dir == Dir::Up => Classified::of(Size, b_text, controls),
        // From a size down to a constant: as many runs as the start.
        (None, Some(_)) if dir == Dir::Down => Classified::of(Size, s_text, controls),
        // From a size towards a constant away from it: grows with the start
        // (as it moves the other way), and may not run at all.
        (None, Some(_)) => Classified::of(Size, s_text, controls).min(MinTrips::Zero),
        (Some(_), None) => Classified::of(Size, b_text, controls).min(MinTrips::Zero),
        (None, None) => {
            let size = match dir {
                Dir::Up => format!("{b_text} - {s_text}"),
                Dir::Down => format!("{s_text} - {b_text}"),
            };
            Classified::of(Size, size, controls).min(MinTrips::Zero)
        }
    }
}

fn classify_comparison(
    op: &str,
    l: Node,
    r: Node,
    loop_node: Node,
    cond_text: &str,
    f: &Facts,
    src: &[u8],
) -> Classified {
    let (lt, rt) = (squash(l, src), squash(r, src));
    if op.starts_with("!=")
        && (SENTINELS.contains(&lt.as_str()) || SENTINELS.contains(&rt.as_str()))
    {
        let side = if SENTINELS.contains(&lt.as_str()) {
            r
        } else {
            l
        };
        return sentinel(sentinel_names(side, src), f, cond_text);
    }
    let mut controls = Vec::new();
    if f.assigned(&lt) {
        controls.push(lt.clone());
    }
    if f.assigned(&rt) {
        controls.push(rt.clone());
    }
    if controls.is_empty() {
        return Classified::unknown(cond_text.to_string());
    }
    let changes: Vec<Change> = f
        .assigns
        .iter()
        .filter(|a| controls.contains(&a.target))
        .map(|a| a.change)
        .collect();
    if controls.len() > 1 {
        // Binary search: both ends move to a midpoint.
        return if changes
            .iter()
            .all(|c| matches!(c, Change::Mid | Change::HalveDown))
        {
            Classified::of(BoundClass::Halving, cond_text.to_string(), controls)
        } else {
            Classified::unknown(cond_text.to_string())
        };
    }
    let left = controls[0] == lt;
    let bound = if left { r } else { l };
    // `c != end` with `c = c->next`: a walk along a structure to its end.
    if op.starts_with("!=")
        && f.assigns
            .iter()
            .filter(|a| a.target == controls[0])
            .all(|a| a.change == Change::Other && traverses(&a.rhs, &controls[0]))
        && !identifiers(bound, src)
            .iter()
            .any(|i| f.assigned(i) || f.grows(i))
    {
        return Classified::of(BoundClass::Sentinel, cond_text.to_string(), controls)
            .min(MinTrips::Zero);
    }
    if changes
        .iter()
        .any(|c| matches!(c, Change::Other | Change::Mid))
    {
        return Classified::unknown(cond_text.to_string());
    }
    if identifiers(bound, src)
        .iter()
        .any(|i| f.assigned(i) || f.grows(i))
    {
        return Classified::unknown(cond_text.to_string());
    }
    let up = changes
        .iter()
        .any(|c| matches!(c, Change::StepUp | Change::DoubleUp));
    let down = changes
        .iter()
        .any(|c| matches!(c, Change::StepDown | Change::HalveDown));
    if up == down {
        return Classified::unknown(cond_text.to_string());
    }
    let dir = if up { Dir::Up } else { Dir::Down };
    // The direction that moves the control towards the bound.
    let needed = match (op, left) {
        ("<" | "<=", true) | (">" | ">=", false) => Some(Dir::Up),
        (">" | ">=", true) | ("<" | "<=", false) => Some(Dir::Down),
        _ => None,
    };
    if needed.is_some_and(|d| d != dir) {
        return Classified::unknown(cond_text.to_string());
    }
    let start = match find_start(&controls[0], loop_node, src) {
        Some(s) => Start::At(s),
        None => Start::Unseen,
    };
    if changes
        .iter()
        .all(|c| matches!(c, Change::HalveDown | Change::DoubleUp))
    {
        let anchored = match dir {
            Dir::Up => matches!(start, Start::At(s) if const_kind(s, src).is_some()),
            Dir::Down => const_kind(bound, src).is_some(),
        };
        return Classified::of(BoundClass::Halving, cond_text.to_string(), controls).min(
            if anchored {
                MinTrips::Bound
            } else {
                MinTrips::Zero
            },
        );
    }
    if changes
        .iter()
        .any(|c| matches!(c, Change::HalveDown | Change::DoubleUp))
    {
        return Classified::unknown(cond_text.to_string());
    }
    let inclusive = op.ends_with('=') && !op.starts_with('!');
    let name = controls[0].clone();
    counted(dir, start, &name, unparen(bound), inclusive, src, controls)
}

fn classify_iterable(
    iterable: Node,
    body: Option<Node>,
    src: &[u8],
    controls: Vec<String>,
) -> Classified {
    let it = unparen(iterable);
    let it_text = squash(it, src);
    if let Some(body) = body {
        let calls = method_calls(&[body], src);
        if calls
            .iter()
            .any(|(r, m)| *r == it_text && GROWING.contains(&m.as_str()))
        {
            return Classified::unknown(txt(it, src).to_string());
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
                1 => counted(Dir::Up, Start::Zero, "", named[0], false, src, controls),
                2 => counted(
                    Dir::Up,
                    Start::At(named[0]),
                    "",
                    named[1],
                    false,
                    src,
                    controls,
                ),
                3 => match literal_value(&squash(named[2], src)) {
                    Some(s) if s > 0 => counted(
                        Dir::Up,
                        Start::At(named[0]),
                        "",
                        named[1],
                        false,
                        src,
                        controls,
                    ),
                    Some(s) if s < 0 => counted(
                        Dir::Down,
                        Start::At(named[0]),
                        "",
                        named[1],
                        false,
                        src,
                        controls,
                    ),
                    _ => Classified::unknown(txt(it, src).to_string()),
                },
                _ => Classified::unknown(txt(it, src).to_string()),
            }
        }
        "range_expression" => {
            let mut c = it.walk();
            let named: Vec<Node> = it.named_children(&mut c).collect();
            let starts_open = txt(it, src).trim_start().starts_with("..");
            let inclusive = txt(it, src).contains("..=");
            match (named.len(), starts_open) {
                (2, _) => counted(
                    Dir::Up,
                    Start::At(named[0]),
                    "",
                    unparen(named[1]),
                    inclusive,
                    src,
                    controls,
                ),
                (1, true) => counted(
                    Dir::Up,
                    Start::Unseen,
                    "?",
                    unparen(named[0]),
                    inclusive,
                    src,
                    controls,
                ),
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

    fn trips(g: Grammar, src: &str, name: &str) -> Vec<(BoundClass, MinTrips)> {
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
                    .map(|f| (f.class, f.min_trips))
                    .collect();
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
        panic!("no function {name}");
    }

    /// Final review C1: the start value and the step direction count.
    #[test]
    fn counting_down_and_wrong_directions() {
        use MinTrips::*;
        let src = "void f(int n, int k, int len, int *a) {
    for (int i = n; i > 0; i--) {}
    while (len >= 4) { len -= 4; }
    for (int i = 0; i < n; i--) {}
    for (int i = 0; i < n; i++) { if (a[i]) { i--; } }
    for (int i = k; i < n; i++) {}
    for (int j = k; j < 10; j++) {}
    for (int i = 0; i < 10; i++) {}
    for (int i = 10; i > 0; i--) {}
    for (int i = 5; i < 3; i++) {}
    for (int i = 0; i != n; i++) {}
    for (int i = 1; i < n; i *= 2) {}
    do { n--; } while (0);
    while (0) { n++; }
}
";
        assert_eq!(
            trips(Grammar::C, src, "f"),
            vec![
                (Size, Bound),
                (Size, Bound),
                (Unknown, Zero),
                (Unknown, Zero),
                (Size, Zero),
                (Size, Zero),
                (Constant, One),
                (Constant, One),
                (Constant, Zero),
                (Size, Bound),
                (Halving, Bound),
                (Constant, One),
                (Constant, Zero),
            ]
        );
        let got = classes(Grammar::C, src, "f");
        assert_eq!(got[0].1, "n", "counting down from n is linear in n");
        assert_eq!(got[1].1, "len");
        assert_eq!(got[4].1, "n - k");
        assert_eq!(got[5].1, "k");
        let py = "def f(n, k):\n    for i in range(n, 0, -1):\n        pass\n    for i in range(k, n):\n        pass\n    for i in range(0, n, 2):\n        pass\n    for i in range(0, n, k):\n        pass\n";
        assert_eq!(
            trips(Grammar::Python, py, "f"),
            vec![(Size, Bound), (Size, Zero), (Size, Bound), (Unknown, Zero)]
        );
        assert_eq!(classes(Grammar::Python, py, "f")[0].1, "n");
        let rs = "fn f(n: usize, k: usize) { let mut i = n; while i > 0 { i -= 1; } for j in k..n {} for j in 0..=n {} }";
        assert_eq!(
            trips(Grammar::Rust, rs, "f"),
            vec![(Size, Bound), (Size, Zero), (Size, Bound)]
        );
        let js = "function f(a, n) { for (let i = n - 1; i >= 0; i--) {} }";
        assert_eq!(trips(Grammar::JavaScript, js, "f"), vec![(Size, Bound)]);
        assert_eq!(classes(Grammar::JavaScript, js, "f")[0].1, "n - 1");
    }

    /// Final review C2: a bound is constant only as a whole.
    #[test]
    fn constant_bounds_are_whole_expressions() {
        let src = "void f(int n, char *p, int x) {
    for (int i = 0; i < 2 * n; i++) {}
    for (int i = 0; i < sizeof(int) * n; i++) {}
    for (int i = 0; i < sizeof(x); i++) {}
    for (int i = 0; i < 4 * 8; i++) {}
    for (int i = 0; i < 2 * MAXN; i++) {}
}
";
        assert_eq!(
            only(classes(Grammar::C, src, "f")),
            vec![Size, Size, Constant, Constant, Constant]
        );
        let py = "def f(n):\n    for i in range(2 * n):\n        pass\n";
        assert_eq!(only(classes(Grammar::Python, py, "f")), vec![Size]);
        let rs = "fn f(n: usize) { for i in 0..2 * n {} }";
        assert_eq!(only(classes(Grammar::Rust, rs, "f")), vec![Size]);
    }

    /// Final review C3: sentinel loops are shape-checked; a bound whose
    /// receiver grows in the body is data-dependent.
    #[test]
    fn sentinels_must_advance_and_bounds_must_not_grow() {
        let src = "void f(struct node *p, int *a, int n, int running, const char *s) {
    while (p) { p = p->next; }
    while (p != NULL) { p = p->left; }
    while (*s) { s++; }
    while (running) { n++; }
    int changed = 1; while (changed) { changed = 0; }
    while (p) { p = head; }
}
";
        assert_eq!(
            only(classes(Grammar::C, src, "f")),
            vec![Sentinel, Sentinel, Sentinel, Unknown, Unknown, Unknown]
        );
        let py = "def f(stack, q, node):\n    while stack:\n        stack.pop()\n    while q:\n        x = q.pop()\n        q.append(x)\n    i = 0\n    while i < len(q):\n        q.append(i)\n        i += 1\n    while node:\n        node = node.next\n";
        assert_eq!(
            only(classes(Grammar::Python, py, "f")),
            vec![Sentinel, Unknown, Unknown, Sentinel]
        );
        let js = "function f(queue, node) { for (let i = 0; i < queue.length; i++) { queue.push(i); } while (node) { node = node.next; } }";
        assert_eq!(
            only(classes(Grammar::JavaScript, js, "f")),
            vec![Unknown, Sentinel]
        );
        let rs = "fn f(mut st: Vec<i32>, mut it: I, rx: R) { while let Some(x) = st.pop() { st.push(x); } while let Some(x) = it.next() {} while let Ok(m) = rx.recv() {} }";
        assert_eq!(
            only(classes(Grammar::Rust, rs, "f")),
            vec![Unknown, Sentinel, Unknown]
        );
    }

    /// Final review C4 and C9: a generator expression may stop early.
    #[test]
    fn generator_expressions_may_stop_early() {
        let py = "def f(xs):\n    a = [x for x in xs]\n    b = any(x for x in xs)\n";
        assert_eq!(
            trips(Grammar::Python, py, "f"),
            vec![(Size, MinTrips::Bound), (Size, MinTrips::Zero)]
        );
    }

    /// A conjunction is bounded by its counted conjunct (upper bound only);
    /// a ternary that yields true or false is its condition; a walk to an
    /// end marker by field access is a sentinel; an opaque `p = step(p)` is
    /// not an advance.
    #[test]
    fn conjunctions_ternaries_and_walks() {
        use MinTrips::*;
        let src = "void f(int n, struct node *p, struct node *end, int x) {
    for (int i = 0; p != NULL && i < n; i++) {}
    for (int i = 0; (i < n) ? (x = 1, 1) : (x = 0, 0); i++) {}
    for (int i = 0; (p != NULL && i < n) ? (x = 1, true) : (x = 0, false); i++) {}
    while (p != end) { p = p->next; }
    while (p) { p = step(p); }
    for (int i = 0; p != NULL || i < n; i++) {}
}
";
        assert_eq!(
            trips(Grammar::C, src, "f"),
            vec![
                (Size, Zero),
                (Size, Bound),
                (Size, Zero),
                (Sentinel, Zero),
                (Unknown, Zero),
                (Unknown, Zero),
            ]
        );
    }
}
