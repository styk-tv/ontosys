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
