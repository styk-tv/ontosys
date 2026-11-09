//! # Tier C
//!
//! A growth estimate per function: an upper bound only when nothing in the
//! body is unknown, a lower bound counting only work on every path, every
//! reason an upper bound is missing, and every assumption a bound rests on.

use super::catalog::{Catalog, SizeArg};
use super::cost::Cost;
use super::engine::{CallKind, CallSite, TierA};
use super::loops::{Assumption, BoundClass, LoopFact, MinTrips};
use super::model::{Model, Target};
use super::Grammar;
use crate::ontology::Iri;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum Basis {
    ExactShape,
    Partial,
    Unknown,
}

/// Why an upper bound is missing (spec §3.6; closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum Reason {
    UncostedCall,
    IndirectCall,
    AmbiguousCall,
    CyclicCall,
    BlockedCallee,
    DataDependentLoop,
    UnboundedLoop,
    UnstructuredFlow,
    Unparsed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    pub reason: Reason,
    pub line: usize,
    pub text: String,
    /// For `BlockedCallee`: the callee's model index.
    pub callee: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estimate {
    pub upper: Option<Cost>,
    pub lower: Cost,
    pub size_expressions: BTreeSet<String>,
    pub basis: Basis,
    pub blockers: Vec<Blocker>,
    pub assumptions: BTreeMap<Assumption, BTreeSet<String>>,
    pub definition_count: usize,
}

/// A C macro definition, for deriving its cost from its body.
#[derive(Debug, Clone)]
pub struct MacroBody {
    pub iri: Iri,
    pub name: String,
    pub params: Vec<String>,
    pub text: String,
}

/// Every macro of the C files, by IRI.
pub fn macro_bodies(project: &str, files: &[crate::lang_c::CFile]) -> Vec<MacroBody> {
    let mut out = Vec::new();
    for f in files {
        for m in &f.macros {
            let params = m
                .params
                .as_deref()
                .map(|p| {
                    p.trim_matches(|c| c == '(' || c == ')')
                        .split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty() && x != "...")
                        .collect()
                })
                .unwrap_or_default();
            out.push(MacroBody {
                iri: crate::ontology::instance_iri(
                    project,
                    "macro",
                    &format!("{}/{}", f.path, m.name),
                ),
                name: m.name.clone(),
                params,
                text: m.value.clone(),
            });
        }
    }
    out
}

/// A macro body parsed as if it were a function `void __m(void) { BODY; }`.
struct ParsedMacro {
    tier_a: TierA,
    loops: Vec<LoopFact>,
    params: Vec<String>,
    name: String,
}

fn parse_macro(mb: &MacroBody) -> Option<ParsedMacro> {
    use tree_sitter::Parser;
    // `#x` (stringify) and `a##b` (paste) are preprocessor syntax, not C.
    // A backslash before a line break (kept by the extractor, possibly with
    // the break collapsed to a space) only continues the definition.
    let mut body = String::with_capacity(mb.text.len());
    let mut chars = mb.text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_none_or(|n| n.is_whitespace()) {
            body.push(' ');
        } else {
            body.push(c);
        }
    }
    let body = body.replace("##", "").replace('#', "");
    let src = format!("void __m(void) {{ {} ; }}\n", body);
    let mut p = Parser::new();
    p.set_language(&Grammar::C.language()?).ok()?;
    let tree = p.parse(&src, None)?;
    let def = tree.root_node().named_child(0)?;
    let body = def.child_by_field_name("body")?;
    let tier_a = super::engine::analyze(body, src.as_bytes(), "__m", Grammar::C.profile()?);
    let loops = super::loops::classify_all(body, src.as_bytes(), Grammar::C, &tier_a.loops);
    Some(ParsedMacro {
        tier_a,
        loops,
        params: mb.params.clone(),
        name: mb.name.clone(),
    })
}

fn clip(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

fn min_cost(a: Cost, b: Cost) -> Cost {
    if a.max(b) == a {
        b
    } else {
        a
    }
}

/// What a call costs, as seen from its caller.
#[derive(Default)]
struct Callee {
    upper: Option<Cost>,
    lower: Cost,
    sizes: Vec<String>,
    blockers: Vec<Blocker>,
    assumptions: Vec<(Assumption, String)>,
}

impl Default for Cost {
    fn default() -> Self {
        Cost::ONE
    }
}

/// A macro's derived upper and lower bounds, sizes and assumptions.
type MacroCost = (Cost, Cost, Vec<String>, Vec<(Assumption, String)>);

struct Env<'a> {
    m: &'a Model,
    cat: &'a Catalog,
    est: Vec<Option<Estimate>>,
    component: Vec<usize>,
    macros_by_name: HashMap<String, Vec<String>>,
    parsed: HashMap<String, Option<ParsedMacro>>,
    macro_cost: HashMap<String, Option<MacroCost>>,
    in_progress: HashSet<String>,
    c_by_name: HashMap<String, Vec<usize>>,
    /// Every function by (language family, name), for callbacks passed by name.
    by_name: HashMap<(u8, String), Vec<usize>>,
}

/// A point a later jump can re-enter: code after it may run again.
const REENTRY_CALLS: &[&str] = &["setjmp", "sigsetjmp", "_setjmp"];
/// A jump away: it only ends the current execution early, like `throw`.
const JUMP_AWAY_CALLS: &[&str] = &["longjmp", "siglongjmp", "_longjmp"];

/// A function value as written in an argument (`cmp`, `&f`, `self.f`,
/// `Self::f`): the name it refers to, if it is one.
fn function_reference(arg: &str) -> Option<&str> {
    let t = arg.trim().trim_start_matches(['&', '*']).trim();
    let t = ["self.", "this.", "Self::", "self::"]
        .iter()
        .find_map(|p| t.strip_prefix(p))
        .unwrap_or(t);
    let ident = !t.is_empty()
        && t.starts_with(|c: char| c.is_alphabetic() || c == '_')
        && t.chars().all(|c| c.is_alphanumeric() || c == '_');
    ident.then_some(t)
}

/// Whether an argument is a function written in place (lambda, closure, arrow).
fn is_inline_function(arg: &str) -> bool {
    let t = arg.trim();
    t.starts_with("lambda")
        || t.starts_with('|')
        || t.starts_with("move |")
        || t.contains("=>")
        || t.starts_with("function")
}

impl<'a> Env<'a> {
    /// Targets for the calls of a parsed macro body.
    fn macro_targets(&self, pm: &ParsedMacro) -> Vec<Target> {
        pm.tier_a
            .calls
            .iter()
            .map(|c| {
                if c.kind != CallKind::Direct || pm.params.contains(&c.callee) {
                    return Target::Indirect;
                }
                if let Some(v) = self.macros_by_name.get(&c.callee) {
                    return Target::Macro(v.iter().map(|i| Iri::new(i.as_str())).collect());
                }
                match self.c_by_name.get(&c.callee).map(Vec::as_slice) {
                    Some([j]) => Target::Repo(*j),
                    Some(v) if !v.is_empty() => Target::Ambiguous(v.to_vec()),
                    _ => Target::External(c.callee.clone()),
                }
            })
            .collect()
    }

    /// Upper bound, lower bound and sizes of a macro, derived from its body.
    fn macro_cost(&mut self, iri: &str) -> Option<MacroCost> {
        if let Some(c) = self.macro_cost.get(iri) {
            return c.clone();
        }
        if !self.in_progress.insert(iri.to_string()) {
            return None; // macros defined through each other
        }
        let parsed = self.parsed.remove(iri).flatten();
        let result = parsed.as_ref().and_then(|pm| {
            let targets = self.macro_targets(pm);
            let e = self.estimate_body(
                None,
                &pm.name,
                Grammar::C,
                &pm.tier_a,
                &pm.loops,
                &targets,
                false,
            );
            let assumptions = e
                .assumptions
                .keys()
                .map(|k| (*k, format!("via macro {}", pm.name)))
                .collect();
            e.upper.map(|u| {
                (
                    u,
                    e.lower,
                    e.size_expressions.into_iter().collect(),
                    assumptions,
                )
            })
        });
        self.parsed.insert(iri.to_string(), parsed);
        self.in_progress.remove(iri);
        if result.is_none() {
            tracing::debug!(
                macro_iri = iri,
                "no cost derived for macro (unparsable, or its body is blocked)"
            );
        }
        self.macro_cost.insert(iri.to_string(), result.clone());
        result
    }

    /// Whether every definition of model function `j` is a stub.
    fn is_stub(&self, j: usize) -> bool {
        self.m.fns[j].defs.iter().all(|d| d.stub)
    }

    /// A generator or async function: calling it runs none of its body.
    fn is_deferred(&self, j: usize) -> bool {
        self.m.fns[j].defs.iter().any(|d| d.tier_a.deferred)
    }

    /// What one repository function costs its caller.
    fn repo_cost(&self, me: Option<usize>, c: &CallSite, j: usize, out: &mut Callee) {
        let block = |out: &mut Callee, reason: Reason, text: String, callee: Option<usize>| {
            out.blockers.push(Blocker {
                reason,
                line: c.line,
                text: clip(&text),
                callee,
            });
        };
        if self.is_stub(j) {
            block(
                out,
                Reason::UncostedCall,
                format!("{} (abstract or stub)", self.m.fns[j].name),
                None,
            );
            return;
        }
        if me.is_some_and(|i| self.component[i] == self.component[j]) {
            block(
                out,
                Reason::CyclicCall,
                format!("{} (same call cycle)", c.callee),
                None,
            );
            return;
        }
        match self.est[j].as_ref() {
            Some(e) if e.upper.is_some() => {
                out.upper = e.upper;
                out.lower = if self.is_deferred(j) {
                    Cost::ONE
                } else {
                    e.lower
                };
                let callee = &self.m.fns[j].name;
                out.sizes = callee_sizes(callee, e.size_expressions.iter());
                out.assumptions
                    .extend(e.assumptions.keys().map(|k| (*k, format!("via {callee}"))));
            }
            _ => block(
                out,
                Reason::BlockedCallee,
                self.m.fns[j].name.clone(),
                Some(j),
            ),
        }
    }

    fn callee_cost(
        &mut self,
        me: Option<usize>,
        grammar: Grammar,
        c: &CallSite,
        t: &Target,
    ) -> Callee {
        let mut out = Callee::default();
        let block = |out: &mut Callee, reason: Reason, text: String, callee: Option<usize>| {
            out.blockers.push(Blocker {
                reason,
                line: c.line,
                text: clip(&text),
                callee,
            });
        };
        if REENTRY_CALLS.contains(&c.callee.as_str()) {
            block(&mut out, Reason::UnstructuredFlow, c.callee.clone(), None);
            return out;
        }
        if JUMP_AWAY_CALLS.contains(&c.callee.as_str()) {
            out.upper = Some(Cost::ONE);
            return out;
        }
        // The repository's overlay states its own functions and macros.
        if !matches!(t, Target::External(_) | Target::Indirect)
            && self
                .cat
                .lookup(grammar, &c.callee)
                .iter()
                .any(|e| e.overlay)
        {
            out.upper = Some(Cost::ONE);
            self.join_catalog(&mut out, grammar, c, false, true);
            return out;
        }
        let catalog_applies = matches!(c.kind, CallKind::Method | CallKind::Operator);
        match t {
            Target::Repo(j) => {
                self.repo_cost(me, c, *j, &mut out);
                if catalog_applies && out.upper.is_some() {
                    self.join_catalog(&mut out, grammar, c, true, false);
                }
            }
            Target::Macro(iris) => {
                let mut upper = Some(Cost::ONE);
                let mut lower: Option<Cost> = None;
                for iri in iris {
                    match self.macro_cost(iri.as_str()) {
                        Some((u, l, sizes, assumptions)) => {
                            upper = upper.map(|x| x.max(u));
                            lower = Some(lower.map_or(l, |x| min_cost(x, l)));
                            out.sizes.extend(callee_sizes(&c.callee, sizes.iter()));
                            out.assumptions.extend(assumptions);
                        }
                        None => upper = None,
                    }
                }
                match upper {
                    Some(u) => {
                        out.upper = Some(u);
                        out.lower = lower.unwrap_or(Cost::ONE);
                    }
                    None => block(
                        &mut out,
                        Reason::UncostedCall,
                        format!("macro {}", c.callee),
                        None,
                    ),
                }
            }
            Target::External(name) => {
                // A capitalised Rust call is an enum variant or tuple struct.
                if grammar == Grammar::Rust
                    && c.kind == CallKind::Direct
                    && name.starts_with(|ch: char| ch.is_ascii_uppercase())
                    && !name.contains("::")
                {
                    out.upper = Some(Cost::ONE);
                    return out;
                }
                out.upper = Some(Cost::ONE);
                if !self.join_catalog(&mut out, grammar, c, false, false) {
                    out.upper = None;
                    // A method on a receiver of unknown type: the callee is
                    // not known (§5), not merely uncatalogued.
                    let reason = if catalog_applies {
                        Reason::IndirectCall
                    } else {
                        Reason::UncostedCall
                    };
                    block(&mut out, reason, name.clone(), None);
                }
            }
            Target::Ambiguous(v) => {
                let mut each = Vec::with_capacity(v.len());
                for &j in v {
                    let mut one = Callee::default();
                    self.repo_cost(me, c, j, &mut one);
                    each.push(one);
                }
                if each.iter().all(|x| x.upper.is_some()) {
                    out.upper = Some(
                        each.iter()
                            .filter_map(|x| x.upper)
                            .fold(Cost::ONE, Cost::max),
                    );
                    out.lower = each.iter().map(|x| x.lower).fold(each[0].lower, min_cost);
                    out.assumptions.push((
                        Assumption::BuiltinReceiver,
                        format!("{}: one of {} same-named definitions", c.callee, v.len()),
                    ));
                    // A method may as well be the built-in one of that name.
                    if catalog_applies {
                        self.join_catalog(&mut out, grammar, c, true, false);
                    }
                } else {
                    block(
                        &mut out,
                        Reason::AmbiguousCall,
                        format!("{} ({} definitions)", c.callee, v.len()),
                        None,
                    );
                }
            }
            Target::Unattached(name) => block(
                &mut out,
                Reason::UncostedCall,
                format!("{name} (definition not attached)"),
                None,
            ),
            Target::Indirect => block(&mut out, Reason::IndirectCall, c.callee.clone(), None),
        }
        out
    }

    /// Raise `out.upper` to the worst catalog entry for the call's name
    /// (only the repository's own entries when `overlay_only`). Returns
    /// whether any entry exists.
    fn join_catalog(
        &self,
        out: &mut Callee,
        grammar: Grammar,
        c: &CallSite,
        also_repo: bool,
        overlay_only: bool,
    ) -> bool {
        let entries: Vec<_> = self
            .cat
            .lookup(grammar, &c.callee)
            .into_iter()
            .filter(|e| !overlay_only || e.overlay)
            .collect();
        if entries.is_empty() {
            return false;
        }
        let mut worst = Cost::ONE;
        for e in &entries {
            let mut sizes: Vec<String> = e
                .size_args
                .iter()
                .filter_map(|s| match s {
                    SizeArg::Arg(k) => c.args.get(*k).cloned(),
                    SizeArg::Receiver => c.receiver.clone(),
                })
                .collect();
            if sizes.is_empty() && e.output {
                sizes.push(format!("output of {}", c.callee));
            }
            let mut cost = e.worst;
            if !sizes.is_empty() && cost != Cost::ONE {
                let caps: Vec<String> = sizes
                    .iter()
                    .filter_map(|sz| capped(sz, &self.cat.caps))
                    .collect();
                if caps.len() == sizes.len() {
                    for cap in caps {
                        out.assumptions.push((Assumption::OverlayCap, cap));
                    }
                    cost = Cost::ONE;
                } else {
                    out.sizes.extend(sizes.iter().map(|s| clip(s)));
                }
            }
            if e.overlay {
                out.assumptions.push((
                    Assumption::OverlayCost,
                    clip(&format!("{}: {}", c.callee, e.source)),
                ));
            }
            worst = worst.max(cost);
            if e.average.is_some() || e.amortized.is_some() {
                let which = if e.amortized.is_some() {
                    "amortized"
                } else {
                    "average"
                };
                let cheaper = e
                    .amortized
                    .or(e.average)
                    .map(|x| format!("{:?}", x.class().0))
                    .unwrap_or_default();
                out.assumptions.push((
                    Assumption::CatalogCase,
                    format!("{}: worst case used; {which} is {cheaper}", c.callee),
                ));
            }
        }
        if entries.iter().any(|e| e.receiver.is_some())
            && (c.kind == CallKind::Method || c.kind == CallKind::Operator || also_repo)
        {
            let recv = c.receiver.clone().unwrap_or_default();
            out.assumptions.push((
                Assumption::BuiltinReceiver,
                clip(&format!(
                    "{} on {recv}: a built-in type or a repository class",
                    c.callee
                )),
            ));
        }
        out.upper = out.upper.map(|u| u.max(worst));
        true
    }

    /// The catalog entries that may describe this call: an external name,
    /// or a method or operator whose receiver may be a built-in.
    fn entries_for(
        &self,
        grammar: Grammar,
        c: &CallSite,
        t: &Target,
    ) -> Vec<&'a super::catalog::CostEntry> {
        let applies = matches!(t, Target::External(_))
            || (matches!(c.kind, CallKind::Method | CallKind::Operator)
                && matches!(t, Target::Repo(_) | Target::Ambiguous(_)));
        if applies {
            self.cat.lookup(grammar, &c.callee)
        } else {
            Vec::new()
        }
    }

    /// How many times a higher-order call may run a function it is given:
    /// once per element for an iterating entry, else the entry's worst cost
    /// (a sort's comparisons). `None` when no entry describes the call.
    fn callback_runs(
        &self,
        grammar: Grammar,
        c: &CallSite,
        t: &Target,
    ) -> Option<(Cost, Option<String>)> {
        let entries = self.entries_for(grammar, c, t);
        if entries.is_empty() {
            return None;
        }
        let mut runs = Cost::ONE;
        let mut size = None;
        for e in &entries {
            match &e.iterates {
                Some(SizeArg::Arg(k)) => {
                    runs = runs.max(Cost::n());
                    size = size.or_else(|| c.args.get(*k).cloned());
                }
                Some(SizeArg::Receiver) => {
                    runs = runs.max(Cost::n());
                    size = size.or_else(|| c.receiver.clone());
                }
                None => runs = runs.max(e.worst),
            }
        }
        Some((runs, size))
    }

    /// Functions passed by name to a higher-order call (`qsort(…, cmp)`,
    /// `map(f, xs)`, `xs.map(transform)`): what they cost per run, or why
    /// that is not known. An argument in a slot the catalog names as the
    /// callback that is not a repository function is an indirect call.
    fn passed_functions(
        &mut self,
        me: Option<usize>,
        grammar: Grammar,
        c: &CallSite,
        t: &Target,
    ) -> Vec<Callee> {
        let entries = self.entries_for(grammar, c, t);
        // A macro's arguments are tokens; calls in them are re-read as calls.
        if entries.is_empty() || c.kind == CallKind::Macro {
            return Vec::new();
        }
        let slots: Vec<usize> = entries
            .iter()
            .filter_map(|e| match &e.callback {
                Some(super::catalog::Callback::Arg(k)) => Some(*k),
                Some(super::catalog::Callback::Keyword(kw)) => c
                    .args
                    .iter()
                    .position(|a| a.trim_start().starts_with(&format!("{kw}="))),
                None => None,
            })
            .collect();
        let family = super::catalog::family(grammar);
        let mut out = Vec::new();
        for (k, arg) in c.args.iter().enumerate() {
            let value = arg
                .split_once('=')
                .filter(|(l, _)| function_reference(l).is_some() && !l.contains(['(', '[']))
                .map_or(arg.as_str(), |(_, r)| r);
            if is_inline_function(value) {
                continue;
            }
            let slot = slots.contains(&k);
            let Some(name) = function_reference(value) else {
                if slot {
                    let mut x = Callee::default();
                    x.blockers.push(Blocker {
                        reason: Reason::IndirectCall,
                        line: c.line,
                        text: clip(&format!("{value} passed to {}", c.callee)),
                        callee: None,
                    });
                    out.push(x);
                }
                continue;
            };
            let found = self
                .by_name
                .get(&(family, name.to_string()))
                .cloned()
                .unwrap_or_default();
            let mut x = Callee::default();
            match found.as_slice() {
                [] if slot => x.blockers.push(Blocker {
                    reason: Reason::IndirectCall,
                    line: c.line,
                    text: clip(&format!("{name} passed to {}", c.callee)),
                    callee: None,
                }),
                [] => continue,
                [j] => self.repo_cost(me, c, *j, &mut x),
                many => {
                    let target = Target::Ambiguous(many.to_vec());
                    let site = CallSite {
                        callee: name.to_string(),
                        kind: CallKind::Direct,
                        ..c.clone()
                    };
                    x = self.callee_cost(me, grammar, &site, &target);
                }
            }
            out.push(x);
        }
        out
    }

    /// One definition (or macro body). `stub` marks an abstract method.
    #[allow(clippy::too_many_arguments)]
    fn estimate_body(
        &mut self,
        me: Option<usize>,
        name: &str,
        grammar: Grammar,
        a: &TierA,
        loops: &[LoopFact],
        targets: &[Target],
        stub: bool,
    ) -> Estimate {
        let mut e = Estimate {
            upper: None,
            lower: Cost::ONE,
            size_expressions: BTreeSet::new(),
            basis: Basis::ExactShape,
            blockers: Vec::new(),
            assumptions: BTreeMap::new(),
            definition_count: 1,
        };
        if a.error_nodes > 0 {
            e.basis = Basis::Unknown;
            e.blockers.push(Blocker {
                reason: Reason::Unparsed,
                line: 0,
                text: format!("{} error node(s)", a.error_nodes),
                callee: None,
            });
            return e;
        }
        if stub {
            e.basis = Basis::Unknown;
            e.blockers.push(Blocker {
                reason: Reason::UncostedCall,
                line: 0,
                text: format!("{name} (abstract or stub)"),
                callee: None,
            });
            return e;
        }
        let assume = |e: &mut Estimate, k: Assumption, t: String| {
            e.assumptions.entry(k).or_default().insert(clip(&t));
        };
        // Loop bounds: (upper factor, lower factor when the contents always run).
        let mut bound: Vec<(Option<Cost>, Option<Cost>)> = Vec::with_capacity(loops.len());
        for l in loops {
            let size = || {
                if l.bound_expression.is_empty() {
                    None
                } else {
                    Some(l.bound_expression.clone())
                }
            };
            let (u, lo) = match l.class {
                BoundClass::Constant => (Some(Cost::ONE), Cost::ONE),
                BoundClass::Size | BoundClass::Sentinel => {
                    match capped(&l.bound_expression, &self.cat.caps) {
                        Some(cap) => {
                            assume(&mut e, Assumption::OverlayCap, cap);
                            (Some(Cost::ONE), Cost::ONE)
                        }
                        None => {
                            if let Some(sz) = size() {
                                e.size_expressions.insert(clip(&sz));
                            }
                            (Some(Cost::n()), Cost::n())
                        }
                    }
                }
                BoundClass::Halving => {
                    if let Some(sz) = size() {
                        e.size_expressions.insert(clip(&sz));
                    }
                    (Some(Cost::log_n()), Cost::log_n())
                }
                BoundClass::Unbounded => {
                    e.blockers.push(Blocker {
                        reason: Reason::UnboundedLoop,
                        line: l.site.line,
                        text: "no condition".into(),
                        callee: None,
                    });
                    (None, Cost::ONE)
                }
                BoundClass::Unknown => {
                    e.blockers.push(Blocker {
                        reason: Reason::DataDependentLoop,
                        line: l.site.line,
                        text: clip(&l.bound_expression),
                        callee: None,
                    });
                    (None, Cost::ONE)
                }
            };
            let lo = match l.min_trips {
                MinTrips::Zero => None,
                MinTrips::One => Some(Cost::ONE),
                MinTrips::Bound => Some(lo),
            };
            for k in &l.assumptions {
                let t = match k {
                    Assumption::ConstantName => format!("{} is a constant", l.bound_expression),
                    _ => "each loop runs at most its bound".to_string(),
                };
                assume(&mut e, *k, t);
            }
            bound.push((u, lo));
        }
        // Calls: their costs; functions handed to higher-order calls; and the
        // argument ranges whose inline functions run once per callback.
        let mut callees: Vec<Option<Callee>> = Vec::with_capacity(a.calls.len());
        let mut passed: Vec<Vec<Callee>> = Vec::with_capacity(a.calls.len());
        let mut runs_of: Vec<Option<Cost>> = Vec::with_capacity(a.calls.len());
        let mut callback_ranges: Vec<((usize, usize), Cost)> = Vec::new();
        let mut grows: Vec<bool> = Vec::with_capacity(a.calls.len());
        let mut self_sites: Vec<&CallSite> = Vec::new();
        for (c, t) in a.calls.iter().zip(targets) {
            let is_self = (me.is_some() && matches!(t, Target::Repo(j) if Some(*j) == me))
                || (me.is_none() && c.callee == name);
            if is_self {
                self_sites.push(c);
                callees.push(None);
                passed.push(Vec::new());
                runs_of.push(None);
                grows.push(false);
                continue;
            }
            let runs = self.callback_runs(grammar, c, t);
            if let Some((r, size)) = &runs {
                if *r != Cost::ONE {
                    callback_ranges.push((c.args_range, *r));
                    if let Some(sz) = size {
                        e.size_expressions.insert(clip(sz));
                    }
                }
            }
            runs_of.push(runs.map(|(r, _)| r));
            grows.push(
                self.entries_for(grammar, c, t)
                    .iter()
                    .any(|en| en.grows.is_some()),
            );
            passed.push(self.passed_functions(me, grammar, c, t));
            callees.push(Some(self.callee_cost(me, grammar, c, t)));
        }
        // Loops around an item: (upper factor, lower factor if it runs on every path).
        let ancestors = |k: usize| -> Vec<usize> {
            let mut path = vec![k];
            let mut cur = loops.get(k).and_then(|l| l.site.parent);
            while let Some(p) = cur {
                path.push(p as usize);
                cur = loops.get(p as usize).and_then(|l| l.site.parent);
            }
            path.reverse();
            path
        };
        let factor =
            |path: &[usize], in_anonymous: Option<usize>| -> (Option<Cost>, Option<Cost>) {
                let mut up = Some(Cost::ONE);
                let mut low = Some(Cost::ONE);
                for &k in path {
                    let (u, l) = bound.get(k).copied().unwrap_or((None, None));
                    up = up.and_then(|x| u.map(|u| x.mul(u)));
                    let site = &loops[k].site;
                    low = match l {
                        Some(l) if !site.early_exit && !site.conditional => low.map(|x| x.mul(l)),
                        _ => None,
                    };
                }
                // An inline function handed to a higher-order call runs once per callback.
                if let Some(at) = in_anonymous {
                    for ((s, end), runs) in &callback_ranges {
                        if at > *s && at < *end {
                            up = up.map(|x| x.mul(*runs));
                        }
                    }
                }
                (up, low)
            };
        let mut upper = Cost::ONE;
        let mut lower = Cost::ONE;
        let mut known = true;
        for k in 0..loops.len() {
            let (u, l) = factor(&ancestors(k), None);
            match u {
                Some(u) => upper = upper.max(u),
                None => known = false,
            }
            if let (Some(l), false) = (l, loops[k].site.conditional) {
                lower = lower.max(l);
            }
        }
        for (i, (c, callee)) in a.calls.iter().zip(&callees).enumerate() {
            let Some(callee) = callee else { continue };
            let path: Vec<usize> = c.loop_path.iter().map(|&x| x as usize).collect();
            let (u, l) = factor(&path, c.in_anonymous);
            // A call that appends to its argument inside loops: the argument
            // grows with every iteration, so its size is bounded by the loops'
            // count times what each call adds.
            let u = if grows[i] && !path.is_empty() {
                u.map(|x| x.mul(factor(&path, None).0.unwrap_or(Cost::ONE)))
            } else {
                u
            };
            match (u, callee.upper) {
                (Some(u), Some(cu)) => upper = upper.max(u.mul(cu)),
                _ => known = false,
            }
            if let (Some(l), false) = (l, c.conditional) {
                lower = lower.max(l.mul(callee.lower));
            }
            e.size_expressions.extend(callee.sizes.iter().cloned());
            e.blockers.extend(callee.blockers.iter().cloned());
            for (k, t) in &callee.assumptions {
                assume(&mut e, *k, t.clone());
            }
            let runs = runs_of[i].unwrap_or(Cost::ONE);
            for f in &passed[i] {
                match (u, f.upper) {
                    (Some(u), Some(fu)) => upper = upper.max(u.mul(runs).mul(fu)),
                    _ => known = false,
                }
                e.size_expressions.extend(f.sizes.iter().cloned());
                e.blockers.extend(f.blockers.iter().cloned());
                for (k, t) in &f.assumptions {
                    assume(&mut e, *k, t.clone());
                }
            }
        }
        for &line in &a.backward_gotos {
            e.blockers.push(Blocker {
                reason: Reason::UnstructuredFlow,
                line,
                text: "backward goto".into(),
                callee: None,
            });
        }
        // Recursion (spec §3.3 table).
        if !self_sites.is_empty() {
            match recursion(self_sites.as_slice(), a.memoized, known.then_some(upper)) {
                Ok(cost) => {
                    upper = cost;
                    e.size_expressions.insert(format!("{name} argument"));
                }
                Err(why) => e.blockers.push(Blocker {
                    reason: Reason::CyclicCall,
                    line: self_sites[0].line,
                    text: why,
                    callee: None,
                }),
            }
        }
        // One blocker per blocking construct: a name called a thousand times
        // is one reason, at its first line.
        let mut seen = HashSet::new();
        e.blockers
            .retain(|b| seen.insert((b.reason, b.text.clone(), b.callee)));
        // Calling a generator or async function runs none of its body.
        if a.deferred {
            lower = Cost::ONE;
        }
        if e.blockers.is_empty() && known {
            e.upper = Some(upper);
            e.lower = min_cost(lower, upper);
        } else {
            e.basis = Basis::Partial;
            e.lower = lower;
        }
        e
    }
}

/// A callee's sizes as its caller sees them: its own direct sizes, named
/// after it, or just its name when its sizes come from further down.
fn callee_sizes<'s>(callee: &str, sizes: impl Iterator<Item = &'s String>) -> Vec<String> {
    let (direct, deeper): (Vec<&String>, Vec<&String>) =
        sizes.partition(|s| !s.contains(": ") && !s.starts_with("via "));
    let mut out: Vec<String> = direct
        .into_iter()
        .map(|s| format!("{callee}: {s}"))
        .collect();
    if !deeper.is_empty() {
        out.push(format!("via {callee}"));
    }
    out
}

/// Whether a size expression is capped: every name in it is a capped size
/// (or an ALL_CAPS constant), and at least one is capped. The caps' wording.
fn capped(size: &str, caps: &BTreeMap<String, String>) -> Option<String> {
    let idents: Vec<&str> = size
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|x| !x.is_empty() && !x.starts_with(|c: char| c.is_ascii_digit()))
        .collect();
    let constant = |x: &str| {
        x.len() >= 2
            && x.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    let named: Vec<String> = idents
        .iter()
        .filter_map(|x| caps.get(*x).map(|c| format!("{x} capped by {c}")))
        .collect();
    let all = idents.iter().all(|x| caps.contains_key(*x) || constant(x));
    (all && !named.is_empty()).then(|| named.join(", "))
}

/// The spec's recursion table: `T(n) = a·T(smaller) + f(n)`. A self-call
/// shrinks its argument by halving (`n / 2`, `mid`) or by a constant
/// (`n - 1`, `xs[1:]`); one that does both counts as the slower,
/// decrementing shape.
fn recursion(sites: &[&CallSite], memoized: bool, f: Option<Cost>) -> Result<Cost, String> {
    if memoized {
        return Err("memoised recursion (cost not in the table)".into());
    }
    if sites.iter().any(|s| !s.loop_path.is_empty()) {
        return Err("recursion inside a loop".into());
    }
    let squash = |s: &String| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let words = |a: &str| -> Vec<String> {
        a.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let halved = |s: &&CallSite| {
        s.args.iter().map(squash).any(|a| {
            a.contains("/2")
                || a.contains(">>1")
                || a.contains("//2")
                || words(&a)
                    .iter()
                    .any(|w| matches!(w.as_str(), "mid" | "half" | "middle"))
        })
    };
    let decremented = |s: &&CallSite| {
        s.args.iter().map(squash).any(|a| {
            let after_minus = a.rsplit_once('-').map(|(l, r)| {
                !l.is_empty() && !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())
            });
            after_minus == Some(true) || a.ends_with("[1:]") || a == "rest" || a == "tail"
        })
    };
    let a = sites.len() as u32;
    let Some(f) = f else {
        return Err("non-recursive work has no upper bound".into());
    };
    let f_linear = f.exp_base.is_none() && (f.degree == 0 || (f.degree == 1 && f.log == 0));
    let f_const = f == Cost::ONE;
    let (n, log) = (Cost::n(), Cost::log_n());
    if sites.iter().all(|s| halved(s) || decremented(s)) && sites.iter().any(decremented) {
        return match (a, f_const, f_linear) {
            (1, true, _) => Ok(n),
            (1, false, true) => Ok(n.mul(n)),
            (a, _, _) if a >= 2 && f.exp_base.is_none() => Ok(Cost::exp(a)),
            _ => Err("decrementing recursion with exponential work".into()),
        };
    }
    if sites.iter().all(halved) {
        return match (a, f_const, f_linear) {
            (1, true, _) => Ok(log),
            (1, false, true) => Ok(n),
            (2, true, _) => Ok(n),
            (2, false, true) => Ok(n.mul(log)),
            _ => Err(format!("{a} halved self-calls (not in the table)")),
        };
    }
    Err("recursion shape not in the table".into())
}

/// Estimates for every function of the model, aligned with `m.fns`.
pub fn estimate_all(
    m: &Model,
    _cycles: &[Vec<usize>],
    cat: &Catalog,
    macros: &[MacroBody],
) -> Vec<Estimate> {
    let mut macros_by_name: HashMap<String, Vec<String>> = HashMap::new();
    let mut parsed: HashMap<String, Option<ParsedMacro>> = HashMap::new();
    for mb in macros {
        macros_by_name
            .entry(mb.name.clone())
            .or_default()
            .push(mb.iri.as_str().to_string());
        parsed.insert(mb.iri.as_str().to_string(), parse_macro(mb));
    }
    let mut c_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, f) in m.fns.iter().enumerate() {
        if f.grammar == Grammar::C {
            c_by_name.entry(f.name.clone()).or_default().push(i);
        }
    }
    // Order: callees first, through macro bodies too.
    let mut env = Env {
        m,
        cat,
        est: vec![None; m.fns.len()],
        component: vec![0; m.fns.len()],
        macros_by_name,
        parsed,
        macro_cost: HashMap::new(),
        in_progress: HashSet::new(),
        c_by_name,
        by_name: m.fns.iter().enumerate().fold(
            HashMap::new(),
            |mut acc: HashMap<(u8, String), Vec<usize>>, (i, f)| {
                acc.entry((super::catalog::family(f.grammar), f.name.clone()))
                    .or_default()
                    .push(i);
                acc
            },
        ),
    };
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); m.fns.len()];
    for (i, f) in m.fns.iter().enumerate() {
        let mut set = BTreeSet::new();
        for d in &f.defs {
            // Functions handed by name to higher-order calls are estimated first too.
            for (c, t) in d.tier_a.calls.iter().zip(&d.targets) {
                if env.entries_for(f.grammar, c, t).is_empty() {
                    continue;
                }
                for arg in &c.args {
                    let value = arg.split_once('=').map_or(arg.as_str(), |(_, r)| r);
                    if let Some(name) = function_reference(value) {
                        let key = (super::catalog::family(f.grammar), name.to_string());
                        if let Some(v) = env.by_name.get(&key) {
                            set.extend(v.iter().copied().filter(|j| *j != i));
                        }
                    }
                }
            }
            for t in &d.targets {
                match t {
                    Target::Repo(j) if *j != i => {
                        set.insert(*j);
                    }
                    Target::Ambiguous(v) => set.extend(v.iter().copied().filter(|j| *j != i)),
                    Target::Macro(iris) => {
                        let mut stack: Vec<String> =
                            iris.iter().map(|x| x.as_str().to_string()).collect();
                        let mut seen = HashSet::new();
                        while let Some(iri) = stack.pop() {
                            if !seen.insert(iri.clone()) || seen.len() > 64 {
                                continue;
                            }
                            if let Some(Some(pm)) = env.parsed.get(&iri) {
                                for t2 in env.macro_targets(pm) {
                                    match t2 {
                                        Target::Repo(j) if j != i => {
                                            set.insert(j);
                                        }
                                        Target::Macro(v) => {
                                            stack.extend(v.iter().map(|x| x.as_str().to_string()))
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        succ[i] = set.into_iter().collect();
    }
    let components = super::tier_b::tarjan(&succ);
    for (k, comp) in components.iter().enumerate() {
        for &i in comp {
            env.component[i] = k;
        }
    }
    for comp in &components {
        for &i in comp {
            let f = &m.fns[i];
            let defs: Vec<Estimate> = f
                .defs
                .iter()
                .map(|d| {
                    env.estimate_body(
                        Some(i),
                        &f.name,
                        f.grammar,
                        &d.tier_a,
                        &d.loops,
                        &d.targets,
                        d.stub,
                    )
                })
                .collect();
            env.est[i] = Some(merge_defs(defs));
        }
    }
    env.est
        .into_iter()
        .map(|e| e.expect("every function is estimated"))
        .collect()
}

/// Several definitions under one IRI: the maximum upper and minimum lower.
fn merge_defs(mut defs: Vec<Estimate>) -> Estimate {
    if defs.len() == 1 {
        return defs.pop().expect("one definition");
    }
    let n = defs.len();
    let mut out = defs[0].clone();
    for d in &defs[1..] {
        out.upper = match (out.upper, d.upper) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
        out.lower = min_cost(out.lower, d.lower);
        out.size_expressions
            .extend(d.size_expressions.iter().cloned());
        out.blockers.extend(d.blockers.iter().cloned());
        for (k, v) in &d.assumptions {
            out.assumptions
                .entry(*k)
                .or_default()
                .extend(v.iter().cloned());
        }
    }
    out.assumptions
        .entry(Assumption::MergedDefinitions)
        .or_default()
        .insert(format!("{n} definitions share this IRI"));
    out.definition_count = n;
    out.basis = if defs.iter().all(|d| d.basis == Basis::Unknown) {
        Basis::Unknown
    } else if out.upper.is_none() {
        Basis::Partial
    } else {
        Basis::ExactShape
    };
    out
}

/// The unlock list: for every uncatalogued name, how many functions it
/// alone keeps without an upper bound, following blocked callees back to
/// their root causes. The `top` names by count, ties by name.
pub fn unlock(m: &Model, est: &[Estimate], top: usize) -> Vec<(String, usize)> {
    let n = m.fns.len();
    let mut roots: Vec<Option<BTreeSet<(Reason, String)>>> = vec![None; n];
    for start in 0..n {
        if est[start].upper.is_some() || roots[start].is_some() {
            continue;
        }
        // Iterative post-order over blocked callees.
        let mut stack = vec![(start, false)];
        let mut on_path = HashSet::new();
        while let Some((i, expanded)) = stack.pop() {
            if roots[i].is_some() {
                continue;
            }
            if !expanded {
                on_path.insert(i);
                stack.push((i, true));
                for b in &est[i].blockers {
                    if let (Reason::BlockedCallee, Some(j)) = (b.reason, b.callee) {
                        if roots[j].is_none() && !on_path.contains(&j) {
                            stack.push((j, false));
                        }
                    }
                }
                continue;
            }
            on_path.remove(&i);
            let mut r = BTreeSet::new();
            for b in &est[i].blockers {
                match (b.reason, b.callee) {
                    (Reason::BlockedCallee, Some(j)) => {
                        if let Some(rj) = &roots[j] {
                            r.extend(rj.iter().cloned());
                        }
                    }
                    (reason, _) => {
                        r.insert((reason, b.text.clone()));
                    }
                }
            }
            roots[i] = Some(r);
        }
    }
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for r in roots.iter().flatten() {
        if let [(Reason::UncostedCall, name)] = r.iter().collect::<Vec<_>>().as_slice() {
            *count.entry(name.clone()).or_default() += 1;
        }
    }
    let mut v: Vec<(String, usize)> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(top);
    v
}

#[cfg(test)]
mod tests {
    use super::Basis::*;
    use super::Reason::*;
    use super::*;
    use crate::complexity::catalog::shipped;
    use crate::complexity::cost::GrowthClass::{self, *};
    use crate::complexity::model::{model_add_scanned, model_from_c};
    use crate::complexity::scan::scan_file;
    use crate::complexity::tier_b::tier_b;
    use crate::complexity::Grammar;

    fn by_name(m: &Model, est: Vec<Estimate>) -> BTreeMap<String, Estimate> {
        m.fns
            .iter()
            .zip(est)
            .map(|(f, e)| (f.name.clone(), e))
            .collect()
    }

    fn c_estimates(src: &str) -> BTreeMap<String, Estimate> {
        use crate::lang_c::{extract, new_parser, rdf as crdf};
        let files = vec![extract(&mut new_parser(), "f.c", src, None)];
        let ix = crdf::index("p", &files);
        let m = model_from_c("p", &files, &ix);
        let (_, cycles) = tier_b(&m);
        let est = estimate_all(&m, &cycles, &shipped(), &macro_bodies("p", &files));
        by_name(&m, est)
    }

    fn scanned_estimates(g: Grammar, src: &str) -> BTreeMap<String, Estimate> {
        let facts = scan_file("f", src.as_bytes(), g).0;
        let attached = facts
            .into_iter()
            .map(|f| (Iri::new(format!("urn:t:{}", f.name)), f))
            .collect();
        let mut m = Model::default();
        model_add_scanned(&mut m, attached);
        let (_, cycles) = tier_b(&m);
        let est = estimate_all(&m, &cycles, &shipped(), &[]);
        by_name(&m, est)
    }

    type Row<'a> = (
        &'a str,
        Option<GrowthClass>,
        GrowthClass,
        Basis,
        &'a [Reason],
        Option<GrowthClass>,
        GrowthClass,
    );

    /// Checks expected bounds, and that no bound is wrong: an upper bound is
    /// never below the true worst case, a lower bound never above the true
    /// best case.
    fn check(est: &BTreeMap<String, Estimate>, rows: &[Row]) {
        for (name, upper, lower, basis, reasons, true_upper, true_lower) in rows {
            let e = est
                .get(*name)
                .unwrap_or_else(|| panic!("no estimate for {name}"));
            let got_reasons: BTreeSet<Reason> = e.blockers.iter().map(|b| b.reason).collect();
            let want_reasons: BTreeSet<Reason> = reasons.iter().copied().collect();
            assert_eq!(
                e.upper.map(|c| c.class().0),
                *upper,
                "{name}: upper ({e:?})"
            );
            assert_eq!(e.lower.class().0, *lower, "{name}: lower");
            assert_eq!(e.basis, *basis, "{name}: basis");
            assert_eq!(got_reasons, want_reasons, "{name}: reasons");
            if let (Some(u), Some(t)) = (e.upper.map(|c| c.class().0), true_upper) {
                assert!(
                    u >= *t,
                    "{name}: upper {u:?} is below the true {t:?}: a wrong bound"
                );
            }
            assert!(
                e.lower.class().0 <= *true_lower,
                "{name}: lower above the true best case: a wrong bound"
            );
        }
    }

    #[test]
    fn fixture_bounds_c() {
        let est = c_estimates(include_str!("../../tests/fixtures/complexity/fixtures.c"));
        check(
            &est,
            &[
                ("sum", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("bs", Some(OLogN), O1, ExactShape, &[], Some(OLogN), O1),
                ("merge", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                (
                    "msort",
                    Some(ONLogN),
                    O1,
                    ExactShape,
                    &[],
                    Some(ONLogN),
                    ONLogN,
                ),
                ("bubble", Some(ON2), ON2, ExactShape, &[], Some(ON2), ON2),
                (
                    "fib",
                    Some(OExp(2)),
                    O1,
                    ExactShape,
                    &[],
                    Some(OExp(2)),
                    OExp(1),
                ),
                ("mfib", None, O1, Partial, &[CyclicCall], Some(ON), O1),
                ("count_a", Some(ON2), ON, ExactShape, &[], Some(ON2), ON2),
                ("pairs", Some(ON2), ON2, ExactShape, &[], Some(ON2), ON2),
                ("ten", Some(O1), O1, ExactShape, &[], Some(O1), O1),
                ("first", None, O1, Partial, &[UnboundedLoop], None, O1),
                ("sorts", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
                ("find", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("grow", None, O1, Partial, &[DataDependentLoop], None, O1),
                ("t3", None, O1, Partial, &[CyclicCall], Some(ON2), O1),
                ("u", None, O1, Partial, &[UncostedCall], None, O1),
                ("p", None, O1, Partial, &[BlockedCallee], None, O1),
                ("gt", None, O1, Partial, &[UnstructuredFlow], None, O1),
                ("fp", None, O1, Partial, &[IndirectCall], None, O1),
                ("e", Some(O1), O1, ExactShape, &[], Some(O1), O1),
                ("ma", Some(O1), O1, ExactShape, &[], Some(O1), O1),
                ("ms", Some(ON), O1, ExactShape, &[], Some(ON), ON),
                ("ml", Some(ON), ON, ExactShape, &[], Some(ON), ON),
            ],
        );
        assert_eq!(
            est["sorts"].upper.unwrap().class().1,
            None,
            "n * m^2 is exactly cubic"
        );
        assert!(est["count_a"].size_expressions.contains("strlen(s)"));
        let p = &est["p"].blockers[0];
        assert_eq!(p.reason, BlockedCallee);
        assert!(p.callee.is_some(), "the blocker names its callee");
        assert!(est["sum"].assumptions.contains_key(&Assumption::LoopShape));
        assert!(
            est["msort"]
                .assumptions
                .contains_key(&Assumption::LoopShape),
            "a caller's bound rests on its callee's assumptions"
        );
        assert!(
            est["ml"].assumptions.contains_key(&Assumption::LoopShape),
            "and on a macro body's"
        );
    }

    #[test]
    fn fixture_bounds_python() {
        let est = scanned_estimates(
            Grammar::Python,
            include_str!("../../tests/fixtures/complexity/fixtures.py"),
        );
        check(
            &est,
            &[
                ("total", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("bsearch", Some(OLogN), O1, ExactShape, &[], Some(OLogN), O1),
                ("merge", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                (
                    "msort",
                    Some(ONLogN),
                    O1,
                    ExactShape,
                    &[],
                    Some(ONLogN),
                    ONLogN,
                ),
                ("bubble", Some(ON2), ON2, ExactShape, &[], Some(ON2), ON2),
                (
                    "fib",
                    Some(OExp(2)),
                    O1,
                    ExactShape,
                    &[],
                    Some(OExp(2)),
                    OExp(1),
                ),
                ("mfib", None, O1, Partial, &[CyclicCall], Some(ON), O1),
                ("pairs", Some(ON2), ON2, ExactShape, &[], Some(ON2), ON2),
                ("ten", Some(O1), O1, ExactShape, &[], Some(O1), O1),
                ("first", None, O1, Partial, &[UnboundedLoop], None, O1),
                // `in` on a receiver of unknown type: a str substring search is the worst
                ("dup", Some(ON3), ON, ExactShape, &[], Some(ON2), ON),
                ("sorts", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
                ("find", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("grow", None, O1, Partial, &[DataDependentLoop], None, O1),
                ("t3", None, O1, Partial, &[CyclicCall], Some(ON2), O1),
                ("u", None, O1, Partial, &[UncostedCall], None, O1),
                ("p", None, O1, Partial, &[BlockedCallee], None, O1),
                ("e", Some(O1), O1, ExactShape, &[], Some(O1), O1),
            ],
        );
        assert_eq!(
            est["sorts"].upper.unwrap().class().1.as_deref(),
            Some("n^2 log n")
        );
        assert!(est["dup"]
            .assumptions
            .contains_key(&Assumption::BuiltinReceiver));
        assert!(est["dup"]
            .assumptions
            .contains_key(&Assumption::CatalogCase));
    }

    #[test]
    fn fixture_bounds_rust() {
        let est = scanned_estimates(
            Grammar::Rust,
            include_str!("../../tests/fixtures/complexity/fixtures.rs"),
        );
        check(
            &est,
            &[
                ("total", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("bsearch", Some(OLogN), O1, ExactShape, &[], Some(OLogN), O1),
                ("pairs", Some(ON2), ON2, ExactShape, &[], Some(ON2), ON2),
                ("ten", Some(O1), O1, ExactShape, &[], Some(O1), O1),
                ("first", None, O1, Partial, &[UnboundedLoop], None, O1),
                ("dup", Some(ON2), ON, ExactShape, &[], Some(ON2), ON),
                ("sorts", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
                ("find", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                (
                    "fib",
                    Some(OExp(2)),
                    O1,
                    ExactShape,
                    &[],
                    Some(OExp(2)),
                    OExp(1),
                ),
                ("grow", None, O1, Partial, &[DataDependentLoop], None, O1),
                ("u", None, O1, Partial, &[UncostedCall], None, O1),
                ("e", Some(O1), O1, ExactShape, &[], Some(O1), O1),
            ],
        );
    }

    fn js_rows() -> Vec<Row<'static>> {
        vec![
            ("total", Some(ON), ON, ExactShape, &[], Some(ON), ON),
            ("bsearch", Some(OLogN), O1, ExactShape, &[], Some(OLogN), O1),
            ("pairs", Some(ON2), ON2, ExactShape, &[], Some(ON2), ON2),
            ("ten", Some(O1), O1, ExactShape, &[], Some(O1), O1),
            ("first", None, O1, Partial, &[UnboundedLoop], None, O1),
            // `includes` on a receiver of unknown type: String's substring search is the worst
            ("dup", Some(ON3), ON, ExactShape, &[], Some(ON2), ON),
            ("sorts", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
            ("find", Some(ON), O1, ExactShape, &[], Some(ON), O1),
            ("cnt", Some(ON3), ON, ExactShape, &[], Some(ON2), ON2),
            (
                "fib",
                Some(OExp(2)),
                O1,
                ExactShape,
                &[],
                Some(OExp(2)),
                OExp(1),
            ),
            ("grow", None, O1, Partial, &[DataDependentLoop], None, O1),
            ("u", None, O1, Partial, &[UncostedCall], None, O1),
            ("e", Some(O1), O1, ExactShape, &[], Some(O1), O1),
        ]
    }

    #[test]
    fn fixture_bounds_javascript() {
        check(
            &scanned_estimates(
                Grammar::JavaScript,
                include_str!("../../tests/fixtures/complexity/fixtures.js"),
            ),
            &js_rows(),
        );
    }

    #[test]
    fn fixture_bounds_typescript() {
        check(
            &scanned_estimates(
                Grammar::TypeScript,
                include_str!("../../tests/fixtures/complexity/fixtures.ts"),
            ),
            &js_rows(),
        );
    }

    /// As [`c_estimates`], with the macro lists derived from the source so
    /// loop macros are rewritten and classified by their own headers.
    fn c_estimates_prepassed(src: &str) -> BTreeMap<String, Estimate> {
        use crate::lang_c::{extract, new_parser, prepass, rdf as crdf};
        let lists = prepass::derive_macros(&[("f.c".to_string(), src.to_string())]);
        let files = vec![extract(&mut new_parser(), "f.c", src, Some(&lists))];
        let ix = crdf::index("p", &files);
        let m = model_from_c("p", &files, &ix);
        let (_, cycles) = tier_b(&m);
        let est = estimate_all(&m, &cycles, &shipped(), &macro_bodies("p", &files));
        by_name(&m, est)
    }

    /// The final review's probes: shapes that got a wrong bound before.
    #[test]
    fn probe_bounds_c() {
        let est = c_estimates_prepassed(include_str!("../../tests/fixtures/complexity/probes.c"));
        check(
            &est,
            &[
                ("count_down", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("hash_words", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                (
                    "fixpoint",
                    None,
                    O1,
                    Partial,
                    &[DataDependentLoop],
                    None,
                    ON,
                ),
                ("serve", None, O1, Partial, &[DataDependentLoop], None, O1),
                (
                    "skip_continue",
                    Some(ON2),
                    ON,
                    ExactShape,
                    &[],
                    Some(ON2),
                    ON,
                ),
                (
                    "unknown_outer",
                    None,
                    O1,
                    Partial,
                    &[DataDependentLoop],
                    None,
                    O1,
                ),
                ("cmp_str", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("sort_names", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
                ("sort_unknown", None, O1, Partial, &[IndirectCall], None, O1),
                ("join_all", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
                ("walk", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("walk2", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                (
                    "draw",
                    Some(OExp(2)),
                    O1,
                    ExactShape,
                    &[],
                    Some(OExp(2)),
                    O1,
                ),
                (
                    "retry_forever",
                    None,
                    O1,
                    Partial,
                    &[UnboundedLoop],
                    None,
                    O1,
                ),
                ("repeat_sum", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                (
                    "retry_index",
                    None,
                    O1,
                    Partial,
                    &[DataDependentLoop],
                    None,
                    O1,
                ),
                ("start_offset", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("zero_trip", Some(ON2), O1, ExactShape, &[], Some(ON2), O1),
                ("ret_in_loop", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("twice", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("bytes", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("never", Some(ON), O1, ExactShape, &[], Some(O1), O1),
                ("list_len", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("str_len", Some(ON), ON, ExactShape, &[], Some(ON), ON),
            ],
        );
    }

    #[test]
    fn probe_bounds_python() {
        let est = scanned_estimates(
            Grammar::Python,
            include_str!("../../tests/fixtures/complexity/probes.py"),
        );
        check(
            &est,
            &[
                ("heavy", Some(ONLogN), O1, ExactShape, &[], Some(ONLogN), O1),
                ("map_named", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
                ("map_param", None, O1, Partial, &[IndirectCall], None, O1),
                ("sort_key", Some(ON3), O1, ExactShape, &[], Some(ON2), O1),
                ("gen", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("first_of", Some(ON), O1, ExactShape, &[], Some(O1), O1),
                ("fetch_all", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
                ("start_fetch", Some(ON3), O1, ExactShape, &[], Some(O1), O1),
                (
                    "walk_tree",
                    None,
                    O1,
                    Partial,
                    &[DataDependentLoop],
                    None,
                    O1,
                ),
                // the catalog's worst `pop` is pop(0): O(n)
                ("drain", Some(ON2), ON, ExactShape, &[], Some(ON), ON),
                ("purge", Some(ON2), ON, ExactShape, &[], Some(ON2), ON),
                (
                    "__contains__",
                    Some(ON2),
                    O1,
                    ExactShape,
                    &[],
                    Some(ON2),
                    O1,
                ),
                ("member", Some(ON2), O1, ExactShape, &[], Some(ON2), O1),
                ("count_down_py", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("twice_py", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("from_k", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("render", None, O1, Unknown, &[UncostedCall], None, O1),
                ("render_all", None, ON, Partial, &[UncostedCall], None, ON),
                ("bfs_py", None, O1, Partial, &[DataDependentLoop], None, O1),
                ("comp", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
                ("gen_sum", Some(ON3), O1, ExactShape, &[], Some(ON3), ON),
                (
                    "nested_comp",
                    Some(ON2),
                    ON2,
                    ExactShape,
                    &[],
                    Some(ON2),
                    ON2,
                ),
                (
                    "filtered_comp",
                    Some(ON3),
                    ON,
                    ExactShape,
                    &[],
                    Some(ON3),
                    ON,
                ),
                ("plain_loop", Some(ON3), ON, ExactShape, &[], Some(ON3), ON),
                ("skip_py", Some(ON2), ON, ExactShape, &[], Some(ON2), ON),
                ("poke", None, O1, Partial, &[IndirectCall], None, O1),
            ],
        );
    }

    #[test]
    fn probe_bounds_rust() {
        let est = scanned_estimates(
            Grammar::Rust,
            include_str!("../../tests/fixtures/complexity/probes.rs"),
        );
        check(
            &est,
            &[
                ("check", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("sum_down", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("sort_names", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
                ("heavy_rs", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                (
                    "map_named_rs",
                    Some(ON2),
                    O1,
                    ExactShape,
                    &[],
                    Some(ON2),
                    O1,
                ),
                ("skip_cont", Some(ON2), ON, ExactShape, &[], Some(ON2), ON),
                ("twice_rs", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("from_k", Some(ON), O1, ExactShape, &[], Some(ON), O1),
                ("down_range", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("drain_stack", Some(ON), ON, ExactShape, &[], Some(ON), ON),
                ("dfs", None, O1, Partial, &[DataDependentLoop], None, O1),
                ("fmt_len", Some(ON), ON, ExactShape, &[], Some(ON), ON),
            ],
        );
    }

    fn js_probe_rows() -> Vec<Row<'static>> {
        vec![
            (
                "transform",
                Some(ONLogN),
                O1,
                ExactShape,
                &[],
                Some(ONLogN),
                O1,
            ),
            ("mapNamed", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
            ("mapParam", None, O1, Partial, &[IndirectCall], None, O1),
            ("overlap", Some(ON3), ON, ExactShape, &[], Some(ON2), ON),
            ("countDown", Some(ON), ON, ExactShape, &[], Some(ON), ON),
            ("bfs", None, O1, Partial, &[DataDependentLoop], None, O1),
            ("twiceJs", Some(ON), ON, ExactShape, &[], Some(ON), ON),
            ("sortCmp", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
            ("skipJs", Some(ON2), ON, ExactShape, &[], Some(ON2), ON),
            ("genJs", Some(ON3), O1, ExactShape, &[], Some(ON3), O1),
            ("useGen", None, O1, Partial, &[IndirectCall], None, O1),
            ("poke", None, O1, Partial, &[IndirectCall], None, O1),
            ("walkList", Some(ON), ON, ExactShape, &[], Some(ON), ON),
        ]
    }

    #[test]
    fn probe_bounds_javascript_and_typescript() {
        let src = include_str!("../../tests/fixtures/complexity/probes.js");
        check(
            &scanned_estimates(Grammar::JavaScript, src),
            &js_probe_rows(),
        );
        check(
            &scanned_estimates(Grammar::TypeScript, src),
            &js_probe_rows(),
        );
    }

    fn c_estimates_with(src: &str, overlay: &str) -> BTreeMap<String, Estimate> {
        use crate::lang_c::{extract, new_parser, rdf as crdf};
        let files = vec![extract(&mut new_parser(), "f.c", src, None)];
        let ix = crdf::index("p", &files);
        let m = model_from_c("p", &files, &ix);
        let (_, cycles) = tier_b(&m);
        let cat = shipped().with_repository_overlay(overlay).unwrap();
        by_name(
            &m,
            estimate_all(&m, &cycles, &cat, &macro_bodies("p", &files)),
        )
    }

    #[test]
    fn a_size_is_capped_only_when_every_name_in_it_is() {
        let overlay = "@prefix code: <https://ontosys.io/ns/code#> .\n[] a code:SizeCap ; code:sizeName \"npos\" ; code:cappedBy \"MAXNUMPOS\" .\n";
        let est = c_estimates_with(
            "int both(int *a, int n, int npos) { int s = 0; for (int i = 0; i < n + npos; i++) s += a[i]; return s; }\nint only(int *a, int npos) { int s = 0; for (int i = 0; i < npos; i++) s += a[i]; return s; }\n",
            overlay,
        );
        assert_eq!(
            est["both"].upper.map(|c| c.class().0),
            Some(ON),
            "n is not capped"
        );
        assert_eq!(est["only"].upper.map(|c| c.class().0), Some(O1));
        assert!(est["only"]
            .assumptions
            .contains_key(&Assumption::OverlayCap));
    }

    #[test]
    fn an_overlay_entry_states_a_repository_function_or_macro() {
        let overlay = "@prefix code: <https://ontosys.io/ns/code#> .\n\
            [] a code:CostEntry ; code:language \"c\" ; code:operation \"find_entry\" ; code:worstClass code:OLogN ; code:sizeOf \"arg1\" ; code:source \"read\" .\n\
            [] a code:CostEntry ; code:language \"c\" ; code:operation \"BLOCKER\" ; code:worstClass code:O1 ; code:source \"stated by the owner\" .\n";
        let est = c_estimates_with(
            "#define BLOCKER(x) frob(x)\nint find_entry(int *a, int n, int x) { int lo = 0, hi = n; while (lo < hi) { int m = lo + step(hi - lo); if (a[m] < x) lo = m + 1; else hi = m; } return lo; }\nint caller(int *a, int n, int x) { return find_entry(a, n, x); }\nint uses_macro(int x) { return BLOCKER(x); }\n",
            overlay,
        );
        assert_eq!(
            est["caller"].upper.map(|c| c.class().0),
            Some(OLogN),
            "{:?}",
            est["caller"].blockers
        );
        assert!(est["caller"]
            .assumptions
            .contains_key(&Assumption::OverlayCost));
        assert_eq!(
            est["uses_macro"].upper.map(|c| c.class().0),
            Some(O1),
            "{:?}",
            est["uses_macro"].blockers
        );
        assert_eq!(est["find_entry"].upper, None, "its own body stays derived");
    }

    #[test]
    fn ifdef_alternatives_merge_soundly() {
        let est = c_estimates("#ifdef A\nint mm(int n) { int s = 0; for (int i = 0; i < n; i++) s++; return s; }\n#else\nint mm(int n) { return 0; }\n#endif\n");
        let e = &est["mm"];
        assert_eq!(e.definition_count, 2);
        assert_eq!(
            e.upper.map(|c| c.class().0),
            Some(ON),
            "the maximum over the alternatives"
        );
        assert_eq!(e.lower.class().0, O1, "the minimum over the alternatives");
        assert!(e.assumptions.contains_key(&Assumption::MergedDefinitions));
    }

    #[test]
    fn longjmp_leaves_early_and_setjmp_blocks() {
        let est = c_estimates("void raise_it(void) { siglongjmp(env, 1); }\nint guarded(int n) { if (sigsetjmp(env, 0)) return 0; return n; }\nint caller(int n) { if (n < 0) raise_it(); return n; }\n");
        assert_eq!(
            est["raise_it"].upper.map(|c| c.class().0),
            Some(O1),
            "a jump away only shortens execution"
        );
        assert_eq!(est["caller"].upper.map(|c| c.class().0), Some(O1));
        assert_eq!(
            est["guarded"].upper, None,
            "a setjmp point can be re-entered"
        );
        assert!(est["guarded"]
            .blockers
            .iter()
            .any(|b| b.reason == UnstructuredFlow));
    }

    #[test]
    fn unlock_list_follows_blocked_callees_to_roots() {
        use crate::lang_c::{extract, new_parser, rdf as crdf};
        let src = "int b(int n) { return foo(n); }\nint a(int n) { return b(n); }\nint c(int n) { return foo(n); }\nint d(int n) { return foo(n) + bar(n); }\nint z(int n) { return baz(n); }\n";
        let files = vec![extract(&mut new_parser(), "f.c", src, None)];
        let ix = crdf::index("p", &files);
        let m = model_from_c("p", &files, &ix);
        let (_, cycles) = tier_b(&m);
        let est = estimate_all(&m, &cycles, &shipped(), &[]);
        let list = unlock(&m, &est, 10);
        assert_eq!(
            list,
            vec![("foo".to_string(), 3), ("baz".to_string(), 1)],
            "d needs foo and bar, so neither alone unlocks it"
        );
    }

    #[test]
    fn macros_with_stringify_and_paste_are_derived() {
        let est = c_estimates("#define CHECK(c) do { if (!(c)) fail(#c, __LINE__); } while (0)\n#define CAT(a, b) a##b\nvoid fail(const char *s, int l) { }\nint f(int x) { CHECK(x > 0); return x; }\n");
        assert_eq!(
            est["f"].upper.map(|c| c.class().0),
            Some(O1),
            "{:?}",
            est["f"].blockers
        );
    }

    #[test]
    fn an_ambiguous_c_name_is_costed_over_its_candidates() {
        use crate::lang_c::{extract, new_parser, rdf as crdf};
        let files = vec![
            extract(
                &mut new_parser(),
                "backend/mem.c",
                "void release(void *p) { }\n",
                None,
            ),
            extract(
                &mut new_parser(),
                "frontend/mem.c",
                "void release(void *p) { }\n",
                None,
            ),
            extract(
                &mut new_parser(),
                "ext/use.c",
                "void use(void *p) { release(p); }\n",
                None,
            ),
        ];
        let ix = crdf::index("p", &files);
        let m = model_from_c("p", &files, &ix);
        let (_, cycles) = tier_b(&m);
        let est = by_name(&m, estimate_all(&m, &cycles, &shipped(), &[]));
        assert_eq!(est["use"].upper.map(|c| c.class().0), Some(O1));
        assert!(
            est["use"]
                .assumptions
                .contains_key(&Assumption::BuiltinReceiver),
            "the choice of candidate is recorded"
        );
    }

    #[test]
    fn callee_detail_does_not_snowball_up_the_call_chain() {
        let mut src = String::from("int f0(int *a, int n) { int s = 0; for (int i = 0; i < n; i++) for (int j = 0; j < n; j++) s += a[j]; return s; }\n");
        for k in 1..30 {
            src.push_str(&format!(
                "int f{k}(int *a, int n) {{ return f{}(a, n) + f{}(a, n); }}\n",
                k - 1,
                k - 1
            ));
        }
        let est = c_estimates(&src);
        let top = &est["f29"];
        assert!(
            top.size_expressions.len() <= 4,
            "{:?}",
            top.size_expressions
        );
        assert!(
            top.size_expressions
                .iter()
                .all(|s| s.matches(": ").count() <= 1),
            "a callee is named once, not its whole chain: {:?}",
            top.size_expressions
        );
        assert!(
            top.assumptions.values().map(|v| v.len()).sum::<usize>() <= 4,
            "{:?}",
            top.assumptions
        );
        assert_eq!(top.upper.map(|c| c.class().0), Some(ON2));
    }

    #[test]
    fn multi_line_macros_are_derived() {
        let est = c_estimates("#define CHECK(c) \\\n\tdo { \\\n\t\tif (!(c)) \\\n\t\t\tfail(#c); \\\n\t} while (0)\nvoid fail(const char *s) { }\nint f(int x) { CHECK(x > 0); return x; }\n");
        assert_eq!(
            est["f"].upper.map(|c| c.class().0),
            Some(O1),
            "{:?}",
            est["f"].blockers
        );
    }

    #[test]
    fn a_construct_repeated_is_one_blocker() {
        let est =
            c_estimates("int f(int n) { return frob(n) + frob(n + 1) + frob(n + 2) + zap(n); }\n");
        let texts: Vec<&str> = est["f"].blockers.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["frob", "zap"]);
    }

    #[test]
    fn empty_body_is_theta_one() {
        let e = &c_estimates("void nothing(void) { }\n")["nothing"];
        assert_eq!(
            (e.upper, e.lower, e.basis),
            (Some(Cost::ONE), Cost::ONE, ExactShape)
        );
    }

    #[test]
    fn a_body_with_parse_errors_is_unknown() {
        let est = scanned_estimates(
            Grammar::Python,
            "def broken(xs):\n    for x in xs:\n        y = (x +\n    return 1\n",
        );
        if let Some(e) = est.get("broken") {
            assert_eq!(e.basis, Unknown);
            assert_eq!(e.upper, None);
            assert!(e.blockers.iter().any(|b| b.reason == Unparsed));
        }
    }
}
