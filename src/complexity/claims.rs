//! # Tier D: claims
//!
//! A person's or an agent's reading of how a function's cost grows, written
//! into the repository's own `.ontosys/claims/complexity.ttl` and tied to the
//! function's `code:definitionSha256`. Claims are inputs, never build
//! outputs: the build copies each one into complexity.nt with its status,
//! current while the definition is byte-for-byte unchanged, stale after.

use super::model::Model;
use crate::ontology::TripleSet;

/// One claim as read from the repository.
#[derive(Debug, Clone)]
pub struct Claim {
    /// The function it is about.
    pub about: String,
    /// The `code:definitionSha256` it was made against.
    pub made_against: String,
    /// Every other `code:` property, copied as written.
    pub props: Vec<(String, oxigraph::model::Term)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ClaimStats {
    pub current: usize,
    pub stale: usize,
    /// Claims about a function this build no longer has (also stale).
    pub missing: usize,
}

const CODE: &str = "https://ontosys.io/ns/code#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// Read claims from Turtle. A parse error names its line.
pub fn parse_claims(ttl: &str) -> anyhow::Result<Vec<Claim>> {
    use oxigraph::io::{RdfFormat, RdfParser};
    use oxigraph::model::{NamedOrBlankNode, Term};
    let mut by_subject: std::collections::BTreeMap<String, Vec<(String, Term)>> =
        std::collections::BTreeMap::new();
    for q in RdfParser::from_format(RdfFormat::Turtle).for_slice(ttl.as_bytes()) {
        let q = q.map_err(|e| anyhow::anyhow!("claims: {e}"))?;
        let key = match &q.subject {
            NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
            NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
        };
        by_subject
            .entry(key)
            .or_default()
            .push((q.predicate.as_str().to_string(), q.object.clone()));
    }
    let mut out = Vec::new();
    for (subject, ps) in by_subject {
        let is_claim = ps.iter().any(|(p, o)| {
            p == RDF_TYPE && matches!(o, Term::NamedNode(n) if n.as_str() == format!("{CODE}Claim"))
        });
        if !is_claim {
            continue;
        }
        let about = ps.iter().find_map(|(p, o)| match o {
            Term::NamedNode(n) if p == &format!("{CODE}about") => Some(n.as_str().to_string()),
            _ => None,
        });
        let made_against = ps.iter().find_map(|(p, o)| match o {
            Term::Literal(l) if p == &format!("{CODE}madeAgainst") => Some(l.value().to_string()),
            _ => None,
        });
        let (Some(about), Some(made_against)) = (about, made_against) else {
            anyhow::bail!("claims: {subject}: a claim needs code:about (a function) and code:madeAgainst (its definition sha256)");
        };
        let props = ps
            .into_iter()
            .filter(|(p, o)| {
                p.starts_with(CODE)
                    && !p.ends_with("#about")
                    && !p.ends_with("#madeAgainst")
                    && !matches!(o, Term::BlankNode(_))
            })
            .collect();
        out.push(Claim {
            about,
            made_against,
            props,
        });
    }
    Ok(out)
}

/// Claims into complexity.nt, each with its current status.
pub fn claim_triples(claims: &[Claim], m: &Model, out: &mut TripleSet) -> ClaimStats {
    use crate::ontology::{Iri, Literal, Term as T, Triple, OC};
    use sha2::{Digest, Sha256};
    let mut st = ClaimStats::default();
    for c in claims {
        let found = m.index_of(&c.about);
        let current = found.is_some_and(|i| {
            m.fns[i]
                .defs
                .iter()
                .any(|d| d.definition_sha256.as_deref() == Some(c.made_against.as_str()))
        });
        if current {
            st.current += 1;
        } else {
            st.stale += 1;
            if found.is_none() {
                st.missing += 1;
            }
        }
        let mut lines: Vec<String> = c.props.iter().map(|(p, o)| format!("{p} {o}")).collect();
        lines.push(format!("madeAgainst {}", c.made_against));
        lines.sort();
        let digest: String = Sha256::digest(lines.join("\n").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let ci = Iri::new(format!("{}/claim/{}", c.about, &digest[..32]));
        out.add(Triple::type_of(ci.clone(), OC.iri("Claim")));
        out.add(Triple::new(
            ci.clone(),
            OC.iri("about"),
            Iri::new(c.about.as_str()),
        ));
        out.add(Triple::new(
            ci.clone(),
            OC.iri("madeAgainst"),
            Literal::string(c.made_against.as_str()),
        ));
        out.add(Triple::new(
            ci.clone(),
            OC.iri("claimStatus"),
            OC.iri(if current { "Current" } else { "Stale" }),
        ));
        for (p, o) in &c.props {
            let object: T = match o {
                oxigraph::model::Term::NamedNode(n) => Iri::new(n.as_str()).into(),
                oxigraph::model::Term::Literal(l) if l.datatype().as_str().ends_with("#string") => {
                    Literal::string(l.value()).into()
                }
                oxigraph::model::Term::Literal(l) => Literal::Typed {
                    value: l.value().to_string(),
                    datatype: Iri::new(l.datatype().as_str()),
                }
                .into(),
                _ => continue,
            };
            out.add(Triple::new(ci.clone(), Iri::new(p.as_str()), object));
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complexity::model::model_add_scanned;
    use crate::complexity::scan::scan_file;
    use crate::complexity::Grammar;
    use crate::ontology::Iri;

    const OC: &str = "https://ontosys.io/ns/code#";

    fn model_of(src: &str) -> (Model, String) {
        let facts = scan_file("f.py", src.as_bytes(), Grammar::Python).0;
        let sha = facts[0].definition_sha256.clone().unwrap();
        let mut m = Model::default();
        model_add_scanned(
            &mut m,
            facts
                .into_iter()
                .map(|f| (Iri::new("urn:t:f"), f))
                .collect(),
        );
        (m, sha)
    }

    fn claim(about: &str, sha: &str) -> String {
        format!(
            "@prefix code: <{OC}> .\n[] a code:Claim ; code:about <{about}> ; code:madeAgainst \"{sha}\" ;\n  code:upperClass code:ON2 ; code:lowerClass code:O1 ; code:sizeMeaning \"n = len(xs)\" ;\n  code:madeBy \"a reader\" ; code:madeAt \"2026-11-09\" .\n"
        )
    }

    fn emit(ttl: &str, m: &Model) -> (ClaimStats, String) {
        let claims = parse_claims(ttl).unwrap();
        let mut out = TripleSet::new();
        let st = claim_triples(&claims, m, &mut out);
        out.canonicalize();
        (st, out.to_ntriples())
    }

    const SRC: &str = "def f(xs):\n    for x in xs:\n        for y in xs:\n            pass\n";

    #[test]
    fn claim_is_current_while_definition_unchanged() {
        let (m, sha) = model_of(SRC);
        let (st, nt) = emit(&claim("urn:t:f", &sha), &m);
        assert_eq!(
            st,
            ClaimStats {
                current: 1,
                stale: 0,
                missing: 0
            }
        );
        assert!(
            nt.contains(&format!("<{OC}claimStatus> <{OC}Current>")),
            "{nt}"
        );
        assert!(nt.contains(&format!("<{OC}upperClass> <{OC}ON2>")));
        assert!(
            nt.contains("<urn:t:f/claim/"),
            "the claim's IRI hangs off its function"
        );
        assert!(nt.contains(&format!("<{OC}about> <urn:t:f>")));
        assert!(crate::ontology::strict_violations(&nt).is_empty());
    }

    #[test]
    fn whitespace_edit_makes_claim_stale() {
        let (_, sha) = model_of(SRC);
        // One more space inside the definition (trailing spaces after its
        // last token are outside the span and change nothing).
        let (m2, _) = model_of(&SRC.replace("for y in xs", "for y in  xs"));
        let (st, nt) = emit(&claim("urn:t:f", &sha), &m2);
        assert_eq!(
            st,
            ClaimStats {
                current: 0,
                stale: 1,
                missing: 0
            }
        );
        assert!(nt.contains(&format!("<{OC}claimStatus> <{OC}Stale>")));
    }

    #[test]
    fn claim_about_missing_function_is_stale_and_counted() {
        let (m, sha) = model_of(SRC);
        let (st, nt) = emit(&claim("urn:t:gone", &sha), &m);
        assert_eq!(
            st,
            ClaimStats {
                current: 0,
                stale: 1,
                missing: 1
            }
        );
        assert!(nt.contains(&format!("<{OC}claimStatus> <{OC}Stale>")));
    }

    #[test]
    fn no_claims_is_fine_and_bad_turtle_names_its_line() {
        assert!(parse_claims("").unwrap().is_empty());
        let err =
            parse_claims("@prefix code: <https://ontosys.io/ns/code#> .\n\n[] a code:Claim ;; x\n")
                .unwrap_err();
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn two_claims_get_two_iris() {
        let (m, sha) = model_of(SRC);
        let two = format!(
            "{}{}",
            claim("urn:t:f", &sha),
            claim("urn:t:f", &sha).replace("a reader", "another reader")
        );
        let (st, nt) = emit(&two, &m);
        assert_eq!(st.current, 2);
        let iris: std::collections::BTreeSet<&str> = nt
            .lines()
            .filter(|l| l.contains("/claim/"))
            .map(|l| l.split(' ').next().unwrap())
            .collect();
        assert_eq!(iris.len(), 2);
    }
}
