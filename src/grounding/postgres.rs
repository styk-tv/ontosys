//! # PostgreSQL grounding
//!
//! Attaches meaning to the C graph using only files PostgreSQL itself maintains
//! as declarative data. Nothing is inferred from names or guessed by a model:
//!
//! | Source                                  | Grounds                                   |
//! |-----------------------------------------|-------------------------------------------|
//! | directory layout, README, `*.control`   | code areas (subsystems, programs, ...)    |
//! | `src/include/catalog/pg_*.h` CATALOG()  | system catalogs, columns, lookups, indexes|
//! | `src/include/catalog/*.dat`             | built-in functions, types, operators, ... |
//! | `pg_proc.dat` `prosrc`                  | SQL function → implementing C function    |
//! | `guc_parameters.dat`                    | settings → C variable, hooks              |
//! | `errcodes.txt`                          | SQLSTATEs (+ `ERRCODE_*` references)      |
//! | `wait_event_names.txt`                  | wait events (+ `WAIT_EVENT_*` references) |
//! | `lwlocklist.h`                          | LWLocks and tranches                      |
//! | `kwlist.h`                              | SQL keywords                              |
//! | structs whose first field is `NodeTag`  | node types and their inheritance          |
//! | `extern PGDLLIMPORT *_hook_type`        | extension hooks                           |
//!
//! Every function then gets a profile in those terms: which SQL function it
//! implements, which conditions it may raise, which settings it reads, which
//! node types it creates/inspects/operates on, which locks, waits and hooks it
//! touches — the part of "what this body means" that the repository states.

use super::dat::{self, DatRecord};
use crate::lang_c::rdf::{component_of, file_iri, fn_iri, CIndex};
use crate::lang_c::{self, CFile};
use crate::ontology::*;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

/// Classes drawn by the browser view when the full graph is too large to render:
/// the grounded structure of PostgreSQL (code-area tree, catalogs with their
/// columns and lookups, node-type inheritance, SQLSTATE classes) without the
/// tens of thousands of C functions hanging off it.
pub const OVERVIEW_CLASSES: &[&str] = &[
    "CodeArea", "BackendSubsystem", "ClientProgram", "ClientLibrary", "ProceduralLanguage",
    "Extension", "TestModule", "SharedCode", "PublicHeaders", "SystemCatalog", "CatalogColumn",
    "NodeType", "SQLState", "SQLStateClass",
];

pub fn detect(root: &Path) -> bool {
    root.join("src/include/catalog/pg_proc.dat").is_file() && root.join("src/backend").is_dir()
}

const ERASE_WORDS: &[&str] = &[
    "PGDLLIMPORT", "PGDLLEXPORT", "pg_noreturn", "pg_nodiscard", "PG_USED_FOR_ASSERTS_ONLY",
    "pg_node_attr", "pg_restrict",
];
const ERASE_PREFIXES: &[&str] = &["pg_attribute_", "BKI_"];

/// Iteration macros from pg_list.h, ilist.h and friends: `NAME(args) { body }`.
const LOOP_MACROS: &[&str] = &[
    "foreach", "foreach_ptr", "foreach_node", "foreach_int", "foreach_oid", "foreach_xid",
    "foreach_from", "forboth", "forthree", "forfour", "forfive", "for_each_cell", "for_each_from",
    "for_both_cell", "dlist_foreach", "dlist_foreach_modify", "dlist_reverse_foreach", "dclist_foreach",
    "dclist_foreach_modify", "dclist_reverse_foreach", "slist_foreach", "slist_foreach_modify",
];

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("static regex"))
}

static CATALOG_RE: OnceLock<Regex> = OnceLock::new();
fn catalog_re() -> &'static Regex {
    re(&CATALOG_RE, r"(?m)^CATALOG\((\w+),\s*(\d+),\s*(\w+)\)(.*)$")
}

/// Length-preserving rewrite so tree-sitter sees plain C: `CATALOG(pg_x,...)`
/// becomes `struct pg_x`, annotation macros are blanked.
pub fn prepass(src: &str) -> String {
    let mut s = src.to_string();
    if s.contains("CATALOG(") {
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for m in catalog_re().captures_iter(&s) {
            let head = m.get(0).unwrap();
            let macro_end = m.get(3).unwrap().end() + 1; // through the ')'
            let rep = format!("struct {}", &m[1]);
            out.push_str(&s[last..head.start()]);
            out.push_str(&rep);
            out.push_str(&" ".repeat(macro_end - head.start() - rep.len()));
            last = macro_end;
        }
        out.push_str(&s[last..]);
        s = out;
    }
    let s = lang_c::loop_macros_as_while(&s, LOOP_MACROS);
    lang_c::erase_macros(&s, ERASE_WORDS, ERASE_PREFIXES)
}

// ----------------------------------------------------------------------------

struct G<'a> {
    project: &'a str,
    root: &'a Path,
    files: &'a [CFile],
    ix: &'a CIndex,
    out: &'a mut TripleSet,
    stats: BTreeMap<String, usize>,

    sqlstate_by_macro: BTreeMap<String, Iri>,
    wait_by_macro: BTreeMap<String, Iri>,
    lock_by_ident: BTreeMap<String, Iri>,
    tranche_by_ident: BTreeMap<String, Iri>,
    guc_by_var: BTreeMap<String, Vec<Iri>>,
    node_by_name: BTreeMap<String, Iri>,
    hook_by_var: BTreeMap<String, Iri>,
    roles: BTreeMap<String, BTreeSet<&'static str>>,

    proc_keys: BTreeSet<String>,
    proc_keys_by_name: BTreeMap<String, Vec<String>>,
    proc_cfns: BTreeMap<String, Vec<Iri>>,
    type_names: BTreeSet<String>,
}

/// Ground the C graph in PostgreSQL's own declarative sources.
pub fn ground(project: &str, root: &Path, files: &[CFile], ix: &CIndex, out: &mut TripleSet) -> anyhow::Result<BTreeMap<String, usize>> {
    let mut g = G {
        project,
        root,
        files,
        ix,
        out,
        stats: BTreeMap::new(),
        sqlstate_by_macro: BTreeMap::new(),
        wait_by_macro: BTreeMap::new(),
        lock_by_ident: BTreeMap::new(),
        tranche_by_ident: BTreeMap::new(),
        guc_by_var: BTreeMap::new(),
        node_by_name: BTreeMap::new(),
        hook_by_var: BTreeMap::new(),
        roles: BTreeMap::new(),
        proc_keys: BTreeSet::new(),
        proc_keys_by_name: BTreeMap::new(),
        proc_cfns: BTreeMap::new(),
        type_names: BTreeSet::new(),
    };
    g.release();
    g.areas();
    g.catalog_headers();
    g.catalog_rows()?;
    g.settings()?;
    g.sqlstates()?;
    g.wait_events()?;
    g.lwlocks()?;
    g.keywords()?;
    g.node_types();
    g.hooks();
    g.function_profiles();
    Ok(g.stats)
}

fn lit(s: impl Into<String>) -> Term {
    Term::Literal(Literal::string(s))
}

fn pg(local: &str) -> Iri {
    PG.iri(local)
}

impl<'a> G<'a> {
    fn iri(&self, kind: &str, key: &str) -> Iri {
        instance_iri(self.project, &format!("pg/{}", kind), key)
    }

    fn count(&mut self, what: &str) {
        *self.stats.entry(what.to_string()).or_default() += 1;
    }

    fn add(&mut self, s: &Iri, p: Iri, o: impl Into<Term>) {
        self.out.add(Triple::new(s.clone(), p, o));
    }

    fn read(&self, rel: &str) -> Option<String> {
        fs::read(self.root.join(rel)).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn role(&mut self, f: &Iri, role: &'static str) {
        self.roles.entry(f.as_str().to_string()).or_default().insert(role);
    }

    // ---- release ---------------------------------------------------------

    fn release(&mut self) {
        let Some(meson) = self.read("meson.build") else { return };
        static V: OnceLock<Regex> = OnceLock::new();
        if let Some(c) = re(&V, r"(?s)project\(\s*'postgresql'.*?version:\s*'([^']+)'").captures(&meson) {
            let r = self.iri("release", "current");
            self.add(&r, rdf::type_(), pg("Release"));
            self.add(&r, rdfs::label(), lit(format!("PostgreSQL {}", &c[1])));
            self.add(&r, pg("version"), lit(&c[1]));
        }
    }

    // ---- code areas ------------------------------------------------------

    fn areas(&mut self) {
        let mut dirs: BTreeSet<String> = BTreeSet::new();
        for f in self.files {
            let mut d = f.path.as_str();
            while let Some((parent, _)) = d.rsplit_once('/') {
                dirs.insert(parent.to_string());
                d = parent;
            }
        }
        for d in &dirs {
            let a = self.iri("area", d);
            let class = area_class(d);
            self.add(&a, rdf::type_(), pg(class));
            self.add(&a, rdfs::label(), lit(d.as_str()));
            if let Some((parent, _)) = d.rsplit_once('/') {
                let p = self.iri("area", parent);
                self.add(&a, pg("partOf"), p);
            }
            for readme in ["README", "README.md"] {
                if let Some(text) = self.read(&format!("{}/{}", d, readme)) {
                    if let Some(desc) = readme_summary(&text) {
                        self.add(&a, rdfs::comment(), lit(desc));
                        self.add(&a, pg("groundedIn"), lit(format!("{}/{}", d, readme)));
                    }
                    break;
                }
            }
            if class == "Extension" {
                let name = d.rsplit('/').next().unwrap_or(d);
                if let Some(ctl) = self.read(&format!("{}/{}.control", d, name)) {
                    for line in ctl.lines() {
                        if let Some((k, v)) = line.split_once('=') {
                            let v = v.trim().trim_matches('\'').to_string();
                            match k.trim() {
                                "comment" => self.add(&a, rdfs::comment(), lit(v)),
                                "default_version" => self.add(&a, pg("defaultVersion"), lit(v)),
                                "trusted" => self.add(&a, pg("trusted"), lit(v)),
                                "relocatable" => self.add(&a, pg("relocatable"), lit(v)),
                                _ => {}
                            }
                        }
                    }
                }
            }
            self.count("code areas");
        }
        for f in self.files {
            let fi = file_iri(self.project, &f.path);
            if let Some((dir, _)) = f.path.rsplit_once('/') {
                let a = self.iri("area", dir);
                self.add(&fi, pg("inArea"), a);
            }
            if let Some(desc) = f.header_comment.as_deref().and_then(|c| file_description(c, &f.path)) {
                self.add(&fi, pg("fileDescription"), lit(desc));
                self.count("file descriptions");
            }
        }
    }

    // ---- catalog definitions (headers) ----------------------------------

    fn catalog_headers(&mut self) {
        static COL: OnceLock<Regex> = OnceLock::new();
        static IDX: OnceLock<Regex> = OnceLock::new();
        static CACHE: OnceLock<Regex> = OnceLock::new();
        static ANN: OnceLock<Regex> = OnceLock::new();
        let col_re = re(&COL, r"^\s*([A-Za-z_][\w\s\*]*?[\s\*])(\w+)(\[[^\]]*\])?\s*(.*?);");
        let idx_re = re(&IDX, r"DECLARE_(UNIQUE_)?INDEX(_PKEY)?\((\w+),\s*(\d+),\s*(\w+),\s*(\w+),\s*(\w+\(.*\))\);");
        let cache_re = re(&CACHE, r"MAKE_SYSCACHE\((\w+),\s*(\w+),\s*\d+\)");
        let ann_re = re(&ANN, r"BKI_(\w+)(?:\(([^)]*)\))?");

        let headers: Vec<String> = self
            .files
            .iter()
            .filter(|f| f.path.starts_with("src/include/catalog/pg_") && f.path.ends_with(".h"))
            .map(|f| f.path.clone())
            .collect();
        for path in headers {
            let Some(text) = self.read(&path) else { continue };
            let Some(m) = catalog_re().captures(&text) else { continue };
            let name = m[1].to_string();
            let cat = self.iri("catalog", &name);
            self.add(&cat, rdf::type_(), pg("SystemCatalog"));
            self.add(&cat, rdfs::label(), lit(name.as_str()));
            self.add(&cat, pg("catalogOid"), lit(&m[2]));
            self.add(&cat, pg("oidMacro"), lit(&m[3]));
            self.add(&cat, CX.iri("definedIn"), file_iri(self.project, &path));
            let flags = &m[4];
            if flags.contains("BKI_BOOTSTRAP") {
                self.add(&cat, pg("isBootstrap"), Term::Literal(Literal::boolean(true)));
            }
            if flags.contains("BKI_SHARED_RELATION") {
                self.add(&cat, pg("isShared"), Term::Literal(Literal::boolean(true)));
            }
            let st = instance_iri(self.project, "struct", &format!("{}/{}", path, name));
            self.add(&cat, pg("representedBy"), st);
            self.count("system catalogs");

            // Columns: between the CATALOG line and `} FormData_...;`
            let start = m.get(0).unwrap().end();
            let body_end = text[start..].find("\n}").map(|e| start + e).unwrap_or(text.len());
            let mut pending_comment: Vec<String> = Vec::new();
            let mut in_comment = false;
            let mut varlen = false;
            let mut attnum = 0;
            for raw in text[start..body_end].lines() {
                let line = raw.trim();
                if in_comment {
                    pending_comment.push(line.trim_end_matches("*/").trim_start_matches('*').trim().to_string());
                    if line.contains("*/") {
                        in_comment = false;
                    }
                    continue;
                }
                if line.starts_with("/*") {
                    pending_comment.clear();
                    pending_comment.push(line.trim_start_matches("/*").trim_end_matches("*/").trim().to_string());
                    in_comment = !line.contains("*/");
                    continue;
                }
                if line.starts_with("#ifdef CATALOG_VARLEN") {
                    varlen = true;
                    continue;
                }
                if line.starts_with("#endif") {
                    varlen = false;
                    continue;
                }
                let code = line.split("/*").next().unwrap_or("").trim();
                let Some(c) = col_re.captures(code) else { continue };
                attnum += 1;
                let col = c[2].to_string();
                let ci = Iri::new(format!("{}/column/{}", cat.as_str(), iri_segment(&col)));
                self.add(&cat, pg("hasColumn"), ci.clone());
                self.add(&ci, rdf::type_(), pg("CatalogColumn"));
                self.add(&ci, rdfs::label(), lit(format!("{}.{}", name, col)));
                self.add(&ci, pg("columnNumber"), Term::Literal(Literal::integer(attnum)));
                let ctype = format!("{}{}", lang_c::norm_ws(&c[1]), c.get(3).map(|a| a.as_str()).unwrap_or(""));
                self.add(&ci, pg("cType"), lit(ctype));
                if varlen {
                    self.add(&ci, pg("isVarlen"), Term::Literal(Literal::boolean(true)));
                }
                let doc = lang_c::norm_ws(&pending_comment.join(" "));
                if !doc.is_empty() {
                    self.add(&ci, rdfs::comment(), lit(doc));
                }
                pending_comment.clear();
                for a in ann_re.captures_iter(&c[4]) {
                    let arg = a.get(2).map(|x| x.as_str().trim().to_string());
                    match (&a[1], arg) {
                        ("LOOKUP" | "LOOKUP_OPT", Some(t)) => {
                            let target = self.iri("catalog", &t);
                            self.add(&ci, pg("lookupCatalog"), target);
                        }
                        ("DEFAULT", Some(v)) => self.add(&ci, pg("defaultValue"), lit(v)),
                        ("ARRAY_DEFAULT", Some(v)) => self.add(&ci, pg("arrayDefaultValue"), lit(v)),
                        (flag, _) => self.add(&ci, pg("annotation"), lit(flag)),
                    }
                }
                self.count("catalog columns");
            }

            let caches: Vec<(String, String)> = cache_re.captures_iter(&text).map(|c| (c[1].to_string(), c[2].to_string())).collect();
            for c in idx_re.captures_iter(&text) {
                let idx_name = c[3].to_string();
                let ii = Iri::new(format!("{}/index/{}", cat.as_str(), iri_segment(&idx_name)));
                self.add(&cat, pg("hasIndex"), ii.clone());
                self.add(&ii, rdf::type_(), pg("CatalogIndex"));
                self.add(&ii, rdfs::label(), lit(idx_name.as_str()));
                self.add(&ii, pg("indexOid"), lit(&c[4]));
                self.add(&ii, pg("isUnique"), Term::Literal(Literal::boolean(c.get(1).is_some())));
                self.add(&ii, pg("isPrimaryKey"), Term::Literal(Literal::boolean(c.get(2).is_some())));
                self.add(&ii, pg("indexDefinition"), lit(&c[7]));
                for (cache, ix_name) in &caches {
                    if *ix_name == idx_name {
                        self.add(&ii, pg("syscache"), lit(cache.as_str()));
                    }
                }
                self.count("catalog indexes");
            }
        }
    }

    // ---- catalog contents (.dat) ----------------------------------------

    fn catalog_rows(&mut self) -> anyhow::Result<()> {
        let dir = self.root.join("src/include/catalog");
        let mut cats: Vec<String> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| n.ends_with(".dat"))
            .map(|n| n.trim_end_matches(".dat").to_string())
            .collect();
        cats.sort();
        let mut parsed: BTreeMap<String, Vec<DatRecord>> = BTreeMap::new();
        for c in &cats {
            if let Some(t) = self.read(&format!("src/include/catalog/{}.dat", c)) {
                parsed.insert(c.clone(), dat::parse(&t));
            }
        }

        // Symbol tables first: functions and types are referenced by name everywhere.
        for r in parsed.get("pg_proc").map(|v| v.as_slice()).unwrap_or(&[]) {
            let k = row_key("pg_proc", r);
            self.proc_keys_by_name.entry(r.get("proname").unwrap_or("").to_string()).or_default().push(k.clone());
            self.proc_keys.insert(k);
        }
        for r in parsed.get("pg_type").map(|v| v.as_slice()).unwrap_or(&[]) {
            if let Some(t) = r.get("typname") {
                self.type_names.insert(t.to_string());
            }
        }

        // pg_proc first so implementing C functions are known before references.
        let mut order: Vec<String> = vec!["pg_proc".into()];
        order.extend(cats.iter().filter(|c| *c != "pg_proc").cloned());
        let mut pending_roles: Vec<(String, &'static str)> = Vec::new();

        for cat in order {
            let Some(records) = parsed.get(&cat) else { continue };
            let cat_iri = self.iri("catalog", &cat);
            let class = row_class(&cat);
            for r in records.clone() {
                let key = row_key(&cat, &r);
                let ri = self.iri(&format!("row/{}", cat), &key);
                self.add(&ri, rdf::type_(), pg("CatalogRow"));
                if let Some(c) = class {
                    self.add(&ri, rdf::type_(), pg(c));
                }
                self.add(&ri, pg("inCatalog"), cat_iri.clone());
                self.add(&ri, rdfs::label(), lit(key.as_str()));
                for (k, v) in &r.fields {
                    if k == "descr" {
                        self.add(&ri, rdfs::comment(), lit(v.as_str()));
                        continue;
                    }
                    let pred = PGCAT.iri(k);
                    if REGPROC_COLS.contains(&k.as_str()) {
                        if let Some(pk) = self.resolve_proc(v) {
                            let target = self.iri("row/pg_proc", &pk);
                            self.add(&ri, pred, target);
                            if let Some(role) = role_for(&cat, k) {
                                pending_roles.push((pk, role));
                            }
                            continue;
                        }
                    } else if TYPE_COLS.contains(&k.as_str()) && self.type_names.contains(v) {
                        let target = self.iri("row/pg_type", v);
                        self.add(&ri, pred, target);
                        continue;
                    } else if TYPE_LIST_COLS.contains(&k.as_str()) {
                        for t in v.trim_matches(|c| c == '{' || c == '}').split(|c: char| c == ',' || c.is_whitespace()) {
                            if self.type_names.contains(t) {
                                let target = self.iri("row/pg_type", t);
                                self.add(&ri, pg("argumentType"), target);
                            }
                        }
                    }
                    self.add(&ri, pred, lit(v.as_str()));
                }
                if cat == "pg_proc" {
                    self.link_proc(&ri, &key, &r);
                }
                self.count(&format!("{} rows", cat));
            }
        }

        for (pk, role) in pending_roles {
            for f in self.proc_cfns.get(&pk).cloned().unwrap_or_default() {
                self.role(&f, role);
            }
        }
        Ok(())
    }

    fn resolve_proc(&self, v: &str) -> Option<String> {
        let v = v.trim();
        if v.is_empty() || v == "-" || v == "0" {
            return None;
        }
        if v.contains('(') {
            let k: String = v.chars().filter(|c| !c.is_whitespace()).collect();
            return self.proc_keys.contains(&k).then_some(k);
        }
        match self.proc_keys_by_name.get(v) {
            Some(ks) if ks.len() == 1 => Some(ks[0].clone()),
            _ => None,
        }
    }

    fn link_proc(&mut self, ri: &Iri, key: &str, r: &DatRecord) {
        let lang = r.get("prolang").unwrap_or("internal");
        let Some(src) = r.get("prosrc") else { return };
        if lang == "sql" {
            self.add(ri, pg("sqlBody"), lit(src));
            return;
        }
        let defs: Vec<Iri> = self.ix.global_defs(src, "src/backend").into_iter().take(4).map(|d| d.iri.clone()).collect();
        for f in &defs {
            self.add(ri, pg("implementedBy"), f.clone());
            self.add(f, pg("implementsSQLFunction"), ri.clone());
            self.role(f, "SQLCallableFunction");
        }
        if !defs.is_empty() {
            self.count("SQL functions linked to C");
        }
        self.proc_cfns.insert(key.to_string(), defs);
    }

    // ---- settings --------------------------------------------------------

    fn settings(&mut self) -> anyhow::Result<()> {
        let Some(text) = self.read("src/backend/utils/misc/guc_parameters.dat") else { return Ok(()) };
        for r in dat::parse(&text) {
            let Some(name) = r.get("name").map(|s| s.to_string()) else { continue };
            let gi = self.iri("guc", &name);
            self.add(&gi, rdf::type_(), pg("ConfigParameter"));
            self.add(&gi, rdfs::label(), lit(name.as_str()));
            for (k, v) in &r.fields {
                let pred = match k.as_str() {
                    "name" => continue,
                    "short_desc" => rdfs::comment(),
                    "long_desc" => pg("longDescription"),
                    "type" => pg("settingType"),
                    "context" => pg("context"),
                    "group" => pg("group"),
                    "flags" => pg("flags"),
                    "boot_val" => pg("bootValue"),
                    "min" => pg("minValue"),
                    "max" => pg("maxValue"),
                    "options" => pg("enumOptions"),
                    "variable" => pg("variableName"),
                    "check_hook" | "assign_hook" | "show_hook" => {
                        let hook_pred = match k.as_str() {
                            "check_hook" => "checkHook",
                            "assign_hook" => "assignHook",
                            _ => "showHook",
                        };
                        let targets: Vec<Iri> = self
                            .ix
                            .functions
                            .get(v.as_str())
                            .map(|ds| ds.iter().filter(|d| d.component == "src/backend").map(|d| d.iri.clone()).collect())
                            .unwrap_or_default();
                        if targets.is_empty() {
                            self.add(&gi, pg(hook_pred), lit(v.as_str()));
                        }
                        for t in targets {
                            self.add(&gi, pg(hook_pred), t.clone());
                            self.role(&t, "SettingHookFunction");
                        }
                        continue;
                    }
                    other => pg(&format!("setting_{}", other)),
                };
                self.add(&gi, pred, lit(v.as_str()));
            }
            if let Some(var) = r.get("variable") {
                let vars: Vec<Iri> = self
                    .ix
                    .globals
                    .get(var)
                    .map(|ds| ds.iter().filter(|d| d.component == "src/backend").map(|d| d.iri.clone()).collect())
                    .unwrap_or_default();
                for v in vars {
                    self.add(&gi, pg("boundToVariable"), v);
                }
                self.guc_by_var.entry(var.to_string()).or_default().push(gi.clone());
            }
            self.count("settings");
        }
        Ok(())
    }

    // ---- error conditions -----------------------------------------------

    fn sqlstates(&mut self) -> anyhow::Result<()> {
        let Some(text) = self.read("src/backend/utils/errcodes.txt") else { return Ok(()) };
        static SEC: OnceLock<Regex> = OnceLock::new();
        static ENT: OnceLock<Regex> = OnceLock::new();
        let sec = re(&SEC, r"^Section: Class (\w\w) - (.*)$");
        let ent = re(&ENT, r"^([0-9A-Z]{5})\s+([EWS])\s+(ERRCODE_[A-Z0-9_]+)(?:\s+([a-z0-9_]+))?\s*$");
        let mut class: Option<Iri> = None;
        for line in text.lines() {
            if let Some(c) = sec.captures(line) {
                let ci = self.iri("sqlstate-class", &c[1]);
                self.add(&ci, rdf::type_(), pg("SQLStateClass"));
                self.add(&ci, rdfs::label(), lit(c[2].trim()));
                self.add(&ci, pg("classCode"), lit(&c[1]));
                class = Some(ci);
                continue;
            }
            let Some(c) = ent.captures(line) else { continue };
            let si = self.iri("sqlstate", &c[1]);
            self.add(&si, rdf::type_(), pg("SQLState"));
            self.add(&si, rdfs::label(), lit(c.get(4).map(|m| m.as_str()).unwrap_or(&c[3])));
            self.add(&si, pg("sqlstateCode"), lit(&c[1]));
            self.add(&si, pg("errcodeMacro"), lit(&c[3]));
            if let Some(cond) = c.get(4) {
                self.add(&si, pg("conditionName"), lit(cond.as_str()));
            }
            let kind = match &c[2] {
                "E" => "error",
                "W" => "warning",
                _ => "success",
            };
            self.add(&si, pg("severityKind"), lit(kind));
            if let Some(ci) = class.clone() {
                self.add(&si, pg("inClass"), ci);
            }
            self.sqlstate_by_macro.insert(c[3].to_string(), si);
            self.count("SQLSTATEs");
        }
        Ok(())
    }

    // ---- wait events, locks, keywords -----------------------------------

    fn wait_events(&mut self) -> anyhow::Result<()> {
        let Some(text) = self.read("src/backend/utils/activity/wait_event_names.txt") else { return Ok(()) };
        static SEC: OnceLock<Regex> = OnceLock::new();
        static ENT: OnceLock<Regex> = OnceLock::new();
        let sec = re(&SEC, r"^Section: ClassName - (\w+)");
        let ent = re(&ENT, r#"^([A-Za-z0-9_]+)\s+"(.*)"\s*$"#);
        let mut class = String::new();
        for line in text.lines() {
            if let Some(c) = sec.captures(line) {
                class = c[1].to_string();
                continue;
            }
            let Some(c) = ent.captures(line) else { continue };
            let wi = self.iri("waitevent", &format!("{}/{}", class, &c[1]));
            self.add(&wi, rdf::type_(), pg("WaitEvent"));
            self.add(&wi, rdfs::label(), lit(&c[1]));
            self.add(&wi, pg("waitEventClass"), lit(class.as_str()));
            self.add(&wi, rdfs::comment(), lit(&c[2]));
            self.wait_by_macro.insert(format!("WAIT_EVENT_{}", &c[1]), wi);
            self.count("wait events");
        }
        Ok(())
    }

    fn lwlocks(&mut self) -> anyhow::Result<()> {
        let Some(text) = self.read("src/include/storage/lwlocklist.h") else { return Ok(()) };
        static L: OnceLock<Regex> = OnceLock::new();
        static T: OnceLock<Regex> = OnceLock::new();
        for c in re(&L, r"PG_LWLOCK\((\d+),\s*(\w+)\)").captures_iter(&text) {
            let li = self.iri("lwlock", &c[2]);
            self.add(&li, rdf::type_(), pg("LWLock"));
            self.add(&li, rdfs::label(), lit(&c[2]));
            self.add(&li, pg("lockId"), lit(&c[1]));
            self.lock_by_ident.insert(format!("{}Lock", &c[2]), li);
            self.count("LWLocks");
        }
        for c in re(&T, r"PG_LWLOCKTRANCHE\((\w+),\s*(\w+)\)").captures_iter(&text) {
            let ti = self.iri("lwlock-tranche", &c[2]);
            self.add(&ti, rdf::type_(), pg("LWLockTranche"));
            self.add(&ti, rdfs::label(), lit(&c[2]));
            self.add(&ti, pg("trancheId"), lit(&c[1]));
            self.tranche_by_ident.insert(format!("LWTRANCHE_{}", &c[1]), ti);
            self.count("LWLock tranches");
        }
        Ok(())
    }

    fn keywords(&mut self) -> anyhow::Result<()> {
        let Some(text) = self.read("src/include/parser/kwlist.h") else { return Ok(()) };
        static K: OnceLock<Regex> = OnceLock::new();
        for c in re(&K, r#"PG_KEYWORD\("([^"]+)",\s*(\w+),\s*(\w+),\s*(\w+)\)"#).captures_iter(&text) {
            let ki = self.iri("keyword", &c[1]);
            self.add(&ki, rdf::type_(), pg("Keyword"));
            self.add(&ki, rdfs::label(), lit(&c[1]));
            self.add(&ki, pg("grammarToken"), lit(&c[2]));
            self.add(&ki, pg("keywordCategory"), lit(&c[3]));
            self.add(&ki, pg("labelUsage"), lit(&c[4]));
            self.count("keywords");
        }
        Ok(())
    }

    // ---- node types ------------------------------------------------------

    fn node_types(&mut self) {
        // name → (struct iri, header stem, first field type, comment)
        let mut cands: BTreeMap<String, (Iri, String, String, Option<String>)> = BTreeMap::new();
        for f in self.files.iter().filter(|f| f.is_header) {
            for s in &f.structs {
                let Some(first) = s.fields.first() else { continue };
                let stem = f.path.rsplit('/').next().unwrap_or("").trim_end_matches(".h").to_string();
                let si = instance_iri(self.project, "struct", &format!("{}/{}", f.path, s.name));
                cands.entry(s.name.clone()).or_insert((si, stem, first.type_text.clone(), s.comment.clone()));
            }
        }
        let mut nodes: BTreeMap<String, Option<String>> = BTreeMap::new();
        for (n, (_, _, first, _)) in &cands {
            if first == "NodeTag" {
                nodes.insert(n.clone(), None);
            }
        }
        loop {
            let mut grew = false;
            for (n, (_, _, first, _)) in &cands {
                if !nodes.contains_key(n) && nodes.contains_key(first) {
                    nodes.insert(n.clone(), Some(first.clone()));
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        for name in nodes.keys() {
            let ni = self.iri("node", name);
            self.node_by_name.insert(name.clone(), ni);
        }
        for (name, parent) in nodes.clone() {
            let ni = self.node_by_name[&name].clone();
            let (si, stem, _, comment) = cands[&name].clone();
            self.add(&ni, rdf::type_(), pg("NodeType"));
            self.add(&ni, rdfs::label(), lit(name.as_str()));
            self.add(&ni, pg("representedBy"), si);
            self.add(&ni, pg("nodeHeader"), lit(stem));
            if let Some(c) = comment {
                self.add(&ni, rdfs::comment(), lit(c));
            }
            if let Some(p) = parent {
                let pi = self.node_by_name[&p].clone();
                self.add(&ni, pg("specializes"), pi);
            }
            self.count("node types");
        }
    }

    // ---- hooks -----------------------------------------------------------

    fn hooks(&mut self) {
        static H: OnceLock<Regex> = OnceLock::new();
        let h = re(&H, r"extern\s+PGDLLIMPORT\s+(\w+_hook_type)\s+(\w+)\s*;");
        let typedefs: BTreeMap<&str, &str> = self
            .files
            .iter()
            .flat_map(|f| f.typedefs.iter())
            .map(|t| (t.name.as_str(), t.text.as_str()))
            .collect();
        let headers: Vec<String> = self.files.iter().filter(|f| f.is_header).map(|f| f.path.clone()).collect();
        for path in headers {
            let Some(text) = self.read(&path) else { continue };
            if !text.contains("_hook_type") {
                continue;
            }
            for c in h.captures_iter(&text) {
                let hi = self.iri("hook", &c[2]);
                self.add(&hi, rdf::type_(), pg("Hook"));
                self.add(&hi, rdfs::label(), lit(&c[2]));
                self.add(&hi, pg("hookType"), lit(&c[1]));
                self.add(&hi, CX.iri("declaredIn"), file_iri(self.project, &path));
                if let Some(sig) = typedefs.get(&c[1]) {
                    self.add(&hi, pg("hookSignature"), lit(*sig));
                }
                self.hook_by_var.insert(c[2].to_string(), hi);
                self.count("hooks");
            }
        }
    }

    // ---- function profiles ----------------------------------------------

    fn function_profiles(&mut self) {
        for f in self.files {
            let comp = component_of(&f.path);
            let server_side = !(comp.starts_with("src/bin") || comp.starts_with("src/interfaces") || comp == "src/fe_utils");
            for func in &f.functions {
                let fi = fn_iri(self.project, &f.path, &func.name);
                let mut links: BTreeSet<(&'static str, Iri)> = BTreeSet::new();
                for id in &func.identifiers {
                    if let Some(s) = self.sqlstate_by_macro.get(id) {
                        links.insert(("mayRaise", s.clone()));
                    }
                    if let Some(w) = self.wait_by_macro.get(id) {
                        links.insert(("waitsOn", w.clone()));
                    }
                    if let Some(l) = self.lock_by_ident.get(id) {
                        links.insert(("usesLock", l.clone()));
                    }
                    if let Some(t) = self.tranche_by_ident.get(id) {
                        links.insert(("usesLockTranche", t.clone()));
                    }
                    if let Some(h) = self.hook_by_var.get(id) {
                        links.insert(("touchesHook", h.clone()));
                    }
                    if server_side {
                        for g in self.guc_by_var.get(id).into_iter().flatten() {
                            links.insert(("readsSetting", g.clone()));
                        }
                    }
                    if let Some(n) = id.strip_prefix("T_").and_then(|n| self.node_by_name.get(n)) {
                        links.insert(("inspectsNode", n.clone()));
                    }
                }
                for (callee, idx, arg) in &func.call_args {
                    let Some(n) = self.node_by_name.get(arg.as_str()) else { continue };
                    match (callee.as_str(), *idx) {
                        ("makeNode", 0) => {
                            links.insert(("createsNode", n.clone()));
                        }
                        ("IsA", 1) | ("castNode", 0) | ("lfirst_node", 0) | ("linitial_node", 0) | ("list_nth_node", 0) => {
                            links.insert(("inspectsNode", n.clone()));
                        }
                        _ => {}
                    }
                }
                let sig_types = func.params.iter().map(|p| p.type_text.as_str()).chain(std::iter::once(func.return_type.as_str()));
                for t in sig_types {
                    if let Some(base) = crate::lang_c::rdf::base_type(t) {
                        if let Some(n) = self.node_by_name.get(&base) {
                            links.insert(("operatesOn", n.clone()));
                        }
                    }
                }
                for lvl in &func.error_levels {
                    self.add(&fi, pg("reportsAtLevel"), lit(lvl.as_str()));
                }
                if func.params.iter().any(|p| p.type_text == "PG_FUNCTION_ARGS") {
                    self.role(&fi, "FmgrV1Function");
                }
                if func.name == "_PG_init" {
                    self.role(&fi, "ExtensionEntryPoint");
                }
                if !links.is_empty() {
                    self.count("functions with a grounded profile");
                }
                for (p, o) in links {
                    self.add(&fi, pg(p), o);
                }
            }
        }
        for (f, roles) in std::mem::take(&mut self.roles) {
            let fi = Iri::new(f);
            for r in roles {
                self.add(&fi, rdf::type_(), pg(r));
                self.count(&format!("role {}", r));
            }
        }
    }
}

// ----------------------------------------------------------------------------

const REGPROC_COLS: &[&str] = &[
    "typinput", "typoutput", "typreceive", "typsend", "typmodin", "typmodout", "typanalyze", "typsubscript",
    "oprcode", "oprrest", "oprjoin", "aggfnoid", "aggtransfn", "aggfinalfn", "aggcombinefn", "aggserialfn",
    "aggdeserialfn", "aggmtransfn", "aggminvtransfn", "aggmfinalfn", "amhandler", "amproc", "castfunc",
    "prosupport", "rngcanonical", "rngsubdiff", "prsstart", "prstoken", "prsend", "prsheadline", "prslextype",
    "tmplinit", "tmpllexize", "conproc", "lanplcallfoid", "laninline", "lanvalidator",
];

const TYPE_COLS: &[&str] = &[
    "prorettype", "provariadic", "typelem", "typarray", "typbasetype", "oprleft", "oprright", "oprresult",
    "castsource", "casttarget", "aggtranstype", "aggmtranstype", "amoplefttype", "amoprighttype",
    "amproclefttype", "amprocrighttype", "opcintype", "opckeytype", "rngtypid", "rngsubtype", "rngmultitypid",
];

const TYPE_LIST_COLS: &[&str] = &["proargtypes", "proallargtypes"];

fn role_for(cat: &str, col: &str) -> Option<&'static str> {
    match cat {
        "pg_type" => Some("TypeSupportFunction"),
        "pg_operator" => Some("OperatorImplementation"),
        "pg_aggregate" => Some("AggregateSupportFunction"),
        "pg_am" | "pg_amproc" => Some("IndexSupportFunction"),
        _ => {
            let _ = col;
            None
        }
    }
}

fn row_class(cat: &str) -> Option<&'static str> {
    Some(match cat {
        "pg_proc" => "BuiltinFunction",
        "pg_type" => "DataType",
        "pg_operator" => "Operator",
        "pg_cast" => "Cast",
        "pg_am" => "AccessMethod",
        "pg_aggregate" => "Aggregate",
        "pg_opclass" => "OperatorClass",
        "pg_opfamily" => "OperatorFamily",
        "pg_collation" => "Collation",
        "pg_language" => "LanguageEntry",
        _ => return None,
    })
}

/// The natural key of a bootstrap row — what makes it the same row across versions.
fn row_key(cat: &str, r: &DatRecord) -> String {
    let g = |k: &str| r.get(k).unwrap_or("").to_string();
    let k = match cat {
        "pg_proc" => format!("{}({})", g("proname"), g("proargtypes").split_whitespace().collect::<Vec<_>>().join(",")),
        "pg_type" => g("typname"),
        "pg_operator" => format!("{}({},{})", g("oprname"), r.get("oprleft").unwrap_or("0"), r.get("oprright").unwrap_or("0")),
        "pg_cast" => format!("{}->{}", g("castsource"), g("casttarget")),
        "pg_am" => g("amname"),
        "pg_opclass" => format!("{}/{}", g("opcmethod"), g("opcname")),
        "pg_opfamily" => format!("{}/{}", g("opfmethod"), g("opfname")),
        "pg_amop" => format!("{}/{}/{}/{}", g("amopfamily"), g("amoplefttype"), g("amoprighttype"), g("amopstrategy")),
        "pg_amproc" => format!("{}/{}/{}/{}", g("amprocfamily"), g("amproclefttype"), g("amprocrighttype"), g("amprocnum")),
        "pg_aggregate" => g("aggfnoid"),
        "pg_auth_members" => format!("{}/{}", g("roleid"), g("member")),
        "pg_authid" => g("rolname"),
        "pg_class" => g("relname"),
        "pg_collation" => g("collname"),
        "pg_conversion" => g("conname"),
        "pg_database" => g("datname"),
        "pg_language" => g("lanname"),
        "pg_namespace" => g("nspname"),
        "pg_range" => g("rngtypid"),
        "pg_tablespace" => g("spcname"),
        "pg_ts_config" => g("cfgname"),
        "pg_ts_config_map" => format!("{}/{}/{}", g("mapcfg"), g("maptokentype"), g("mapseqno")),
        "pg_ts_dict" => g("dictname"),
        "pg_ts_parser" => g("prsname"),
        "pg_ts_template" => g("tmplname"),
        _ => String::new(),
    };
    if !k.is_empty() {
        return k;
    }
    if let Some(oid) = r.get("oid") {
        return format!("oid:{}", oid);
    }
    // No natural key: content-address the row (a change reads as remove + add).
    let mut h: u64 = 0xcbf29ce484222325;
    for (k, v) in &r.fields {
        for b in k.bytes().chain([0x1f]).chain(v.bytes()).chain([0x1e]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("h:{:016x}", h)
}

fn area_class(dir: &str) -> &'static str {
    let segs: Vec<&str> = dir.split('/').collect();
    match segs.as_slice() {
        ["src", "backend", _, ..] => "BackendSubsystem",
        ["src", "bin", _] => "ClientProgram",
        ["src", "interfaces", _] => "ClientLibrary",
        ["src", "pl", _] => "ProceduralLanguage",
        ["contrib", _] => "Extension",
        ["src", "test", ..] => "TestModule",
        ["src", "common" | "port" | "fe_utils" | "timezone", ..] => "SharedCode",
        ["src", "include", ..] => "PublicHeaders",
        _ => "CodeArea",
    }
}

/// First real paragraph of a README (skips path lines, titles and underlines).
fn readme_summary(text: &str) -> Option<String> {
    for para in text.split("\n\n") {
        let lines: Vec<&str> = para
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .filter(|l| !l.chars().all(|c| matches!(c, '=' | '-' | '*' | '#')))
            .filter(|l| !(l.contains("/README") || l.starts_with("src/")))
            .collect();
        let joined = lang_c::norm_ws(&lines.join(" ").replace('#', " "));
        if joined.len() >= 40 {
            return Some(joined.chars().take(800).collect());
        }
    }
    None
}

/// PostgreSQL file headers read `* filename.c\n *    One-line purpose.\n *`.
fn file_description(comment: &str, path: &str) -> Option<String> {
    let base = path.rsplit('/').next()?;
    let lines: Vec<String> = comment
        .lines()
        .map(|l| l.trim().trim_start_matches("/*").trim_start_matches('*').trim().to_string())
        .collect();
    let at = lines.iter().position(|l| l == base)?;
    let mut desc = Vec::new();
    for l in &lines[at + 1..] {
        if l.is_empty() || l.starts_with("Portions Copyright") || l.starts_with("Copyright") {
            break;
        }
        desc.push(l.as_str());
    }
    let d = lang_c::norm_ws(&desc.join(" "));
    (!d.is_empty()).then(|| d.chars().take(600).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepass_rewrites_catalog_macro() {
        let src = "CATALOG(pg_class,1259,RelationRelationId) BKI_BOOTSTRAP BKI_ROWTYPE_OID(83,X)\n{\n\tOid oid;\n} FormData_pg_class;\n";
        let out = prepass(src);
        assert_eq!(out.len(), src.len());
        assert!(out.starts_with("struct pg_class "));
        assert!(!out.contains("BKI_"));
        let f = lang_c::extract(&mut lang_c::new_parser(), "src/include/catalog/pg_class.h", src, Some(prepass));
        assert_eq!(f.structs.len(), 1);
        assert_eq!(f.structs[0].name, "pg_class");
        assert_eq!(f.structs[0].fields[0].name, "oid");
    }

    #[test]
    fn file_header_description() {
        let c = "/*-----\n *\n * int.c\n *\t  Functions for the built-in integer types (except int8).\n *\n * Portions Copyright (c) 1996\n */";
        assert_eq!(file_description(c, "src/backend/utils/adt/int.c").as_deref(), Some("Functions for the built-in integer types (except int8)."));
    }

    #[test]
    fn proc_key_is_signature() {
        let r = DatRecord { fields: vec![("proname".into(), "int4pl".into()), ("proargtypes".into(), "int4 int4".into())] };
        assert_eq!(row_key("pg_proc", &r), "int4pl(int4,int4)");
    }
}
