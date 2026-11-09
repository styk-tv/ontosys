//! # Call model
//!
//! Every analysed function, every definition of it (C `#ifdef` alternatives
//! share one IRI until the identity rule lands), and what each call site
//! resolves to. Tiers B and C read this; nothing here is emitted directly.

use super::engine::{CallKind, TierA};
use super::scan::FunctionFacts;
use super::Grammar;
use crate::ontology::Iri;
use std::collections::HashMap;

/// What a call site resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A function in the model.
    Repo(usize),
    /// C: no function of that name, but macro definitions (their IRIs).
    Macro(Vec<Iri>),
    /// Nothing in the repository carries that name.
    External(String),
    /// Several repository functions carry that name.
    Ambiguous(Vec<usize>),
    /// A call through a pointer, callback or computed callee.
    Indirect,
}

#[derive(Debug, Clone)]
pub struct DefRec {
    pub tier_a: TierA,
    /// Parallel to `tier_a.loops`.
    pub loops: Vec<super::loops::LoopFact>,
    pub definition_sha256: Option<String>,
    /// Parallel to `tier_a.calls`.
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone)]
pub struct FnRec {
    pub iri: Iri,
    pub name: String,
    pub file: String,
    pub grammar: Grammar,
    pub defs: Vec<DefRec>,
}

#[derive(Debug, Default)]
pub struct Model {
    pub fns: Vec<FnRec>,
    by_iri: HashMap<String, usize>,
}

impl Model {
    pub fn index_of(&self, iri: &str) -> Option<usize> {
        self.by_iri.get(iri).copied()
    }

    /// Add a definition, merging into an existing function of the same IRI.
    pub fn add_def(
        &mut self,
        iri: Iri,
        name: &str,
        file: &str,
        grammar: Grammar,
        def: DefRec,
    ) -> usize {
        if let Some(&i) = self.by_iri.get(iri.as_str()) {
            self.fns[i].defs.push(def);
            return i;
        }
        let i = self.fns.len();
        self.by_iri.insert(iri.as_str().to_string(), i);
        self.fns.push(FnRec {
            iri,
            name: name.to_string(),
            file: file.to_string(),
            grammar,
            defs: vec![def],
        });
        i
    }
}

/// The C functions of a build, with their calls resolved like `cx:calls`.
pub fn model_from_c(
    project: &str,
    files: &[crate::lang_c::CFile],
    ix: &crate::lang_c::rdf::CIndex,
) -> Model {
    use crate::lang_c::rdf::fn_iri;
    use crate::ontology::instance_iri;
    let mut macros: HashMap<&str, Vec<Iri>> = HashMap::new();
    for f in files {
        for mac in &f.macros {
            macros
                .entry(mac.name.as_str())
                .or_default()
                .push(instance_iri(
                    project,
                    "macro",
                    &format!("{}/{}", f.path, mac.name),
                ));
        }
    }
    let mut m = Model::default();
    let mut params: Vec<Vec<Vec<String>>> = Vec::new();
    for f in files {
        for func in &f.functions {
            let def = DefRec {
                tier_a: func.tier_a.clone(),
                loops: func.loops.clone(),
                definition_sha256: func.definition_sha256.clone(),
                targets: Vec::new(),
            };
            let i = m.add_def(
                fn_iri(project, &f.path, &func.name),
                &func.name,
                &f.path,
                Grammar::C,
                def,
            );
            if i == params.len() {
                params.push(Vec::new());
            }
            params[i].push(func.params.iter().map(|p| p.name.clone()).collect());
        }
    }
    for i in 0..m.fns.len() {
        let file = m.fns[i].file.clone();
        for d in 0..m.fns[i].defs.len() {
            let own_params = &params[i][d];
            let targets = m.fns[i].defs[d]
                .tier_a
                .calls
                .iter()
                .map(|c| match c.kind {
                    // A call through one of the function's own parameters is a callback.
                    CallKind::Direct if own_params.contains(&c.callee) => Target::Indirect,
                    CallKind::Direct => match ix
                        .resolve_fn(&c.callee, &file)
                        .and_then(|d| m.index_of(d.iri.as_str()))
                    {
                        Some(j) => Target::Repo(j),
                        None => match macros.get(c.callee.as_str()) {
                            Some(v) => Target::Macro(v.clone()),
                            None => Target::External(c.callee.clone()),
                        },
                    },
                    _ => Target::Indirect,
                })
                .collect();
            m.fns[i].defs[d].targets = targets;
        }
    }
    m
}

fn family(g: Grammar) -> u8 {
    match g {
        Grammar::C => 0,
        Grammar::Rust => 1,
        Grammar::Python => 2,
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => 3,
    }
}

/// Add the attached Rust, Python and JS/TS functions. A call resolves to a
/// repository function only when exactly one function of that name exists in
/// the same language family (JavaScript, TypeScript and TSX are one family).
pub fn model_add_scanned(m: &mut Model, attached: Vec<(Iri, FunctionFacts)>) {
    let first = m.fns.len();
    for (iri, f) in attached {
        let def = DefRec {
            tier_a: f.tier_a,
            loops: f.loops,
            definition_sha256: f.definition_sha256,
            targets: Vec::new(),
        };
        m.add_def(iri, &f.name, &f.file, f.grammar, def);
    }
    let mut by_name: HashMap<(u8, &str), Vec<usize>> = HashMap::new();
    for (i, f) in m.fns.iter().enumerate().skip(first) {
        by_name
            .entry((family(f.grammar), f.name.as_str()))
            .or_default()
            .push(i);
    }
    let resolved: Vec<Vec<Vec<Target>>> = m.fns[first..]
        .iter()
        .map(|f| {
            f.defs
                .iter()
                .map(|d| {
                    d.tier_a
                        .calls
                        .iter()
                        .map(|c| match c.kind {
                            CallKind::Direct | CallKind::Method => {
                                match by_name.get(&(family(f.grammar), c.callee.as_str())) {
                                    Some(v) if v.len() == 1 => Target::Repo(v[0]),
                                    Some(v) => Target::Ambiguous(v.clone()),
                                    None => Target::External(c.callee.clone()),
                                }
                            }
                            CallKind::Operator | CallKind::Macro => {
                                Target::External(c.callee.clone())
                            }
                            CallKind::Indirect => Target::Indirect,
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    for (f, defs) in m.fns[first..].iter_mut().zip(resolved) {
        for (d, targets) in f.defs.iter_mut().zip(defs) {
            d.targets = targets;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complexity::scan::scan_file;

    #[test]
    fn c_model_merges_ifdef_alternatives_and_resolves_like_cx_calls() {
        use crate::lang_c::{extract, new_parser, rdf as crdf};
        let files = vec![
            extract(&mut new_parser(), "m.h", "#define M(x) (x)\n", None),
            extract(
                &mut new_parser(),
                "a.c",
                "#ifdef A\nint f(int x) { return g(M(x)) + printf(\"\"); }\n#else\nint f(int x) { return 0; }\n#endif\nint g(int y) { void (*fp)(void) = 0; (*fp)(); return y; }\nint apply(int (*cmp)(int), int x) { return cmp(x); }\n",
                None,
            ),
        ];
        let ix = crdf::index("p", &files);
        let m = model_from_c("p", &files, &ix);
        let f = m.index_of(crdf::fn_iri("p", "a.c", "f").as_str()).unwrap();
        let g = m.index_of(crdf::fn_iri("p", "a.c", "g").as_str()).unwrap();
        assert_eq!(m.fns[f].defs.len(), 2, "two #ifdef alternatives, one IRI");
        let first = m.fns[f]
            .defs
            .iter()
            .find(|d| !d.targets.is_empty())
            .unwrap();
        let by_callee: HashMap<&str, &Target> = first
            .tier_a
            .calls
            .iter()
            .map(|c| c.callee.as_str())
            .zip(&first.targets)
            .collect();
        assert_eq!(by_callee["g"], &Target::Repo(g));
        assert!(matches!(by_callee["M"], Target::Macro(v) if v.len() == 1));
        assert_eq!(by_callee["printf"], &Target::External("printf".into()));
        assert_eq!(m.fns[g].defs[0].targets, vec![Target::Indirect]);
        let apply = m
            .index_of(crdf::fn_iri("p", "a.c", "apply").as_str())
            .unwrap();
        assert_eq!(
            m.fns[apply].defs[0].targets,
            vec![Target::Indirect],
            "a call through a parameter is indirect"
        );
    }

    #[test]
    fn scanned_model_resolves_by_unique_name_per_family() {
        let a = "def a(xs):\n    b()\n    c()\n    xs.append(1)\n    return a(xs)\n";
        let b = "def b():\n    return 1\n\ndef c():\n    return 2\n";
        let b2 = "def c():\n    return 3\n";
        let mut attached = Vec::new();
        for (file, src) in [("a.py", a), ("b.py", b), ("b2.py", b2)] {
            for f in scan_file(file, src.as_bytes(), Grammar::Python).0 {
                attached.push((Iri::new(format!("urn:t:{file}/{}", f.name)), f));
            }
        }
        let mut m = Model::default();
        model_add_scanned(&mut m, attached);
        let a = m.index_of("urn:t:a.py/a").unwrap();
        let b = m.index_of("urn:t:b.py/b").unwrap();
        let d = &m.fns[a].defs[0];
        let by: HashMap<&str, &Target> = d
            .tier_a
            .calls
            .iter()
            .map(|c| c.callee.as_str())
            .zip(&d.targets)
            .collect();
        assert_eq!(by["b"], &Target::Repo(b));
        assert!(matches!(by["c"], Target::Ambiguous(v) if v.len() == 2));
        assert_eq!(by["append"], &Target::External("append".into()));
        assert_eq!(
            by["a"],
            &Target::Repo(a),
            "self-recursion resolves to itself"
        );
    }
}
