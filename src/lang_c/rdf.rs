//! # C model → RDF
//!
//! Emits the language-level graph (files, functions, types, macros, calls) and
//! builds the symbol index that grounding layers use to attach meaning.
//!
//! IRIs are `instance_iri(project, kind, "<repo path>/<name>")`, so the same
//! definition in two checkouts of the project gets the same IRI.

use super::{CFile, CFunction};
use crate::ontology::*;
use std::collections::{BTreeMap, BTreeSet};

/// Where a definition lives, for resolution.
#[derive(Debug, Clone)]
pub struct Def {
    pub iri: Iri,
    pub path: String,
    pub component: String,
    pub is_static: bool,
    pub is_header: bool,
}

/// Symbol tables over all parsed files.
#[derive(Debug, Default)]
pub struct CIndex {
    pub functions: BTreeMap<String, Vec<Def>>,
    pub structs: BTreeMap<String, Vec<Def>>,
    pub enums: BTreeMap<String, Vec<Def>>,
    pub globals: BTreeMap<String, Vec<Def>>,
    pub files: BTreeMap<String, Iri>,
}

impl CIndex {
    /// Resolve a function name as seen from `from_path`.
    ///
    /// Order: static in the same file → static inline in a header → the single
    /// non-static definition in the same component → the single one in shared
    /// code → the single one anywhere. Ambiguity yields None.
    pub fn resolve_fn(&self, name: &str, from_path: &str) -> Option<&Def> {
        let defs = self.functions.get(name)?;
        if let Some(d) = defs.iter().find(|d| d.is_static && d.path == from_path) {
            return Some(d);
        }
        let header_inline: Vec<&Def> = defs.iter().filter(|d| d.is_static && d.is_header).collect();
        if header_inline.len() == 1 {
            return Some(header_inline[0]);
        }
        let globals: Vec<&Def> = defs.iter().filter(|d| !d.is_static).collect();
        let comp = component_of(from_path);
        let same: Vec<&Def> = globals.iter().copied().filter(|d| d.component == comp).collect();
        if same.len() == 1 {
            return Some(same[0]);
        }
        let shared: Vec<&Def> = globals.iter().copied().filter(|d| is_shared_component(&d.component)).collect();
        if same.is_empty() && shared.len() == 1 {
            return Some(shared[0]);
        }
        if globals.len() == 1 {
            return Some(globals[0]);
        }
        None
    }

    /// Non-static definitions of `name`, preferring the given component.
    pub fn global_defs(&self, name: &str, prefer_component: &str) -> Vec<&Def> {
        let Some(defs) = self.functions.get(name) else { return vec![] };
        let globals: Vec<&Def> = defs.iter().filter(|d| !d.is_static).collect();
        let pref: Vec<&Def> = globals.iter().copied().filter(|d| d.component == prefer_component).collect();
        if pref.is_empty() { globals } else { pref }
    }

    /// A struct by name, preferring a header definition.
    pub fn struct_def(&self, name: &str) -> Option<&Def> {
        let defs = self.structs.get(name)?;
        defs.iter().find(|d| d.is_header).or_else(|| if defs.len() == 1 { defs.first() } else { None })
    }
}

/// The build unit a path belongs to (used to disambiguate same-named symbols
/// linked into different programs).
pub fn component_of(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').collect();
    let depth = match segs.as_slice() {
        ["src", "backend", ..] => 2,
        ["src", "bin" | "interfaces" | "pl", ..] => 3,
        ["src", "test", "modules", ..] => 4,
        ["src", ..] => 2,
        _ => 2,
    };
    let dirs = &segs[..segs.len().saturating_sub(1)];
    dirs[..depth.min(dirs.len())].join("/")
}

fn is_shared_component(c: &str) -> bool {
    matches!(c, "src/common" | "src/port" | "src/fe_utils" | "src/include" | "src/timezone")
}

fn cx(local: &str) -> Iri {
    CX.iri(local)
}

fn lit(s: impl Into<String>) -> Term {
    Term::Literal(Literal::string(s))
}

fn int(n: usize) -> Term {
    Term::Literal(Literal::integer(n as i64))
}

fn boolean(b: bool) -> Term {
    Term::Literal(Literal::boolean(b))
}

pub fn file_iri(project: &str, path: &str) -> Iri {
    instance_iri(project, "file", path)
}

pub fn fn_iri(project: &str, path: &str, name: &str) -> Iri {
    instance_iri(project, "fn", &format!("{}/{}", path, name))
}

/// Build the symbol index over all files.
pub fn index(project: &str, files: &[CFile]) -> CIndex {
    let mut ix = CIndex::default();
    for f in files {
        ix.files.insert(f.path.clone(), file_iri(project, &f.path));
        let comp = component_of(&f.path);
        let def = |iri: Iri, is_static: bool| Def {
            iri,
            path: f.path.clone(),
            component: comp.clone(),
            is_static,
            is_header: f.is_header,
        };
        for func in &f.functions {
            ix.functions.entry(func.name.clone()).or_default().push(def(fn_iri(project, &f.path, &func.name), func.is_static));
        }
        for s in &f.structs {
            ix.structs.entry(s.name.clone()).or_default().push(def(instance_iri(project, "struct", &format!("{}/{}", f.path, s.name)), false));
        }
        for e in &f.enums {
            ix.enums.entry(e.name.clone()).or_default().push(def(instance_iri(project, "enum", &format!("{}/{}", f.path, e.name)), false));
        }
        for g in &f.globals {
            ix.globals.entry(g.name.clone()).or_default().push(def(instance_iri(project, "var", &format!("{}/{}", f.path, g.name)), g.is_static));
        }
    }
    for defs in ix.functions.values_mut() {
        defs.sort_by(|a, b| a.iri.as_str().cmp(b.iri.as_str()));
        defs.dedup_by(|a, b| a.iri == b.iri);
    }
    ix
}

/// Resolve `#include "x/y.h"` to a repository header path.
fn resolve_include(inc: &str, from: &str, headers: &BTreeSet<&str>) -> Option<String> {
    let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let candidates = [format!("src/include/{}", inc), format!("{}/{}", dir, inc), inc.to_string()];
    for c in &candidates {
        if headers.contains(c.as_str()) {
            return Some(c.clone());
        }
    }
    let suffix = format!("/{}", inc);
    let hits: Vec<&&str> = headers.iter().filter(|h| h.ends_with(&suffix)).collect();
    if hits.len() == 1 {
        return Some(hits[0].to_string());
    }
    None
}

/// Emit the language-level graph for all files.
pub fn to_triples(project: &str, files: &[CFile], ix: &CIndex, out: &mut TripleSet) {
    let headers: BTreeSet<&str> = files.iter().filter(|f| f.is_header).map(|f| f.path.as_str()).collect();

    for f in files {
        let fi = file_iri(project, &f.path);
        out.add(Triple::type_of(fi.clone(), cx(if f.is_header { "HeaderFile" } else { "SourceFile" })));
        out.add(Triple::label(fi.clone(), &f.path));
        out.add(Triple::new(fi.clone(), cx("path"), lit(&f.path)));
        out.add(Triple::new(fi.clone(), cx("parseErrors"), int(f.parse_errors)));
        for inc in &f.includes {
            let target = match resolve_include(inc, &f.path, &headers) {
                Some(p) => file_iri(project, &p),
                None => {
                    let sys = instance_iri(project, "sysheader", inc);
                    out.add(Triple::type_of(sys.clone(), cx("SystemHeader")));
                    out.add(Triple::label(sys.clone(), inc));
                    sys
                }
            };
            out.add(Triple::new(fi.clone(), cx("includes"), target));
        }

        for func in &f.functions {
            function_triples(project, f, func, ix, out);
        }

        for p in &f.prototypes {
            let pi = instance_iri(project, "decl", &format!("{}/{}", f.path, p.name));
            out.add(Triple::type_of(pi.clone(), cx("Prototype")));
            out.add(Triple::label(pi.clone(), &p.name));
            out.add(Triple::new(pi.clone(), cx("declaredIn"), fi.clone()));
            out.add(Triple::new(pi.clone(), cx("signature"), lit(&p.signature)));
            out.add(Triple::new(pi.clone(), cx("line"), int(p.line)));
            for d in ix.global_defs(&p.name, &component_of(&f.path)).into_iter().take(32) {
                out.add(Triple::new(pi.clone(), cx("declares"), d.iri.clone()));
            }
        }

        for s in &f.structs {
            let si = instance_iri(project, "struct", &format!("{}/{}", f.path, s.name));
            out.add(Triple::type_of(si.clone(), cx(if s.is_union { "Union" } else { "Struct" })));
            out.add(Triple::label(si.clone(), &s.name));
            out.add(Triple::new(si.clone(), cx("definedIn"), fi.clone()));
            out.add(Triple::new(si.clone(), cx("line"), int(s.line)));
            if let Some(c) = &s.comment {
                out.add(Triple::new(si.clone(), rdfs::comment(), lit(c)));
            }
            for (i, fld) in s.fields.iter().enumerate() {
                let fli = Iri::new(format!("{}/field/{}", si.as_str(), iri_segment(&fld.name)));
                out.add(Triple::new(si.clone(), cx("hasField"), fli.clone()));
                out.add(Triple::type_of(fli.clone(), cx("Field")));
                out.add(Triple::label(fli.clone(), &fld.name));
                out.add(Triple::new(fli.clone(), cx("index"), int(i)));
                out.add(Triple::new(fli.clone(), cx("typeText"), lit(&fld.type_text)));
                if let Some(c) = &fld.comment {
                    out.add(Triple::new(fli.clone(), rdfs::comment(), lit(c)));
                }
                if let Some(target) = base_type(&fld.type_text).and_then(|t| ix.struct_def(&t)) {
                    out.add(Triple::new(fli.clone(), cx("fieldType"), target.iri.clone()));
                }
            }
        }

        for e in &f.enums {
            let ei = instance_iri(project, "enum", &format!("{}/{}", f.path, e.name));
            out.add(Triple::type_of(ei.clone(), cx("Enum")));
            out.add(Triple::label(ei.clone(), &e.name));
            out.add(Triple::new(ei.clone(), cx("definedIn"), fi.clone()));
            out.add(Triple::new(ei.clone(), cx("line"), int(e.line)));
            if let Some(c) = &e.comment {
                out.add(Triple::new(ei.clone(), rdfs::comment(), lit(c)));
            }
            for (i, (name, value, comment)) in e.members.iter().enumerate() {
                let mi = Iri::new(format!("{}/member/{}", ei.as_str(), iri_segment(name)));
                out.add(Triple::new(ei.clone(), cx("hasMember"), mi.clone()));
                out.add(Triple::type_of(mi.clone(), cx("EnumMember")));
                out.add(Triple::label(mi.clone(), name));
                out.add(Triple::new(mi.clone(), cx("index"), int(i)));
                if !value.is_empty() {
                    out.add(Triple::new(mi.clone(), cx("value"), lit(value)));
                }
                if let Some(c) = comment {
                    out.add(Triple::new(mi.clone(), rdfs::comment(), lit(c)));
                }
            }
        }

        for t in &f.typedefs {
            let ti = instance_iri(project, "typedef", &format!("{}/{}", f.path, t.name));
            out.add(Triple::type_of(ti.clone(), cx("Typedef")));
            out.add(Triple::label(ti.clone(), &t.name));
            out.add(Triple::new(ti.clone(), cx("definedIn"), fi.clone()));
            out.add(Triple::new(ti.clone(), cx("text"), lit(&t.text)));
            out.add(Triple::new(ti.clone(), cx("line"), int(t.line)));
        }

        for m in &f.macros {
            let mi = instance_iri(project, "macro", &format!("{}/{}", f.path, m.name));
            out.add(Triple::type_of(mi.clone(), cx("Macro")));
            out.add(Triple::label(mi.clone(), &m.name));
            out.add(Triple::new(mi.clone(), cx("definedIn"), fi.clone()));
            out.add(Triple::new(mi.clone(), cx("value"), lit(&m.value)));
            out.add(Triple::new(mi.clone(), cx("line"), int(m.line)));
            if let Some(p) = &m.params {
                out.add(Triple::new(mi.clone(), cx("macroParameters"), lit(p)));
            }
        }

        for g in &f.globals {
            let gi = instance_iri(project, "var", &format!("{}/{}", f.path, g.name));
            out.add(Triple::type_of(gi.clone(), cx("GlobalVariable")));
            out.add(Triple::label(gi.clone(), &g.name));
            out.add(Triple::new(gi.clone(), cx("definedIn"), fi.clone()));
            out.add(Triple::new(gi.clone(), cx("typeText"), lit(&g.type_text)));
            out.add(Triple::new(gi.clone(), cx("isStatic"), boolean(g.is_static)));
            out.add(Triple::new(gi.clone(), cx("line"), int(g.line)));
        }
    }
}

fn function_triples(project: &str, f: &CFile, func: &CFunction, ix: &CIndex, out: &mut TripleSet) {
    let fi = fn_iri(project, &f.path, &func.name);
    out.add(Triple::type_of(fi.clone(), cx("Function")));
    out.add(Triple::label(fi.clone(), &func.name));
    out.add(Triple::new(fi.clone(), cx("definedIn"), file_iri(project, &f.path)));
    out.add(Triple::new(fi.clone(), cx("signature"), lit(&func.signature)));
    out.add(Triple::new(fi.clone(), cx("returnType"), lit(&func.return_type)));
    out.add(Triple::new(
        fi.clone(),
        cx("parameters"),
        lit(func.params.iter().map(|p| if p.name.is_empty() { p.type_text.clone() } else { format!("{} {}", p.type_text, p.name) }).collect::<Vec<_>>().join(", ")),
    ));
    out.add(Triple::new(fi.clone(), cx("arity"), int(func.params.len())));
    out.add(Triple::new(fi.clone(), cx("isStatic"), boolean(func.is_static)));
    if func.is_inline {
        out.add(Triple::new(fi.clone(), cx("isInline"), boolean(true)));
    }
    out.add(Triple::new(fi.clone(), cx("startLine"), int(func.start_line)));
    out.add(Triple::new(fi.clone(), cx("endLine"), int(func.end_line)));
    out.add(Triple::new(fi.clone(), cx("bodyHash"), lit(&func.body_hash)));
    out.add(Triple::new(fi.clone(), cx("statementCount"), int(func.statements)));
    out.add(Triple::new(fi.clone(), cx("complexity"), int(func.complexity)));
    if let Some(c) = &func.comment {
        out.add(Triple::new(fi.clone(), rdfs::comment(), lit(c)));
    }
    for callee in &func.calls {
        let target = match ix.resolve_fn(callee, &f.path) {
            Some(d) => d.iri.clone(),
            None => {
                let sym = instance_iri(project, "sym", callee);
                out.add(Triple::type_of(sym.clone(), cx("ExternalSymbol")));
                out.add(Triple::label(sym.clone(), callee));
                sym
            }
        };
        out.add(Triple::new(fi.clone(), cx("calls"), target));
    }
    for (callee, text) in &func.messages {
        let pred = if callee.starts_with("errdetail") {
            "emitsDetail"
        } else if callee.starts_with("errhint") || callee == "pg_log_error_hint" {
            "emitsHint"
        } else if callee == "errcontext" {
            "emitsContext"
        } else {
            "emitsMessage"
        };
        out.add(Triple::new(fi.clone(), cx(pred), lit(text)));
    }
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for t in func.type_refs.iter().map(|s| s.as_str()).chain(func.params.iter().filter_map(|p| base_type_ref(&p.type_text))) {
        used.insert(t);
    }
    if let Some(t) = base_type_ref(&func.return_type) {
        used.insert(t);
    }
    for t in used {
        if let Some(d) = ix.struct_def(t) {
            out.add(Triple::new(fi.clone(), cx("usesType"), d.iri.clone()));
        }
    }
}

/// `const struct Foo *` → `Foo`
pub fn base_type(type_text: &str) -> Option<String> {
    base_type_ref(type_text).map(|s| s.to_string())
}

fn base_type_ref(type_text: &str) -> Option<&str> {
    type_text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .find(|w| !matches!(*w, "const" | "volatile" | "struct" | "union" | "enum" | "unsigned" | "signed" | "static" | "extern" | "inline" | "restrict"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components() {
        assert_eq!(component_of("src/backend/utils/adt/int.c"), "src/backend");
        assert_eq!(component_of("src/bin/psql/command.c"), "src/bin/psql");
        assert_eq!(component_of("contrib/pg_trgm/trgm_op.c"), "contrib/pg_trgm");
        assert_eq!(component_of("src/common/string.c"), "src/common");
    }

    #[test]
    fn base_types() {
        assert_eq!(base_type("const struct Foo *"), Some("Foo".into()));
        assert_eq!(base_type("PlannerInfo *"), Some("PlannerInfo".into()));
    }
}
