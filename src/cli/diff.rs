//! # Diff Command
//!
//! Semantic comparison of two builds of the same project.
//!
//! Instance IRIs are version-independent and `graph.nt` is written in canonical
//! (sorted, de-duplicated) order, so two builds compare as a streaming merge of
//! sorted lines. Differences are grouped by entity and reported in the terms of
//! the ontology — SQL surface, settings, error conditions, catalogs, node types,
//! then C functions per code area with the change to their grounded profile —
//! rather than as text hunks. Positional facts (line numbers) are ignored.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const RDF_TYPE: &str = "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>";
const RDFS_LABEL: &str = "<http://www.w3.org/2000/01/rdf-schema#label>";
const CX: &str = "https://ontosys.io/ns/c#";
const PG: &str = "https://ontosys.io/ns/pg#";

/// Facts that move whenever unrelated lines are added above them.
const POSITIONAL: &[&str] = &["startLine", "endLine", "line", "parseErrors"];

/// Report order: the most specific class an entity has decides its section.
pub const SECTIONS: &[(&str, &str)] = &[
    ("pg:Release", "Release"),
    ("pg:BuiltinFunction", "Built-in SQL functions (pg_proc)"),
    ("pg:DataType", "Built-in types (pg_type)"),
    ("pg:Operator", "Operators (pg_operator)"),
    ("pg:Cast", "Casts (pg_cast)"),
    ("pg:Aggregate", "Aggregates (pg_aggregate)"),
    ("pg:AccessMethod", "Access methods (pg_am)"),
    ("pg:OperatorClass", "Operator classes"),
    ("pg:OperatorFamily", "Operator families"),
    ("pg:Collation", "Collations"),
    ("pg:LanguageEntry", "Languages"),
    ("pg:CatalogRow", "Other bootstrap catalog rows"),
    ("pg:ConfigParameter", "Configuration parameters (GUC)"),
    ("pg:SQLState", "Error conditions (SQLSTATE)"),
    ("pg:SQLStateClass", "SQLSTATE classes"),
    ("pg:WaitEvent", "Wait events"),
    ("pg:LWLock", "LWLocks"),
    ("pg:LWLockTranche", "LWLock tranches"),
    ("pg:Keyword", "SQL keywords"),
    ("pg:SystemCatalog", "System catalogs"),
    ("pg:CatalogColumn", "Catalog columns"),
    ("pg:CatalogIndex", "Catalog indexes"),
    ("pg:NodeType", "Node types (parse / plan / execution trees)"),
    ("pg:Hook", "Extension hooks"),
    ("cx:Function", "C functions"),
    ("cx:Prototype", "Header prototypes (published C interface)"),
    ("cx:Struct", "Structs"),
    ("cx:Union", "Unions"),
    ("cx:Field", "Struct fields"),
    ("cx:Enum", "Enums"),
    ("cx:EnumMember", "Enum members"),
    ("cx:Typedef", "Typedefs"),
    ("cx:Macro", "Header macros"),
    ("cx:GlobalVariable", "Global variables"),
    ("cx:SourceFile", "Source files"),
    ("cx:HeaderFile", "Header files"),
    ("pg:CodeArea", "Code areas"),
];

/// Sections shown as counts only (bookkeeping, not meaning).
const COUNT_ONLY: &[&str] = &["cx:ExternalSymbol", "cx:SystemHeader", "owl:Class", "owl:ObjectProperty", "owl:DatatypeProperty"];

/// Profile predicates of a function, with their reading.
pub const PROFILE: &[(&str, &str)] = &[
    ("pg:implementsSQLFunction", "implements SQL function"),
    ("rdf:type", "role"),
    ("cx:signature", "signature"),
    ("pg:mayRaise", "may raise"),
    ("pg:reportsAtLevel", "reports at level"),
    ("cx:emitsMessage", "message"),
    ("cx:emitsDetail", "detail"),
    ("cx:emitsHint", "hint"),
    ("cx:emitsContext", "context"),
    ("pg:readsSetting", "reads setting"),
    ("pg:createsNode", "creates node"),
    ("pg:inspectsNode", "inspects node"),
    ("pg:operatesOn", "operates on node"),
    ("pg:waitsOn", "waits on"),
    ("pg:usesLock", "uses lock"),
    ("pg:usesLockTranche", "uses lock tranche"),
    ("pg:touchesHook", "touches hook"),
    ("cx:calls", "calls"),
    ("cx:usesType", "uses type"),
    ("rdfs:comment", "comment"),
];

/// One graph: entity → set of (predicate, object), plus lookups for rendering.
#[derive(Default)]
pub struct Graph {
    pub labels: BTreeMap<String, String>,
    pub types: BTreeMap<String, BTreeSet<String>>,
    pub parent: BTreeMap<String, String>,
    pub defined_in: BTreeMap<String, String>,
    pub in_area: BTreeMap<String, String>,
    pub body_hash: BTreeMap<String, String>,
    pub version: Option<String>,
    pub triples: usize,
}

struct Line<'a> {
    s: &'a str,
    p: &'a str,
    o: &'a str,
}

fn split(line: &str) -> Option<Line<'_>> {
    let line = line.strip_suffix(" .").unwrap_or(line);
    let (s, rest) = line.split_once(' ')?;
    let (p, o) = rest.split_once(' ')?;
    Some(Line { s, p, o })
}

pub fn compact(iri: &str) -> String {
    let inner = iri.trim_start_matches('<').trim_end_matches('>');
    for (ns, short) in [
        (CX, "cx"),
        (PG, "pg"),
        ("https://ontosys.io/ns/pgcat#", "pgcat"),
        ("http://www.w3.org/1999/02/22-rdf-syntax-ns#", "rdf"),
        ("http://www.w3.org/2000/01/rdf-schema#", "rdfs"),
        ("http://www.w3.org/2002/07/owl#", "owl"),
    ] {
        if let Some(local) = inner.strip_prefix(ns) {
            return format!("{}:{}", short, local);
        }
    }
    inner.to_string()
}

fn is_positional(p: &str) -> bool {
    let c = compact(p);
    c.strip_prefix("cx:").map_or(false, |l| POSITIONAL.contains(&l))
}

/// Lexical form of a literal object, unescaped.
pub fn literal_text(o: &str) -> Option<String> {
    if !o.starts_with('"') {
        return None;
    }
    let end = o.rfind('"')?;
    let raw = &o[1..end];
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

impl Graph {
    pub fn load(text: &str) -> Graph {
        let mut g = Graph::default();
        for line in text.lines() {
            let Some(l) = split(line) else { continue };
            g.triples += 1;
            match l.p {
                RDFS_LABEL => {
                    if let Some(t) = literal_text(l.o) {
                        g.labels.insert(l.s.to_string(), t);
                    }
                }
                RDF_TYPE => {
                    g.types.entry(l.s.to_string()).or_default().insert(compact(l.o));
                }
                _ => {
                    let p = compact(l.p);
                    match p.as_str() {
                        "cx:hasField" | "cx:hasMember" | "pg:hasColumn" | "pg:hasIndex" => {
                            g.parent.insert(l.o.to_string(), l.s.to_string());
                        }
                        "cx:definedIn" => {
                            g.defined_in.insert(l.s.to_string(), l.o.to_string());
                        }
                        "pg:inArea" => {
                            g.in_area.insert(l.s.to_string(), l.o.to_string());
                        }
                        "cx:bodyHash" => {
                            g.body_hash.insert(l.s.to_string(), literal_text(l.o).unwrap_or_default());
                        }
                        "pg:version" => g.version = literal_text(l.o),
                        _ => {}
                    }
                }
            }
        }
        g
    }

    pub fn label(&self, iri: &str) -> String {
        let own = self.labels.get(iri).cloned().unwrap_or_else(|| {
            let t = iri.trim_end_matches('>');
            t.rsplit('/').next().unwrap_or(t).to_string()
        });
        match self.parent.get(iri) {
            Some(p) if !own.contains('.') => format!("{}.{}", self.label(p), own),
            _ => own,
        }
    }

    /// Label plus defining file, for telling same-named entities apart.
    pub fn locate(&self, o: &str) -> String {
        match self.defined_in.get(o) {
            Some(f) => format!("{} in {}", self.label(o), self.label(f)),
            None => compact(o),
        }
    }

    pub fn render(&self, o: &str) -> String {
        if let Some(t) = literal_text(o) {
            return t;
        }
        if o.starts_with('<') {
            let base = self.label(o);
            // Make SQLSTATEs readable as "condition (code)".
            if o.contains("/pg/sqlstate/") {
                let code = o.trim_end_matches('>').rsplit('/').next().unwrap_or("");
                return format!("{} ({})", base, code);
            }
            if self.types.contains_key(o) || self.labels.contains_key(o) {
                return base;
            }
            return compact(o);
        }
        o.to_string()
    }

    /// The area (directory) of an entity: its own, or that of its defining file.
    pub fn area_of(&self, iri: &str) -> Option<String> {
        let file = self.defined_in.get(iri).map(|s| s.as_str()).unwrap_or(iri);
        self.in_area.get(file).map(|a| self.label(a))
    }

    pub fn section(&self, iri: &str) -> &'static str {
        let types = self.types.get(iri);
        for (class, _) in SECTIONS {
            if types.map_or(false, |t| t.contains(*class)) {
                return class;
            }
        }
        if let Some(t) = types {
            for c in COUNT_ONLY {
                if t.contains(*c) {
                    return c;
                }
            }
            if t.iter().any(|c| c.starts_with("pg:") && c != "pg:CatalogRow") {
                return "pg:CodeArea";
            }
        }
        "other"
    }
}

#[derive(Default)]
pub struct EntityDiff {
    pub removed: Vec<(String, String)>,
    pub added: Vec<(String, String)>,
}

/// Entity-level comparison of two canonical graphs (old → new).
pub struct Analysis {
    pub a: Graph,
    pub b: Graph,
    /// Changed facts per subject (positional facts excluded)
    pub diffs: BTreeMap<String, EntityDiff>,
    /// Section (most specific class) → subjects only in the new graph
    pub added: BTreeMap<&'static str, Vec<String>>,
    /// Section → subjects only in the old graph
    pub removed: BTreeMap<&'static str, Vec<String>>,
    /// Section → subjects in both whose facts differ
    pub changed: BTreeMap<&'static str, Vec<String>>,
    /// Functions that moved file with the same name and body hash: (old, new)
    pub moves: Vec<(String, String)>,
}

/// Compare two canonical N-Triples texts: a streaming merge of sorted lines.
pub fn analyze(old_text: &str, new_text: &str) -> Analysis {
    let a = Graph::load(old_text);
    let b = Graph::load(new_text);

    // Streaming merge over canonical (sorted) lines.
    let mut diffs: BTreeMap<String, EntityDiff> = BTreeMap::new();
    let mut ia = old_text.lines().peekable();
    let mut ib = new_text.lines().peekable();
    loop {
        // true: the next line exists only in the old graph; false: only in the new one
        let only_old = match (ia.peek(), ib.peek()) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(x), Some(y)) => match x.cmp(y) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => {
                    ia.next();
                    ib.next();
                    continue;
                }
            },
        };
        let (line, removed) = if only_old { (ia.next().unwrap(), true) } else { (ib.next().unwrap(), false) };
        let Some(l) = split(line) else { continue };
        if is_positional(l.p) {
            continue;
        }
        let e = diffs.entry(l.s.to_string()).or_default();
        let po = (compact(l.p), l.o.to_string());
        if removed {
            e.removed.push(po);
        } else {
            e.added.push(po);
        }
    }

    // Classify: added / removed / changed, and pair function moves.
    let mut added: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut removed: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut changed: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (s, _) in &diffs {
        let in_a = a.types.contains_key(s) || a.labels.contains_key(s);
        let in_b = b.types.contains_key(s) || b.labels.contains_key(s);
        match (in_a, in_b) {
            (false, true) => added.entry(b.section(s)).or_default().push(s.clone()),
            (true, false) => removed.entry(a.section(s)).or_default().push(s.clone()),
            _ => changed.entry(b.section(s)).or_default().push(s.clone()),
        }
    }
    let mut moves: Vec<(String, String)> = Vec::new();
    if let (Some(ra), Some(ab)) = (removed.get("cx:Function"), added.get("cx:Function")) {
        let mut by_key: BTreeMap<(String, String), Vec<&String>> = BTreeMap::new();
        for s in ab {
            by_key.entry((b.label(s), b.body_hash.get(s).cloned().unwrap_or_default())).or_default().push(s);
        }
        for s in ra {
            let key = (a.label(s), a.body_hash.get(s).cloned().unwrap_or_default());
            if let Some(v) = by_key.get_mut(&key) {
                if let Some(t) = v.pop() {
                    moves.push((s.clone(), t.clone()));
                }
            }
        }
        let moved_from: BTreeSet<&String> = moves.iter().map(|m| &m.0).collect();
        let moved_to: BTreeSet<&String> = moves.iter().map(|m| &m.1).collect();
        removed.get_mut("cx:Function").unwrap().retain(|s| !moved_from.contains(s));
        added.get_mut("cx:Function").unwrap().retain(|s| !moved_to.contains(s));
    }

    Analysis { a, b, diffs, added, removed, changed, moves }
}

pub async fn run(old: &Path, new: &Path, out: Option<PathBuf>, json: Option<PathBuf>) -> anyhow::Result<()> {
    let old_nt = resolve_graph(old)?;
    let new_nt = resolve_graph(new)?;
    info(&format!("Old: {}", old_nt.display()));
    info(&format!("New: {}", new_nt.display()));
    let old_text = fs::read_to_string(&old_nt)?;
    let new_text = fs::read_to_string(&new_nt)?;
    let Analysis { a, b, diffs, added, removed, changed, moves } = analyze(&old_text, &new_text);

    // ---- report -------------------------------------------------------------
    let mut r = String::new();
    let va = a.version.clone().unwrap_or_else(|| "old".into());
    let vb = b.version.clone().unwrap_or_else(|| "new".into());
    writeln!(r, "# Semantic diff: {} → {}\n", va, vb)?;
    writeln!(
        r,
        "Generated by `ontosys diff` from two canonical graphs ({} → {} triples). \
         Entities are matched by version-independent IRI; line-number facts are ignored, \
         so moving code without changing it does not register.\n",
        a.triples, b.triples
    )?;

    writeln!(r, "## Summary\n")?;
    writeln!(r, "| Kind | Added | Removed | Changed |")?;
    writeln!(r, "|---|---:|---:|---:|")?;
    let mut keys: Vec<&str> = SECTIONS.iter().map(|(c, _)| *c).collect();
    keys.extend(COUNT_ONLY.iter().copied());
    keys.push("other");
    for k in &keys {
        let (na, nr, nc) = (
            added.get(k).map_or(0, |v| v.len()),
            removed.get(k).map_or(0, |v| v.len()),
            changed.get(k).map_or(0, |v| v.len()),
        );
        if na + nr + nc > 0 {
            writeln!(r, "| {} | {} | {} | {} |", section_title(k), na, nr, nc)?;
        }
    }
    if !moves.is_empty() {
        writeln!(r, "| C functions moved unchanged (same name and body hash) | {} | | |", moves.len())?;
    }
    writeln!(r)?;

    // Domain sections first, then code.
    for (class, title) in SECTIONS {
        let (na, nr, nc) = (added.get(class), removed.get(class), changed.get(class));
        if na.is_none() && nr.is_none() && nc.is_none() {
            continue;
        }
        writeln!(r, "## {}\n", title)?;
        if *class == "cx:Function" {
            function_section(&mut r, &a, &b, &diffs, na, nr, nc, &moves)?;
            continue;
        }
        if let Some(v) = na {
            writeln!(r, "**Added ({})**\n", v.len())?;
            for s in sorted_by_label(v, &b) {
                writeln!(r, "- `{}`{}", b.label(s), describe(&b, s))?;
            }
            writeln!(r)?;
        }
        if let Some(v) = nr {
            writeln!(r, "**Removed ({})**\n", v.len())?;
            for s in sorted_by_label(v, &a) {
                writeln!(r, "- `{}`{}", a.label(s), describe(&a, s))?;
            }
            writeln!(r)?;
        }
        if let Some(v) = nc {
            writeln!(r, "**Changed ({})**\n", v.len())?;
            for s in sorted_by_label(v, &b) {
                writeln!(r, "- `{}`{}", b.label(s), area_suffix(&b, s))?;
                property_changes(&mut r, &a, &b, &diffs[s], "  ")?;
            }
            writeln!(r)?;
        }
    }

    let out_path = out.unwrap_or_else(|| PathBuf::from(format!("ontosys-diff-{}-{}.md", sanitize(&va), sanitize(&vb))));
    fs::write(&out_path, &r)?;
    success(&format!("Report: {} ({} entities differ)", out_path.display(), diffs.len()));

    if let Some(jp) = json {
        let entities: Vec<serde_json::Value> = diffs
            .iter()
            .map(|(s, d)| {
                let in_b = b.labels.contains_key(s) || b.types.contains_key(s);
                let g = if in_b { &b } else { &a };
                serde_json::json!({
                    "iri": s.trim_matches(|c| c == '<' || c == '>'),
                    "label": g.label(s),
                    "class": g.section(s),
                    "removed": d.removed.iter().map(|(p, o)| serde_json::json!({"p": p, "o": a.render(o)})).collect::<Vec<_>>(),
                    "added": d.added.iter().map(|(p, o)| serde_json::json!({"p": p, "o": b.render(o)})).collect::<Vec<_>>(),
                })
            })
            .collect();
        fs::write(&jp, serde_json::to_string_pretty(&serde_json::json!({ "old": va, "new": vb, "entities": entities }))?)?;
        success(&format!("JSON: {}", jp.display()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn function_section(
    r: &mut String,
    a: &Graph,
    b: &Graph,
    diffs: &BTreeMap<String, EntityDiff>,
    na: Option<&Vec<String>>,
    nr: Option<&Vec<String>>,
    nc: Option<&Vec<String>>,
    moves: &[(String, String)],
) -> anyhow::Result<()> {
    // Bucket every function change under its code area.
    #[derive(Default)]
    struct Area<'a> {
        added: Vec<&'a String>,
        removed: Vec<&'a String>,
        changed: Vec<&'a String>,
        moved: Vec<&'a (String, String)>,
    }
    let mut areas: BTreeMap<String, Area> = BTreeMap::new();
    let unk = || "(no area)".to_string();
    for s in na.into_iter().flatten() {
        areas.entry(b.area_of(s).unwrap_or_else(unk)).or_default().added.push(s);
    }
    for s in nr.into_iter().flatten() {
        areas.entry(a.area_of(s).unwrap_or_else(unk)).or_default().removed.push(s);
    }
    for s in nc.into_iter().flatten() {
        areas.entry(b.area_of(s).unwrap_or_else(unk)).or_default().changed.push(s);
    }
    for m in moves {
        areas.entry(b.area_of(&m.1).unwrap_or_else(unk)).or_default().moved.push(m);
    }

    // Changed functions by kind of change.
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for s in nc.into_iter().flatten() {
        *kinds.entry(change_kind(&diffs[s])).or_default() += 1;
    }
    writeln!(r, "Changed functions by the most significant kind of change:\n")?;
    writeln!(r, "| Kind | Meaning | Functions |")?;
    writeln!(r, "|---|---|---:|")?;
    for (k, meaning) in [
        ("contract", "signature or parameters changed"),
        ("SQL role", "now / no longer implements a SQL function or support role"),
        ("behaviour", "errors raised, messages, settings read, nodes, locks, waits, hooks"),
        ("dependencies", "calls or types used changed, observable profile unchanged"),
        ("implementation", "code changed, profile and dependencies unchanged"),
        ("docs", "header comment only"),
        ("other", "metrics only"),
    ] {
        if let Some(n) = kinds.get(k) {
            writeln!(r, "| {} | {} | {} |", k, meaning, n)?;
        }
    }
    writeln!(r)?;

    for (area, d) in &areas {
        writeln!(r, "### `{}`\n", area)?;
        for s in sorted_by_label(&d.added.iter().map(|s| (*s).clone()).collect::<Vec<_>>(), b) {
            writeln!(r, "- **added** `{}`{}", b.label(s), describe(b, s))?;
            profile_lines(r, b, &diffs[s].added, "+")?;
        }
        for s in sorted_by_label(&d.removed.iter().map(|s| (*s).clone()).collect::<Vec<_>>(), a) {
            writeln!(r, "- **removed** `{}`{}", a.label(s), describe(a, s))?;
        }
        for (from, to) in &d.moved {
            writeln!(
                r,
                "- **moved** `{}`: `{}` → `{}`",
                b.label(to),
                a.defined_in.get(from).map(|f| a.label(f)).unwrap_or_default(),
                b.defined_in.get(to).map(|f| b.label(f)).unwrap_or_default()
            )?;
        }
        for s in sorted_by_label(&d.changed.iter().map(|s| (*s).clone()).collect::<Vec<_>>(), b) {
            let e = &diffs[s];
            writeln!(r, "- **changed** `{}` — *{}*{}", b.label(s), change_kind(e), file_suffix(b, s))?;
            property_changes(r, a, b, e, "  ")?;
        }
        writeln!(r)?;
    }
    Ok(())
}

/// The most significant kind of change to a function, for triage.
pub fn change_kind(e: &EntityDiff) -> &'static str {
    let preds: BTreeSet<&str> = e.removed.iter().chain(e.added.iter()).map(|(p, _)| p.as_str()).collect();
    let has = |p: &str| preds.contains(p);
    if has("cx:signature") || has("cx:parameters") {
        "contract"
    } else if has("pg:implementsSQLFunction") || has("rdf:type") {
        "SQL role"
    } else if preds.iter().any(|p| {
        matches!(
            *p,
            "pg:mayRaise" | "pg:reportsAtLevel" | "cx:emitsMessage" | "cx:emitsDetail" | "cx:emitsHint" | "cx:emitsContext"
                | "pg:readsSetting" | "pg:createsNode" | "pg:inspectsNode" | "pg:operatesOn" | "pg:waitsOn"
                | "pg:usesLock" | "pg:usesLockTranche" | "pg:touchesHook"
        )
    }) {
        "behaviour"
    } else if has("cx:calls") || has("cx:usesType") {
        "dependencies"
    } else if has("cx:bodyHash") {
        "implementation"
    } else if has("rdfs:comment") {
        "docs"
    } else {
        "other"
    }
}

fn property_changes(r: &mut String, a: &Graph, b: &Graph, e: &EntityDiff, indent: &str) -> anyhow::Result<()> {
    let mut preds: BTreeSet<&str> = BTreeSet::new();
    for (p, _) in e.removed.iter().chain(e.added.iter()) {
        preds.insert(p);
    }
    let ordered: Vec<&str> = PROFILE
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| preds.contains(p))
        .chain(preds.iter().copied().filter(|p| !PROFILE.iter().any(|(q, _)| q == p)))
        .collect();
    for p in ordered {
        let reading = PROFILE.iter().find(|(q, _)| *q == p).map(|(_, rd)| *rd).unwrap_or(p);
        let old: Vec<String> = e.removed.iter().filter(|(q, _)| q == p).map(|(_, o)| a.render(o)).collect();
        let new: Vec<String> = e.added.iter().filter(|(q, _)| q == p).map(|(_, o)| b.render(o)).collect();
        if p == "cx:bodyHash" || p == "cx:statementCount" || p == "cx:complexity" {
            if p == "cx:complexity" {
                writeln!(r, "{}- complexity {} → {}", indent, old.join(","), new.join(","))?;
            }
            if p == "cx:bodyHash" {
                writeln!(r, "{}- code changed", indent)?;
            }
            continue;
        }
        if old.len() == 1 && new.len() == 1 {
            let (o, n) = if old[0] == new[0] {
                // Same name, different entity: say where each one lives.
                let ro = e.removed.iter().find(|(q, _)| q == p).map(|(_, o)| a.locate(o)).unwrap_or_default();
                let rn = e.added.iter().find(|(q, _)| q == p).map(|(_, o)| b.locate(o)).unwrap_or_default();
                (ro, rn)
            } else {
                text_delta(&old[0], &new[0])
            };
            writeln!(r, "{}- {}: `{}` → `{}`", indent, reading, o, n)?;
            continue;
        }
        for o in &new {
            writeln!(r, "{}- + {}: `{}`", indent, reading, clip(o))?;
        }
        for o in &old {
            writeln!(r, "{}- − {}: `{}`", indent, reading, clip(o))?;
        }
    }
    Ok(())
}

/// For a newly added function: its grounded profile, as the reason it matters.
fn profile_lines(r: &mut String, g: &Graph, facts: &[(String, String)], _sign: &str) -> anyhow::Result<()> {
    for (p, reading) in PROFILE {
        if matches!(*p, "cx:calls" | "cx:usesType" | "cx:signature" | "rdfs:comment") {
            continue;
        }
        let vals: Vec<String> = facts
            .iter()
            .filter(|(q, o)| q == p && !(*p == "rdf:type" && compact(o).starts_with("cx:")))
            .map(|(_, o)| g.render(o))
            .collect();
        if !vals.is_empty() {
            writeln!(r, "  - {}: {}", reading, vals.iter().map(|v| format!("`{}`", clip(v))).collect::<Vec<_>>().join(", "))?;
        }
    }
    Ok(())
}

fn describe(g: &Graph, s: &str) -> String {
    let mut parts = Vec::new();
    if let Some(f) = g.defined_in.get(s) {
        parts.push(format!("in `{}`", g.label(f)));
    }
    let types: Vec<String> = g
        .types
        .get(s)
        .map(|t| t.iter().filter(|c| c.starts_with("pg:") && *c != "pg:CatalogRow").cloned().collect())
        .unwrap_or_default();
    if !types.is_empty() && g.section(s) == "cx:Function" {
        parts.push(types.join(", "));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" — {}", parts.join("; "))
    }
}

fn area_suffix(g: &Graph, s: &str) -> String {
    g.defined_in.get(s).map(|f| format!(" (`{}`)", g.label(f))).unwrap_or_default()
}

fn file_suffix(g: &Graph, s: &str) -> String {
    g.defined_in
        .get(s)
        .map(|f| format!(" (`{}`)", g.label(f).rsplit('/').next().unwrap_or_default()))
        .unwrap_or_default()
}

fn sorted_by_label<'a>(v: &'a [String], g: &Graph) -> Vec<&'a String> {
    let mut out: Vec<&String> = v.iter().collect();
    out.sort_by_cached_key(|s| (g.label(s), (*s).clone()));
    out
}

fn section_title(class: &str) -> String {
    SECTIONS
        .iter()
        .find(|(c, _)| *c == class)
        .map(|(_, t)| t.to_string())
        .unwrap_or_else(|| format!("{} (count only)", class))
}

/// Show only the differing middle of two long texts, with some context.
pub fn text_delta(old: &str, new: &str) -> (String, String) {
    let (a, b): (Vec<char>, Vec<char>) = (old.replace('\n', " ").chars().collect(), new.replace('\n', " ").chars().collect());
    if a.len() <= 200 && b.len() <= 200 {
        return (clip(old), clip(new));
    }
    let pre = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    let max_suf = a.len().min(b.len()) - pre;
    let suf = a.iter().rev().zip(b.iter().rev()).take(max_suf).take_while(|(x, y)| x == y).count();
    let ctx = 50;
    let cut = |v: &[char]| -> String {
        let start = pre.saturating_sub(ctx);
        let end = (v.len() - suf + ctx).min(v.len());
        format!(
            "{}{}{}",
            if start > 0 { "…" } else { "" },
            v[start..end].iter().collect::<String>().replace('`', "'"),
            if end < v.len() { "…" } else { "" }
        )
    };
    (cut(&a), cut(&b))
}

fn clip(s: &str) -> String {
    let one = s.replace('\n', " ").replace('`', "'");
    if one.chars().count() > 220 {
        format!("{}…", one.chars().take(220).collect::<String>())
    } else {
        one
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

/// Accept a repository (uses its `.ontosys/data/graph.nt`) or an `.nt` file.
pub fn resolve_graph(p: &Path) -> anyhow::Result<PathBuf> {
    if p.is_file() {
        return Ok(p.to_path_buf());
    }
    let nt = p.join(ONTOSYS_DIR).join("data/graph.nt");
    if nt.is_file() {
        Ok(nt)
    } else {
        Err(anyhow::anyhow!("No graph at {} (run 'ontosys build' there first)", nt.display()))
    }
}
