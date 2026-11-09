//! # Join
//!
//! Attaches scanned Tier A facts to the line parsers' declarations: a
//! function attaches to the declaration typed `co:Function` whose file is the
//! same, whose `startLine` is the line of the function's **name**, and whose
//! label is the function's name. Anything else is refused and counted, never
//! guessed. When the line parsers are replaced by tree-sitter parsers that
//! call the engine directly, this module goes away.

use super::scan::FunctionFacts;
use super::Grammar;
use crate::ontology::{Iri, Literal, Term, TripleSet, CO, CODE, RDF, RDFS};
use std::collections::{BTreeMap, HashMap};

/// Why a scanned function was not attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unattached {
    /// No declaration starts on the line of the function's name.
    NoDeclarationOnNameLine,
    /// The declaration on that line carries several start lines (merged
    /// nodes), or several declarations claim it.
    MergedDeclaration,
    /// The declaration on that line has another name.
    NameMismatch,
    /// The file did not parse at all.
    ParseError,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct AttachStats {
    pub analysed: usize,
    pub attached: usize,
    pub not_attached: BTreeMap<Unattached, usize>,
    /// tree-sitter ERROR/MISSING nodes in the scanned files.
    pub error_nodes: usize,
}

#[derive(Default)]
struct Decl {
    is_function: bool,
    file: Option<String>,
    lines: Vec<usize>,
    label: Option<String>,
}

fn literal_text(t: &Term) -> Option<&str> {
    match t {
        Term::Literal(Literal::String(s)) => Some(s),
        Term::Literal(Literal::Typed { value, .. }) => Some(value),
        Term::Literal(Literal::LangString { value, .. }) => Some(value),
        _ => None,
    }
}

/// Attach facts (by repository-relative path) to declarations in `graph`.
pub fn attach(
    graph: &TripleSet,
    per_file: &BTreeMap<String, Vec<FunctionFacts>>,
) -> (Vec<(Iri, FunctionFacts)>, BTreeMap<Grammar, AttachStats>) {
    let (rdf_type, function) = (RDF.iri("type"), CO.iri("Function"));
    let (file_path, start_line, label) = (
        CODE.iri("filePath"),
        CODE.iri("startLine"),
        RDFS.iri("label"),
    );
    let mut decls: HashMap<&str, Decl> = HashMap::new();
    for t in graph.iter() {
        let Term::Iri(s) = &t.subject else { continue };
        let p = &t.predicate;
        if *p == rdf_type {
            if matches!(&t.object, Term::Iri(o) if *o == function) {
                decls.entry(s.as_str()).or_default().is_function = true;
            }
        } else if *p == file_path {
            decls.entry(s.as_str()).or_default().file = literal_text(&t.object).map(str::to_string);
        } else if *p == start_line {
            if let Some(n) = literal_text(&t.object).and_then(|v| v.parse().ok()) {
                decls.entry(s.as_str()).or_default().lines.push(n);
            }
        } else if *p == label {
            decls.entry(s.as_str()).or_default().label =
                literal_text(&t.object).map(str::to_string);
        }
    }
    let mut by_line: HashMap<(&str, usize), Vec<&str>> = HashMap::new();
    for (s, d) in &decls {
        if let (true, Some(file)) = (d.is_function, d.file.as_deref()) {
            for &l in &d.lines {
                by_line.entry((file, l)).or_default().push(s);
            }
        }
    }
    let mut attached = Vec::new();
    let mut stats: BTreeMap<Grammar, AttachStats> = BTreeMap::new();
    for (file, facts) in per_file {
        for f in facts {
            let st = stats.entry(f.grammar).or_default();
            st.analysed += 1;
            let refuse = |st: &mut AttachStats, why| *st.not_attached.entry(why).or_insert(0) += 1;
            let Some(cands) = by_line.get(&(file.as_str(), f.name_line)) else {
                refuse(st, Unattached::NoDeclarationOnNameLine);
                continue;
            };
            let named: Vec<&str> = cands
                .iter()
                .copied()
                .filter(|s| decls[s].label.as_deref() == Some(f.name.as_str()))
                .collect();
            match named.as_slice() {
                [] => refuse(st, Unattached::NameMismatch),
                [one] if decls[one].lines.len() == 1 => {
                    st.attached += 1;
                    attached.push((Iri::new(*one), f.clone()));
                }
                _ => refuse(st, Unattached::MergedDeclaration),
            }
        }
    }
    attached.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    (attached, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complexity::scan::scan_file;
    use crate::graph::GraphBuilder;
    use crate::ontology::{Literal, Triple, CO, CODE, RDF, RDFS};
    use crate::parser::MultiLanguageParser;
    use std::path::Path;

    /// Line-parse and scan `src` as `rel`, then attach.
    fn run(rel: &str, src: &str) -> (Vec<(String, String)>, BTreeMap<Grammar, AttachStats>) {
        let nodes = MultiLanguageParser::new()
            .unwrap()
            .parse_file(Path::new(rel), src)
            .unwrap();
        let graph = GraphBuilder::new("p").build_from_ast(&nodes).unwrap();
        let g = Grammar::for_path(rel).unwrap();
        let mut per_file = BTreeMap::new();
        per_file.insert(rel.to_string(), scan_file(rel, src.as_bytes(), g).0);
        let (attached, stats) = attach(&graph, &per_file);
        (
            attached
                .into_iter()
                .map(|(iri, f)| (f.name, iri.as_str().to_string()))
                .collect(),
            stats,
        )
    }

    #[test]
    fn python_decorated_def_attaches_on_the_def_line() {
        let (got, stats) = run(
            "pkg/m.py",
            "import x\n\n@cache\ndef fib(n):\n    return n\n",
        );
        assert_eq!(got.len(), 1, "{stats:?}");
        assert_eq!(got[0].0, "fib");
        assert!(got[0].1.contains("/fn/fib"), "{}", got[0].1);
        assert_eq!(stats[&Grammar::Python].attached, 1);
    }

    #[test]
    fn rust_fn_with_attribute_and_doc_attaches_on_the_fn_line() {
        let (got, stats) = run(
            "src/a.rs",
            "/// doc\n#[inline]\npub fn run() -> u8 {\n    1\n}\n",
        );
        assert_eq!(
            got.iter().map(|g| g.0.as_str()).collect::<Vec<_>>(),
            vec!["run"],
            "{stats:?}"
        );
    }

    #[test]
    fn ts_name_on_its_own_line_is_not_attached_to_the_wrong_declaration() {
        let src = "export function\n  longName(a: number): number { return a; }\nexport function other() { return 1; }\n";
        let (got, stats) = run("web/a.ts", src);
        for (name, iri) in &got {
            assert!(
                iri.ends_with(&format!("/fn/{name}")) || iri.contains(&format!("/fn/{name}~")),
                "{name} -> {iri}"
            );
        }
        let st = &stats[&Grammar::TypeScript];
        assert_eq!(st.analysed, 2);
        assert_eq!(st.attached + st.not_attached.values().sum::<usize>(), 2);
    }

    fn synthetic(lines: &[i64], label: &str) -> TripleSet {
        let s = Iri::new("urn:t:f");
        let mut t = TripleSet::new();
        t.add(Triple::new(s.clone(), RDF.iri("type"), CO.iri("Function")));
        t.add(Triple::new(
            s.clone(),
            RDFS.iri("label"),
            Literal::string(label),
        ));
        t.add(Triple::new(
            s.clone(),
            CODE.iri("filePath"),
            Literal::string("a.py"),
        ));
        for l in lines {
            t.add(Triple::new(
                s.clone(),
                CODE.iri("startLine"),
                Literal::integer(*l),
            ));
        }
        t
    }

    fn facts_for(src: &str) -> BTreeMap<String, Vec<FunctionFacts>> {
        let mut m = BTreeMap::new();
        m.insert(
            "a.py".to_string(),
            scan_file("a.py", src.as_bytes(), Grammar::Python).0,
        );
        m
    }

    #[test]
    fn merged_declaration_is_refused() {
        let (attached, stats) = attach(
            &synthetic(&[1, 7], "f"),
            &facts_for("def f():\n    return 1\n"),
        );
        assert!(attached.is_empty());
        assert_eq!(
            stats[&Grammar::Python].not_attached[&Unattached::MergedDeclaration],
            1
        );
    }

    #[test]
    fn name_mismatch_is_refused() {
        let (attached, stats) = attach(
            &synthetic(&[1], "g"),
            &facts_for("def f():\n    return 1\n"),
        );
        assert!(attached.is_empty());
        assert_eq!(
            stats[&Grammar::Python].not_attached[&Unattached::NameMismatch],
            1
        );
    }

    #[test]
    fn a_function_on_no_declared_line_is_counted() {
        let (attached, stats) = attach(
            &synthetic(&[5], "f"),
            &facts_for("def f():\n    return 1\n"),
        );
        assert!(attached.is_empty());
        assert_eq!(
            stats[&Grammar::Python].not_attached[&Unattached::NoDeclarationOnNameLine],
            1
        );
        assert_eq!(stats[&Grammar::Python].analysed, 1);
    }
}
