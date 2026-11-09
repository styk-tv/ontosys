//! # Complexity → RDF
//!
//! Every term is in `https://ontosys.io/ns/code#` and declared in
//! `ontology/code.ttl`.

use super::engine::TierA;
use crate::ontology::{Iri, Literal, Triple, TripleSet, OC};

fn int(n: usize) -> Literal {
    Literal::integer(n as i64)
}

/// `oc:resolvesTo` from each external symbol a C function calls to every
/// `cx:Macro` of that name. In complexity.nt only: re-pointing `cx:calls`
/// would move graph.nt for every consumer that pins it.
pub fn macro_links(
    project: &str,
    files: &[crate::lang_c::CFile],
    ix: &crate::lang_c::rdf::CIndex,
    out: &mut TripleSet,
) {
    use crate::ontology::instance_iri;
    let mut by_name: std::collections::BTreeMap<&str, Vec<Iri>> = std::collections::BTreeMap::new();
    for f in files {
        for m in &f.macros {
            by_name
                .entry(m.name.as_str())
                .or_default()
                .push(instance_iri(
                    project,
                    "macro",
                    &format!("{}/{}", f.path, m.name),
                ));
        }
    }
    for f in files {
        for func in &f.functions {
            for callee in &func.calls {
                if ix.resolve_fn(callee, &f.path).is_some() {
                    continue;
                }
                if let Some(macros) = by_name.get(callee.as_str()) {
                    let sym = instance_iri(project, "sym", callee);
                    for m in macros {
                        out.add(Triple::new(sym.clone(), OC.iri("resolvesTo"), m.clone()));
                    }
                }
            }
        }
    }
}

/// Tier B facts, in complexity.nt. Every function described there is typed
/// `oc:AnalysedFunction` there too, so the file validates on its own.
pub fn tier_b_triples(
    project: &str,
    m: &super::model::Model,
    tb: &[super::tier_b::TierB],
    cycles: &[Vec<usize>],
    out: &mut TripleSet,
) {
    let cycle_iris: Vec<Iri> = cycles
        .iter()
        .map(|c| super::tier_b::cycle_iri(project, m, c))
        .collect();
    for (f, b) in m.fns.iter().zip(tb) {
        let s = &f.iri;
        out.add(Triple::type_of(s.clone(), OC.iri("AnalysedFunction")));
        out.add(Triple::new(s.clone(), OC.iri("fanIn"), int(b.fan_in)));
        out.add(Triple::new(s.clone(), OC.iri("fanOut"), int(b.fan_out)));
        out.add(Triple::new(
            s.clone(),
            OC.iri("externalCallCount"),
            int(b.external_calls),
        ));
        if let Some(d) = b.transitive_loop_depth {
            out.add(Triple::new(
                s.clone(),
                OC.iri("transitiveLoopDepth"),
                int(d as usize),
            ));
        }
        if let Some(k) = b.cycle {
            out.add(Triple::new(
                s.clone(),
                OC.iri("inCallCycle"),
                cycle_iris[k].clone(),
            ));
        }
        if f.defs.len() > 1 {
            out.add(Triple::new(
                s.clone(),
                OC.iri("definitionCount"),
                int(f.defs.len()),
            ));
        }
    }
    for (iri, c) in cycle_iris.iter().zip(cycles) {
        out.add(Triple::type_of(iri.clone(), OC.iri("CallCycle")));
        out.add(Triple::new(
            iri.clone(),
            OC.iri("memberCount"),
            int(c.len()),
        ));
    }
}

/// Tier C in complexity.nt: per function an estimate, its loops, blockers
/// and findings, every node typed in the file itself.
pub fn tier_c_triples(
    m: &super::model::Model,
    est: &[super::estimate::Estimate],
    findings: &[Vec<super::findings::Finding>],
    catalog_version: &str,
    out: &mut TripleSet,
) {
    use super::cost::GrowthClass;
    use super::estimate::Basis;
    use super::loops::BoundClass;
    let class = |c: GrowthClass| match c {
        GrowthClass::ONK(_) => OC.iri("ONK"),
        GrowthClass::OExp(_) => OC.iri("OExp"),
        other => OC.iri(&format!("{other:?}")),
    };
    let lit = |s: &str| Literal::string(clip(s));
    for (i, f) in m.fns.iter().enumerate() {
        let (fi, e) = (&f.iri, &est[i]);
        let ei = Iri::new(format!("{}/estimate", fi.as_str()));
        put(out, &ei, "estimates", fi.clone().into());
        let basis = match e.basis {
            Basis::ExactShape => "exact-shape",
            Basis::Partial => "partial",
            Basis::Unknown => "unknown",
        };
        put(out, &ei, "basis", Literal::string(basis).into());
        put(
            out,
            &ei,
            "catalogVersion",
            Literal::string(catalog_version).into(),
        );
        if let Some(u) = e.upper {
            let (c, exact) = u.class();
            put(out, &ei, "upperClass", class(c).into());
            put(out, &ei, "degree", int(u.degree as usize).into());
            if let GrowthClass::OExp(b) = c {
                put(out, &ei, "base", int(b as usize).into());
            }
            if let Some(x) = exact {
                put(out, &ei, "exactForm", Literal::string(x).into());
            }
        }
        put(out, &ei, "lowerClass", class(e.lower.class().0).into());
        for sz in &e.size_expressions {
            put(out, &ei, "sizeExpression", lit(sz).into());
        }
        for r in e
            .blockers
            .iter()
            .map(|b| b.reason)
            .collect::<std::collections::BTreeSet<_>>()
        {
            put(out, &ei, "blockedBy", OC.iri(&format!("{r:?}")).into());
        }
        for (k, texts) in &e.assumptions {
            put(
                out,
                &ei,
                "assumes",
                OC.iri(&format!("{k:?}Assumption")).into(),
            );
            for t in texts {
                put(out, &ei, "assumptionText", lit(t).into());
            }
        }
        let mut n = 0;
        for d in &f.defs {
            for l in &d.loops {
                n += 1;
                let li = Iri::new(format!("{}/loop/{n}", fi.as_str()));
                put(out, &li, "inFunction", fi.clone().into());
                put(out, &li, "startLine", int(l.site.line).into());
                put(out, &li, "loopDepth", int(l.site.depth as usize).into());
                let bc = match l.class {
                    BoundClass::Constant => "ConstantBound",
                    BoundClass::Size => "SizeBound",
                    BoundClass::Sentinel => "SentinelBound",
                    BoundClass::Halving => "HalvingBound",
                    BoundClass::Unbounded => "Unbounded",
                    BoundClass::Unknown => "UnknownBound",
                };
                put(out, &li, "boundClass", OC.iri(bc).into());
                if !l.bound_expression.is_empty() {
                    put(out, &li, "boundExpression", lit(&l.bound_expression).into());
                }
                out.add(Triple::type_of(li, OC.iri("Loop")));
            }
        }
        for (k, b) in e.blockers.iter().enumerate() {
            let bi = Iri::new(format!("{}/blocker/{}", fi.as_str(), k + 1));
            put(
                out,
                &bi,
                "reason",
                OC.iri(&format!("{:?}", b.reason)).into(),
            );
            if b.line > 0 {
                put(out, &bi, "startLine", int(b.line).into());
            }
            if !b.text.is_empty() {
                put(out, &bi, "blockerText", lit(&b.text).into());
            }
            if let Some(j) = b.callee {
                put(out, &bi, "blockedCallee", m.fns[j].iri.clone().into());
            }
            out.add(Triple::type_of(bi, OC.iri("Blocker")));
        }
        for (k, fd) in findings.get(i).into_iter().flatten().enumerate() {
            let di = Iri::new(format!("{}/finding/{}", fi.as_str(), k + 1));
            out.add(Triple::new(
                di.clone(),
                OC.iri("findingKind"),
                OC.iri(&format!("{:?}", fd.kind)),
            ));
            out.add(Triple::new(di.clone(), OC.iri("startLine"), int(fd.line)));
            out.add(Triple::new(
                di.clone(),
                OC.iri("evidenceText"),
                lit(&fd.text),
            ));
            out.add(Triple::new(di.clone(), OC.iri("inFunction"), fi.clone()));
            out.add(Triple::type_of(di, OC.iri("Finding")));
        }
        out.add(Triple::type_of(ei, OC.iri("GrowthEstimate")));
    }
}

fn put(out: &mut TripleSet, s: &Iri, local: &str, o: crate::ontology::Term) {
    out.add(Triple::new(s.clone(), OC.iri(local), o));
}

/// At most 200 characters, cut on a char boundary, whitespace collapsed.
fn clip(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

/// Tier A facts of one function, on its IRI.
pub fn tier_a_triples(subject: &Iri, a: &TierA, sha: Option<&str>, out: &mut TripleSet) {
    let mut add = |local: &str, v: Literal| out.add(Triple::new(subject.clone(), OC.iri(local), v));
    add("statementCount", int(a.statements));
    add("cyclomatic", int(a.cyclomatic));
    add("cognitive", int(a.cognitive));
    add("maxNesting", int(a.max_nesting as usize));
    add("loopCount", int(a.loop_count));
    add("maxLoopNesting", int(a.max_loop_nesting as usize));
    add("selfCalls", int(a.self_calls));
    if let Some(sha) = sha {
        add("definitionSha256", Literal::string(sha));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complexity::{engine, Grammar};
    use oxigraph::io::{RdfFormat, RdfParser};
    use oxigraph::model::{NamedOrBlankNode, Term};
    use std::collections::{BTreeMap, BTreeSet};

    const OC: &str = "https://ontosys.io/ns/code#";
    const OWL: &str = "http://www.w3.org/2002/07/owl#";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

    /// Declared subject IRI → its rdf:type values, from ontology/code.ttl.
    pub(crate) fn declarations() -> BTreeMap<String, BTreeSet<String>> {
        let ttl = include_str!("../../ontology/code.ttl");
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for q in RdfParser::from_format(RdfFormat::Turtle).for_slice(ttl.as_bytes()) {
            let q = q.expect("code.ttl parses");
            if q.predicate.as_str() == RDF_TYPE {
                if let (NamedOrBlankNode::NamedNode(s), Term::NamedNode(o)) =
                    (&q.subject, &q.object)
                {
                    out.entry(s.as_str().to_string())
                        .or_default()
                        .insert(o.as_str().to_string());
                }
            }
        }
        out
    }

    #[test]
    fn vocabulary_declares_every_emitted_term_with_an_owl_type() {
        let src = "int fact(int n) { for (int i = 0; i < n; i++) {} return n <= 1 ? 1 : n * fact(n - 1); }";
        let mut p = tree_sitter::Parser::new();
        p.set_language(&Grammar::C.language().unwrap()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let def = tree.root_node().named_child(0).unwrap();
        let a = engine::analyze(
            def.child_by_field_name("body").unwrap(),
            src.as_bytes(),
            "fact",
            Grammar::C.profile().unwrap(),
        );
        let mut out = TripleSet::new();
        tier_a_triples(&Iri::new("urn:t:fact"), &a, Some("ab"), &mut out);
        let decl = declarations();
        let emitted: BTreeSet<String> = out
            .iter()
            .map(|t| t.predicate.as_str().to_string())
            .filter(|p| p.starts_with(OC))
            .collect();
        assert_eq!(emitted.len(), 8, "all Tier A terms emitted: {emitted:?}");
        for p in &emitted {
            let types = decl
                .get(p)
                .unwrap_or_else(|| panic!("{p} not declared in ontology/code.ttl"));
            assert!(
                types.contains(&format!("{OWL}DatatypeProperty"))
                    || types.contains(&format!("{OWL}ObjectProperty")),
                "{p}: {types:?}"
            );
        }
    }

    #[test]
    fn macro_calls_resolve_to_every_same_named_macro() {
        use crate::lang_c::{extract, new_parser, rdf as crdf};
        let files = vec![
            extract(&mut new_parser(), "a.h", "#define M(x) (x)\n", None),
            extract(&mut new_parser(), "b.h", "#define M(x) ((x) + 1)\n", None),
            extract(
                &mut new_parser(),
                "c.c",
                "int f(void) { return M(1) + g(); }\nint g(void) { return 0; }\n",
                None,
            ),
        ];
        let ix = crdf::index("p", &files);
        let mut cx_out = TripleSet::new();
        crdf::to_triples("p", &files, &ix, &mut cx_out);
        let mut links = TripleSet::new();
        macro_links("p", &files, &ix, &mut links);
        let nt = links.to_ntriples();
        let sym = crate::ontology::instance_iri("p", "sym", "M");
        for path in ["a.h/M", "b.h/M"] {
            let m = crate::ontology::instance_iri("p", "macro", path);
            assert!(
                nt.contains(&format!(
                    "<{}> <{OC}resolvesTo> <{}>",
                    sym.as_str(),
                    m.as_str()
                )),
                "{nt}"
            );
        }
        assert_eq!(
            nt.lines().count(),
            2,
            "g resolves to a function, so it gets no macro link: {nt}"
        );
        assert!(
            !cx_out.to_ntriples().contains("resolvesTo"),
            "graph.nt never carries the links"
        );
    }

    #[test]
    fn complexity_nt_validates_alone() {
        use crate::complexity::model::Target::*;
        use crate::complexity::tier_b::{tests::model, tier_b};
        let m = model(&[
            ("x", 1, vec![("y", 0, Repo(1))]),
            ("y", 0, vec![("x", 0, Repo(0))]),
            ("z", 2, vec![("x", 1, Repo(0))]),
        ]);
        let (tb, cycles) = tier_b(&m);
        let mut out = TripleSet::new();
        tier_b_triples("p", &m, &tb, &cycles, &mut out);
        let mut typed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut described: BTreeSet<String> = BTreeSet::new();
        for t in out.iter() {
            let s = match &t.subject {
                crate::ontology::Term::Iri(i) => i.as_str().to_string(),
                _ => panic!("no blank nodes"),
            };
            if t.predicate.as_str() == RDF_TYPE {
                if let crate::ontology::Term::Iri(o) = &t.object {
                    typed.entry(s).or_default().insert(o.as_str().to_string());
                }
            } else {
                described.insert(s);
            }
        }
        let targeted = described
            .iter()
            .filter(|s| {
                typed.get(*s).is_some_and(|t| {
                    t.contains(&format!("{OC}AnalysedFunction"))
                        || t.contains(&format!("{OC}CallCycle"))
                })
            })
            .count();
        assert_eq!(
            targeted,
            described.len(),
            "every described node is typed in the same file"
        );
        assert_eq!(targeted, 4, "three functions and one cycle");
        let decl = declarations();
        for t in out.iter() {
            for iri in [
                t.predicate.as_str().to_string(),
                match &t.object {
                    crate::ontology::Term::Iri(i) => i.as_str().to_string(),
                    _ => String::new(),
                },
            ] {
                if iri.starts_with(OC) {
                    assert!(
                        decl.contains_key(&iri),
                        "{iri} not declared in ontology/code.ttl"
                    );
                }
            }
        }
    }

    fn fixture_c_model() -> (
        super::super::model::Model,
        Vec<super::super::estimate::Estimate>,
        Vec<Vec<super::super::findings::Finding>>,
    ) {
        use crate::complexity::{catalog::shipped, estimate, findings, model, tier_b};
        let files = vec![crate::lang_c::extract(
            &mut crate::lang_c::new_parser(),
            "f.c",
            include_str!("../../tests/fixtures/complexity/fixtures.c"),
            None,
        )];
        let ix = crate::lang_c::rdf::index("p", &files);
        let m = model::model_from_c("p", &files, &ix);
        let (_, cycles) = tier_b::tier_b(&m);
        let cat = shipped();
        let est = estimate::estimate_all(&m, &cycles, &cat, &estimate::macro_bodies("p", &files));
        let found = m
            .fns
            .iter()
            .map(|f| {
                f.defs
                    .iter()
                    .flat_map(|d| findings::findings(d, &d.evidence, &cat, f.grammar, &f.name))
                    .collect()
            })
            .collect();
        (m, est, found)
    }

    fn emitted(
        m: &super::super::model::Model,
        est: &[super::super::estimate::Estimate],
        found: &[Vec<super::super::findings::Finding>],
    ) -> String {
        let mut out = TripleSet::new();
        tier_c_triples(m, est, found, "v0", &mut out);
        out.canonicalize();
        out.to_ntriples()
    }

    #[test]
    fn estimate_triples_round_trip_for_each_basis() {
        let (m, est, found) = fixture_c_model();
        let nt = emitted(&m, &est, &found);
        let iri = |name: &str| {
            m.fns
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .iri
                .as_str()
                .to_string()
        };
        let has = |s: &str, local: &str, o: &str| nt.contains(&format!("<{s}> <{OC}{local}> {o}"));
        let sum = format!("{}/estimate", iri("sum"));
        assert!(has(&sum, "estimates", &format!("<{}>", iri("sum"))));
        assert!(has(&sum, "upperClass", &format!("<{OC}ON>")));
        assert!(has(&sum, "lowerClass", &format!("<{OC}ON>")));
        assert!(has(&sum, "basis", "\"exact-shape\""));
        assert!(has(&sum, "assumes", &format!("<{OC}LoopShapeAssumption>")));
        assert!(has(&sum, "catalogVersion", "\"v0\""));
        let fib = format!("{}/estimate", iri("fib"));
        assert!(
            has(&fib, "upperClass", &format!("<{OC}OExp>"))
                && has(
                    &fib,
                    "base",
                    "\"2\"^^<http://www.w3.org/2001/XMLSchema#integer>"
                )
        );
        let sorts = format!("{}/estimate", iri("sorts"));
        assert!(has(
            &sorts,
            "degree",
            "\"3\"^^<http://www.w3.org/2001/XMLSchema#integer>"
        ));
        let loop1 = format!("{}/loop/1", iri("sum"));
        assert!(
            has(&loop1, "boundClass", &format!("<{OC}SizeBound>"))
                && has(&loop1, "boundExpression", "\"n\"")
        );
        assert!(has(&loop1, "inFunction", &format!("<{}>", iri("sum"))));
        let p = format!("{}/blocker/1", iri("p"));
        assert!(
            has(&p, "reason", &format!("<{OC}BlockedCallee>"))
                && has(&p, "blockedCallee", &format!("<{}>", iri("u")))
        );
        let f1 = format!("{}/finding/1", iri("count_a"));
        assert!(
            has(
                &f1,
                "findingKind",
                &format!("<{OC}LinearCallInLoopCondition>")
            ),
            "{nt}"
        );
        assert!(crate::ontology::strict_violations(&nt).is_empty());
    }

    #[test]
    fn upper_class_absent_when_partial() {
        let (m, est, found) = fixture_c_model();
        let nt = emitted(&m, &est, &found);
        let grow = format!(
            "{}/estimate",
            m.fns
                .iter()
                .find(|f| f.name == "grow")
                .unwrap()
                .iri
                .as_str()
        );
        assert!(nt.contains(&format!("<{grow}> <{OC}basis> \"partial\"")));
        assert!(nt.contains(&format!("<{grow}> <{OC}blockedBy> <{OC}DataDependentLoop>")));
        assert!(!nt.contains(&format!("<{grow}> <{OC}upperClass>")));
    }

    #[test]
    fn long_multibyte_bound_expression_is_cut_on_a_char_boundary() {
        let long = format!("x < {}", "é中😀".repeat(120));
        let src =
            format!("def f(xs):\n    i = 0\n    while i < len(\"{long}\"):\n        i += 1\n");
        let facts = crate::complexity::scan::scan_file(
            "f.py",
            src.as_bytes(),
            crate::complexity::Grammar::Python,
        )
        .0;
        let mut m = super::super::model::Model::default();
        super::super::model::model_add_scanned(
            &mut m,
            facts
                .into_iter()
                .map(|f| (Iri::new("urn:t:f"), f))
                .collect(),
        );
        let (_, cycles) = super::super::tier_b::tier_b(&m);
        let est = super::super::estimate::estimate_all(
            &m,
            &cycles,
            &crate::complexity::catalog::shipped(),
            &[],
        );
        let nt = emitted(&m, &est, &[vec![]]);
        assert!(crate::ontology::strict_violations(&nt).is_empty(), "{nt}");
        for line in nt.lines().filter(|l| l.contains("boundExpression")) {
            let lit = line.split('"').nth(1).unwrap();
            assert!(lit.chars().count() <= 200, "{}", lit.chars().count());
        }
    }

    #[test]
    fn complexity_nt_deterministic_on_fixture_tree() {
        let (m, est, found) = fixture_c_model();
        let (m2, est2, found2) = fixture_c_model();
        assert_eq!(emitted(&m, &est, &found), emitted(&m2, &est2, &found2));
    }

    #[test]
    fn shapes_target_only_nodes_typed_in_complexity_nt() {
        let ttl = include_str!("../../ontology/code.ttl");
        let mut targets = BTreeSet::new();
        for q in RdfParser::from_format(RdfFormat::Turtle).for_slice(ttl.as_bytes()) {
            let q = q.unwrap();
            if q.predicate.as_str() == "http://www.w3.org/ns/shacl#targetClass" {
                if let Term::NamedNode(o) = &q.object {
                    targets.insert(o.as_str().to_string());
                }
            }
        }
        assert!(!targets.is_empty(), "code.ttl carries shapes");
        for t in &targets {
            assert!(t.starts_with(OC), "{t}: a shape on a class typed in graph.nt would be vacuous or refused on complexity.nt");
        }
        for class in [
            "AnalysedFunction",
            "CallCycle",
            "GrowthEstimate",
            "Loop",
            "Blocker",
            "Finding",
        ] {
            assert!(
                targets.contains(&format!("{OC}{class}")),
                "{class} has a shape"
            );
        }
        let (m, est, found) = fixture_c_model();
        let nt = emitted(&m, &est, &found);
        for class in ["GrowthEstimate", "Loop", "Blocker", "Finding"] {
            assert!(
                nt.contains(&format!("<{RDF_TYPE}> <{OC}{class}>")),
                "{class} nodes are typed in the file, so a shape targets something"
            );
        }
    }

    #[test]
    fn vocabulary_declares_only_its_own_namespace() {
        let decl = declarations();
        assert!(!decl.is_empty());
        for (s, types) in &decl {
            let declared_as_term = types.iter().any(|t| t.starts_with(OWL));
            if declared_as_term && !s.starts_with(OC) && s != OC.trim_end_matches('#') {
                panic!("{s} is declared in code.ttl but is not a code# term");
            }
            assert!(
                !types.contains("http://www.w3.org/1999/02/22-rdf-syntax-ns#Property")
                    || types.len() > 1,
                "{s} typed only rdf:Property escapes V2"
            );
        }
    }
}
