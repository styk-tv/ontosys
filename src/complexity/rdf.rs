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
