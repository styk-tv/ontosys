//! # Cost catalog
//!
//! What external operations cost, worst case first: the shipped
//! `ontology/cost-catalog.ttl` (C libc, Python, Rust, JavaScript/TypeScript)
//! and a scanned repository's own `.ontosys/cost-catalog.ttl`, which may add
//! project functions and caps on sizes. Nothing here is project-specific.

use super::cost::Cost;
use super::Grammar;
use std::collections::{BTreeMap, HashMap};

/// Which size an entry's cost grows with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeArg {
    Arg(usize),
    Receiver,
}

#[derive(Debug, Clone)]
pub struct CostEntry {
    /// Language family; JavaScript entries serve TypeScript and TSX.
    pub family: u8,
    pub name: String,
    pub receiver: Option<String>,
    pub worst: Cost,
    pub average: Option<Cost>,
    pub amortized: Option<Cost>,
    pub size_arg: Option<SizeArg>,
    /// The cost grows with the output written (printf and friends).
    pub output: bool,
    /// The call runs a callback once per element of this argument.
    pub iterates: Option<SizeArg>,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub version: String,
    pub entries: Vec<CostEntry>,
    by_key: HashMap<(u8, String), Vec<usize>>,
    /// Size name → the name that caps it (`npos` ≤ `MAXNUMPOS`).
    pub caps: BTreeMap<String, String>,
}

pub fn family(g: Grammar) -> u8 {
    match g {
        Grammar::C => 0,
        Grammar::Rust => 1,
        Grammar::Python => 2,
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => 3,
    }
}

/// The catalog shipped with this ontosys.
pub fn shipped() -> Catalog {
    Catalog::default()
        .with_overlay(include_str!("../../ontology/cost-catalog.ttl"))
        .expect("the shipped cost catalog parses")
}

const CODE: &str = "https://ontosys.io/ns/code#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const VERSION_INFO: &str = "http://www.w3.org/2002/07/owl#versionInfo";

fn class_cost(iri: &str) -> Option<Cost> {
    let (n, l) = (Cost::n(), Cost::log_n());
    Some(match iri.strip_prefix(CODE)? {
        "O1" => Cost::ONE,
        "OLogN" => l,
        "ON" => n,
        "ONLogN" => n.mul(l),
        "ON2" => n.mul(n),
        "ON3" => n.mul(n).mul(n),
        _ => return None,
    })
}

fn size_arg(s: &str) -> Option<SizeArg> {
    match s {
        "receiver" => Some(SizeArg::Receiver),
        _ => s
            .strip_prefix("arg")
            .and_then(|i| i.parse().ok())
            .map(SizeArg::Arg),
    }
}

impl Catalog {
    /// Add entries and size caps from Turtle (the shipped catalog, or a
    /// repository's overlay). A parse error names its line.
    pub fn with_overlay(mut self, ttl: &str) -> anyhow::Result<Catalog> {
        use oxigraph::io::{RdfFormat, RdfParser};
        use oxigraph::model::{NamedOrBlankNode, Term};
        let mut by_subject: BTreeMap<String, Vec<(String, Term)>> = BTreeMap::new();
        for q in RdfParser::from_format(RdfFormat::Turtle).for_slice(ttl.as_bytes()) {
            let q = q.map_err(|e| anyhow::anyhow!("cost catalog: {e}"))?;
            let key = match &q.subject {
                NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
                NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
            };
            by_subject
                .entry(key)
                .or_default()
                .push((q.predicate.as_str().to_string(), q.object.clone()));
        }
        let value = |ps: &[(String, Term)], p: &str| -> Option<String> {
            ps.iter().find(|(k, _)| k == p).map(|(_, t)| match t {
                Term::Literal(l) => l.value().to_string(),
                Term::NamedNode(n) => n.as_str().to_string(),
                other => other.to_string(),
            })
        };
        for (subject, ps) in &by_subject {
            if let Some(v) = value(ps, VERSION_INFO) {
                self.version = v;
            }
            let ty = value(ps, RDF_TYPE).unwrap_or_default();
            let code = |local: &str| value(ps, &format!("{CODE}{local}"));
            if ty == format!("{CODE}SizeCap") {
                if let (Some(n), Some(c)) = (code("sizeName"), code("cappedBy")) {
                    self.caps.insert(n, c);
                }
            } else if ty == format!("{CODE}CostEntry") {
                let lang = code("language").unwrap_or_default();
                let family = match lang.as_str() {
                    "c" => 0,
                    "rust" => 1,
                    "python" => 2,
                    "javascript" | "typescript" => 3,
                    other => anyhow::bail!("cost catalog: {subject}: unknown language {other:?}"),
                };
                let worst = code("worstClass").and_then(|c| class_cost(&c));
                let (Some(name), Some(worst)) = (code("operation"), worst) else {
                    anyhow::bail!("cost catalog: {subject}: an entry needs code:operation and a code:worstClass");
                };
                self.entries.push(CostEntry {
                    family,
                    name,
                    receiver: code("receiverType"),
                    worst,
                    average: code("averageClass").and_then(|c| class_cost(&c)),
                    amortized: code("amortizedClass").and_then(|c| class_cost(&c)),
                    size_arg: code("sizeOf").and_then(|s| size_arg(&s)),
                    output: code("sizeOf").as_deref() == Some("output"),
                    iterates: code("iterates").and_then(|s| size_arg(&s)),
                    source: code("source").unwrap_or_default(),
                });
            }
        }
        self.by_key.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.by_key
                .entry((e.family, e.name.clone()))
                .or_default()
                .push(i);
        }
        Ok(self)
    }

    /// Every entry for `name` in `g`'s family (all receiver types: the
    /// receiver's type is rarely known, so callers take the worst).
    pub fn lookup(&self, g: Grammar, name: &str) -> Vec<&CostEntry> {
        self.by_key
            .get(&(family(g), name.to_string()))
            .map(|v| v.iter().map(|&i| &self.entries[i]).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_catalog_parses_and_every_entry_is_complete() {
        let c = shipped();
        assert!(c.entries.len() >= 120, "{}", c.entries.len());
        assert!(!c.version.is_empty());
        for e in &c.entries {
            assert!(!e.name.is_empty() && !e.source.is_empty(), "{e:?}");
        }
        for g in [
            Grammar::C,
            Grammar::Python,
            Grammar::Rust,
            Grammar::JavaScript,
        ] {
            assert!(
                c.entries.iter().any(|e| e.family == family(g)),
                "{g:?} has entries"
            );
        }
    }

    #[test]
    fn strlen_is_linear_in_arg_0() {
        let c = shipped();
        let e = c.lookup(Grammar::C, "strlen");
        assert_eq!(e.len(), 1);
        assert_eq!(
            (e[0].worst, e[0].size_arg.clone()),
            (Cost::n(), Some(SizeArg::Arg(0)))
        );
    }

    #[test]
    fn qsort_is_n_log_n_in_arg_1() {
        let c = shipped();
        let e = c.lookup(Grammar::C, "qsort");
        // POSIX does not bound qsort; quicksort-based libcs are O(n^2) worst.
        assert_eq!(
            (e[0].worst, e[0].size_arg.clone()),
            (Cost::n().mul(Cost::n()), Some(SizeArg::Arg(1)))
        );
        assert_eq!(e[0].average, Some(Cost::n().mul(Cost::log_n())));
    }

    #[test]
    fn python_contains_by_receiver_and_max_without() {
        let c = shipped();
        let e = c.lookup(Grammar::Python, "__contains__");
        let by: BTreeMap<&str, Cost> = e
            .iter()
            .map(|x| (x.receiver.as_deref().unwrap_or(""), x.worst))
            .collect();
        assert_eq!(by["list"], Cost::n());
        assert_eq!(by["set"], Cost::n(), "hash lookup: O(n) worst");
        let set = e
            .iter()
            .find(|x| x.receiver.as_deref() == Some("set"))
            .unwrap();
        assert_eq!(
            set.average,
            Some(Cost::ONE),
            "and O(1) average, recorded as an assumption when used"
        );
        assert_eq!(
            e.iter().map(|x| x.worst).fold(Cost::ONE, Cost::max),
            Cost::n(),
            "unknown receiver: the worst"
        );
        let js = c.lookup(Grammar::TypeScript, "includes");
        assert!(!js.is_empty(), "JavaScript entries serve TypeScript");
        let push = &c.lookup(Grammar::Rust, "push")[0];
        assert_eq!(
            (push.worst, push.amortized),
            (Cost::n(), Some(Cost::ONE)),
            "Vec::push: O(n) worst, O(1) amortized"
        );
        let map = &c.lookup(Grammar::JavaScript, "map")[0];
        assert_eq!(map.iterates, Some(SizeArg::Receiver));
    }

    #[test]
    fn overlay_adds_a_size_cap_and_a_project_function() {
        let c = shipped()
            .with_overlay(
                "@prefix code: <https://ontosys.io/ns/code#> .\n\
                 [] a code:SizeCap ; code:sizeName \"npos\" ; code:cappedBy \"MAXNUMPOS\" .\n\
                 [] a code:CostEntry ; code:language \"c\" ; code:operation \"find_wordentry\" ; code:worstClass code:OLogN ; code:sizeOf \"arg1\" ; code:source \"read 2026-11-09\" .\n",
            )
            .unwrap();
        assert_eq!(c.caps.get("npos").map(String::as_str), Some("MAXNUMPOS"));
        assert_eq!(
            c.lookup(Grammar::C, "find_wordentry")[0].worst,
            Cost::log_n()
        );
    }

    #[test]
    fn overlay_parse_error_names_the_line() {
        let err = shipped()
            .with_overlay(
                "@prefix code: <https://ontosys.io/ns/code#> .\n\n[] a code:SizeCap ;; oops\n",
            )
            .unwrap_err();
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn catalog_ttl_declares_no_terms() {
        let ttl = include_str!("../../ontology/cost-catalog.ttl");
        for kind in [
            "owl:Class",
            "owl:ObjectProperty",
            "owl:DatatypeProperty",
            "rdf:Property",
        ] {
            assert!(
                !ttl.contains(&format!("a {kind}")),
                "the catalog is data; terms live in code.ttl"
            );
        }
    }
}
