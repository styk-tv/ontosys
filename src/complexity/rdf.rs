//! # Complexity → RDF
//!
//! Every term is in `https://ontosys.io/ns/code#` and declared in
//! `ontology/code.ttl`.

use super::engine::TierA;
use crate::ontology::{Iri, Literal, Triple, TripleSet, OC};

fn int(n: usize) -> Literal {
    Literal::integer(n as i64)
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
