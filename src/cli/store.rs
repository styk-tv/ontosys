//! # Query store
//!
//! An in-memory Oxigraph store over `.ontosys/data/graph.nt`, optionally with a
//! second build loaded as the named graph `urn:ontosys:baseline` so the two can
//! be queried side by side. Used by `serve` (HTTP API) and `query` (CLI).
//!
//! Labels and types come from the same canonical files through `diff::Graph`,
//! so every IRI the API returns can carry a human label without another query,
//! and the entity-level delta is the same computation `ontosys diff` reports.

use super::diff::{self, Analysis, Graph};
use oxigraph::io::{RdfFormat, RdfParseError, RdfParser};
use oxigraph::model::{NamedNode, Term};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub const BASELINE: &str = "urn:ontosys:baseline";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// Which build a question is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Current,
    Baseline,
}

pub struct Loaded {
    pub store: Store,
    /// Present when a baseline was loaded: `a` = baseline, `b` = current.
    pub analysis: Option<Analysis>,
    current_only: Option<Graph>,
    /// (lowercased label, N-Triples subject key) over both builds, for search
    search_index: Vec<(String, String)>,
    /// TBox: class/property IRI → (label, comment, groundedIn)
    pub tbox: HashMap<String, (String, Option<String>, Option<String>)>,
    pub triples_current: usize,
    pub triples_baseline: usize,
    /// Lines that were not valid N-Triples and were left out (both builds).
    pub skipped_lines: usize,
    pub load_ms: u128,
}

fn key(iri: &str) -> String {
    format!("<{}>", iri)
}

fn unkey(k: &str) -> &str {
    k.trim_start_matches('<').trim_end_matches('>')
}

impl Loaded {
    pub fn load(current_nt: &Path, baseline_nt: Option<&Path>) -> anyhow::Result<Loaded> {
        let t = Instant::now();
        let store = Store::new()?;
        let (cur_text, mut skipped_lines) = load_lenient(&store, std::fs::read_to_string(current_nt)?, None)?;

        let (analysis, current_only) = match baseline_nt {
            Some(b) => {
                let (base_text, skipped) = load_lenient(&store, std::fs::read_to_string(b)?, Some(NamedNode::new(BASELINE)?))?;
                skipped_lines += skipped;
                (Some(diff::analyze(&base_text, &cur_text)), None)
            }
            None => (None, Some(Graph::load(&cur_text))),
        };

        let mut loaded = Loaded {
            store,
            analysis,
            current_only,
            search_index: Vec::new(),
            tbox: HashMap::new(),
            triples_current: 0,
            triples_baseline: 0,
            skipped_lines,
            load_ms: 0,
        };
        loaded.triples_current = loaded.cur().triples;
        loaded.triples_baseline = loaded.base().map_or(0, |g| g.triples);

        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut index = Vec::new();
        for g in [Some(loaded.cur()), loaded.base()].into_iter().flatten() {
            for (k, l) in &g.labels {
                if seen.insert(k.as_str()) {
                    index.push((l.to_lowercase(), k.clone()));
                }
            }
        }
        loaded.search_index = index;
        loaded.tbox = loaded.load_tbox();
        loaded.load_ms = t.elapsed().as_millis();
        Ok(loaded)
    }

    /// Label/type index of the current build.
    pub fn cur(&self) -> &Graph {
        match &self.analysis {
            Some(a) => &a.b,
            None => self.current_only.as_ref().expect("current graph index"),
        }
    }

    pub fn base(&self) -> Option<&Graph> {
        self.analysis.as_ref().map(|a| &a.a)
    }

    fn index_for(&self, side: Side) -> &Graph {
        match side {
            Side::Baseline => self.base().unwrap_or(self.cur()),
            Side::Current => self.cur(),
        }
    }

    /// Where an IRI exists: current if there, else baseline (removed entity).
    pub fn side_of(&self, iri: &str) -> Side {
        let k = key(iri);
        let in_cur = self.cur().labels.contains_key(&k) || self.cur().types.contains_key(&k);
        match self.base() {
            Some(b) if !in_cur && (b.labels.contains_key(&k) || b.types.contains_key(&k)) => Side::Baseline,
            _ => Side::Current,
        }
    }

    fn load_tbox(&self) -> HashMap<String, (String, Option<String>, Option<String>)> {
        let q = "PREFIX owl: <http://www.w3.org/2002/07/owl#>
            PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
            PREFIX pg: <https://ontosys.io/ns/pg#>
            SELECT ?t ?label ?comment ?grounded WHERE {
              ?t a ?k . FILTER(?k IN (owl:Class, owl:ObjectProperty, owl:DatatypeProperty))
              OPTIONAL { ?t rdfs:label ?label } OPTIONAL { ?t rdfs:comment ?comment } OPTIONAL { ?t pg:groundedIn ?grounded }
            }";
        let mut out = HashMap::new();
        if let Ok(QueryResults::Solutions(sol)) = SparqlEvaluator::new().parse_query(q).map(|p| p.on_store(&self.store).execute()).unwrap_or_else(|_| Err(oxigraph::sparql::QueryEvaluationError::Unexpected("parse".into()))) {
            for row in sol.flatten() {
                let iri = match row.get("t") {
                    Some(Term::NamedNode(n)) => n.as_str().to_string(),
                    _ => continue,
                };
                let lit = |v: &str| match row.get(v) {
                    Some(Term::Literal(l)) => Some(l.value().to_string()),
                    _ => None,
                };
                out.insert(iri, (lit("label").unwrap_or_default(), lit("comment"), lit("grounded")));
            }
        }
        out
    }

    // ------------------------------------------------------------------------
    // SPARQL
    // ------------------------------------------------------------------------

    fn term_json(&self, t: &Term) -> Value {
        match t {
            Term::NamedNode(n) => {
                let k = key(n.as_str());
                let g = if self.cur().labels.contains_key(&k) { self.cur() } else { self.base().unwrap_or(self.cur()) };
                let label = g.labels.get(&k).cloned();
                json!({ "type": "iri", "value": n.as_str(), "label": label, "short": diff::compact(&k) })
            }
            Term::BlankNode(b) => json!({ "type": "bnode", "value": b.as_str() }),
            Term::Literal(l) => json!({
                "type": "literal",
                "value": l.value(),
                "datatype": l.datatype().as_str(),
                "lang": l.language(),
            }),
            #[allow(unreachable_patterns)]
            _ => json!({ "type": "other", "value": t.to_string() }),
        }
    }

    /// Run a read-only SPARQL query; rows are capped at `limit` (`truncated` says so).
    pub fn sparql(&self, query: &str, limit: usize) -> anyhow::Result<Value> {
        let t = Instant::now();
        let prepared = SparqlEvaluator::new().parse_query(query).map_err(|e| anyhow::anyhow!("syntax: {}", e))?;
        let results = prepared.on_store(&self.store).execute().map_err(|e| anyhow::anyhow!("evaluation: {}", e))?;
        let out = match results {
            QueryResults::Solutions(sol) => {
                let vars: Vec<String> = sol.variables().iter().map(|v| v.as_str().to_string()).collect();
                let mut rows = Vec::new();
                let mut truncated = false;
                for row in sol {
                    if rows.len() >= limit {
                        truncated = true;
                        break;
                    }
                    let row = row.map_err(|e| anyhow::anyhow!("evaluation: {}", e))?;
                    let mut m = serde_json::Map::new();
                    for (v, term) in row.iter() {
                        m.insert(v.as_str().to_string(), self.term_json(term));
                    }
                    rows.push(Value::Object(m));
                }
                json!({ "kind": "select", "vars": vars, "rows": rows, "truncated": truncated })
            }
            QueryResults::Boolean(b) => json!({ "kind": "ask", "boolean": b }),
            QueryResults::Graph(triples) => {
                let mut rows = Vec::new();
                let mut truncated = false;
                for tr in triples {
                    if rows.len() >= limit {
                        truncated = true;
                        break;
                    }
                    let tr = tr.map_err(|e| anyhow::anyhow!("evaluation: {}", e))?;
                    rows.push(json!({
                        "s": self.term_json(&Term::from(tr.subject.clone())),
                        "p": self.term_json(&Term::from(tr.predicate.clone())),
                        "o": self.term_json(&tr.object),
                    }));
                }
                json!({ "kind": "graph", "vars": ["s", "p", "o"], "rows": rows, "truncated": truncated })
            }
        };
        let mut out = out;
        out["elapsed_ms"] = json!(t.elapsed().as_millis());
        Ok(out)
    }

    fn rows(&self, query: &str) -> Vec<BTreeMap<String, Term>> {
        match SparqlEvaluator::new().parse_query(query) {
            Ok(p) => match p.on_store(&self.store).execute() {
                Ok(QueryResults::Solutions(sol)) => sol
                    .flatten()
                    .map(|r| r.iter().map(|(v, t)| (v.as_str().to_string(), t.clone())).collect())
                    .collect(),
                _ => Vec::new(),
            },
            Err(_) => Vec::new(),
        }
    }

    // ------------------------------------------------------------------------
    // Node neighbourhood
    // ------------------------------------------------------------------------

    fn scope(side: Side, pattern: &str) -> String {
        match side {
            Side::Current => pattern.to_string(),
            Side::Baseline => format!("GRAPH <{}> {{ {} }}", BASELINE, pattern),
        }
    }

    fn brief(&self, iri: &str, side: Side) -> Value {
        let g = self.index_for(side);
        let k = key(iri);
        let label = g.labels.get(&k).cloned().or_else(|| self.cur().labels.get(&k).cloned());
        let types: Vec<String> = g.types.get(&k).map(|t| t.iter().cloned().collect()).unwrap_or_default();
        json!({ "iri": iri, "label": label, "short": diff::compact(&k), "types": types, "kind": kind_of(g, &k), "status": self.status(iri) })
    }

    /// added / removed / changed / moved / unchanged (null without a baseline)
    pub fn status(&self, iri: &str) -> Option<&'static str> {
        let a = self.analysis.as_ref()?;
        let k = key(iri);
        if a.moves.iter().any(|(_, to)| *to == k) {
            return Some("moved");
        }
        if !a.diffs.contains_key(&k) {
            return Some("unchanged");
        }
        let in_a = a.a.labels.contains_key(&k) || a.a.types.contains_key(&k);
        let in_b = a.b.labels.contains_key(&k) || a.b.types.contains_key(&k);
        Some(match (in_a, in_b) {
            (false, true) => "added",
            (true, false) => "removed",
            _ => "changed",
        })
    }

    /// Everything the explorer needs about one entity.
    pub fn node(&self, iri: &str, per_predicate: usize) -> anyhow::Result<Value> {
        NamedNode::new(iri).map_err(|e| anyhow::anyhow!("bad IRI: {}", e))?;
        let side = self.side_of(iri);
        let g = self.index_for(side);
        let k = key(iri);

        // Outgoing: literals become properties, IRIs become grouped links.
        let mut props: Vec<Value> = Vec::new();
        let mut out_links: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in self.rows(&format!("SELECT ?p ?o WHERE {{ {} }}", Self::scope(side, &format!("<{}> ?p ?o", iri)))) {
            let (Some(Term::NamedNode(p)), Some(o)) = (row.get("p"), row.get("o")) else { continue };
            match o {
                Term::Literal(l) => props.push(json!({
                    "p": p.as_str(), "short": diff::compact(&key(p.as_str())),
                    "label": self.tbox.get(p.as_str()).map(|t| t.0.clone()),
                    "value": l.value(),
                })),
                Term::NamedNode(o) if p.as_str() != RDF_TYPE => out_links.entry(p.as_str().to_string()).or_default().push(o.as_str().to_string()),
                _ => {}
            }
        }
        props.sort_by(|a, b| a["short"].as_str().cmp(&b["short"].as_str()).then(a["value"].as_str().cmp(&b["value"].as_str())));
        let group = |p: &str, total: usize, targets: Vec<Value>| -> Value {
            json!({
                "p": p, "short": diff::compact(&key(p)),
                "label": self.tbox.get(p).map(|t| t.0.clone()),
                "comment": self.tbox.get(p).and_then(|t| t.1.clone()),
                "total": total, "items": targets,
            })
        };
        let outgoing: Vec<Value> = out_links
            .into_iter()
            .map(|(p, mut os)| {
                os.sort_by_cached_key(|o| g.label(&key(o)));
                let total = os.len();
                group(&p, total, os.iter().take(per_predicate).map(|o| self.brief(o, side)).collect())
            })
            .collect();

        // Incoming: counts per predicate, then a sample per predicate.
        let mut incoming = Vec::new();
        for row in self.rows(&format!(
            "SELECT ?p (COUNT(?s) AS ?n) WHERE {{ {} }} GROUP BY ?p ORDER BY DESC(?n)",
            Self::scope(side, &format!("?s ?p <{}>", iri))
        )) {
            let (Some(Term::NamedNode(p)), Some(Term::Literal(n))) = (row.get("p"), row.get("n")) else { continue };
            let total: usize = n.value().parse().unwrap_or(0);
            let subjects: Vec<Value> = self
                .rows(&format!("SELECT ?s WHERE {{ {} }} LIMIT {}", Self::scope(side, &format!("?s <{}> <{}>", p.as_str(), iri)), per_predicate))
                .into_iter()
                .filter_map(|r| match r.get("s") {
                    Some(Term::NamedNode(s)) => Some(self.brief(s.as_str(), side)),
                    _ => None,
                })
                .collect();
            incoming.push(group(p.as_str(), total, subjects));
        }

        // Grounding: what each class means and which repository file defines it.
        let types: Vec<String> = g.types.get(&k).map(|t| t.iter().cloned().collect()).unwrap_or_default();
        let grounding: Vec<Value> = g
            .types
            .get(&k)
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let full = expand(c);
                self.tbox.get(&full).map(|(label, comment, grounded)| json!({
                    "class": c, "iri": full, "label": label, "comment": comment, "groundedIn": grounded,
                }))
            })
            .collect();

        // Delta: the same per-predicate changes `ontosys diff` reports.
        let delta = self.analysis.as_ref().and_then(|a| {
            let e = a.diffs.get(&k)?;
            let mut by_p: BTreeMap<&str, (Vec<String>, Vec<String>)> = BTreeMap::new();
            for (p, o) in &e.removed {
                by_p.entry(p).or_default().0.push(a.a.render(o));
            }
            for (p, o) in &e.added {
                by_p.entry(p).or_default().1.push(a.b.render(o));
            }
            let changes: Vec<Value> = by_p
                .into_iter()
                .map(|(p, (r, ad))| {
                    let (r, ad) = if r.len() == 1 && ad.len() == 1 && (r[0].len() > 200 || ad[0].len() > 200) {
                        let (x, y) = diff::text_delta(&r[0], &ad[0]);
                        (vec![x], vec![y])
                    } else {
                        (r, ad)
                    };
                    json!({ "p": p, "removed": r, "added": ad })
                })
                .collect();
            Some(json!({ "kind": if self.status(iri) == Some("changed") { diff::change_kind(e) } else { "" }, "changes": changes }))
        });

        Ok(json!({
            "iri": iri,
            "label": g.labels.get(&k).cloned(),
            "short": diff::compact(&k),
            "side": if side == Side::Baseline { "baseline" } else { "current" },
            "types": types,
            "kind": kind_of(g, &k),
            "status": self.status(iri),
            "properties": props,
            "outgoing": outgoing,
            "incoming": incoming,
            "grounding": grounding,
            "delta": delta,
            "area": g.area_of(&k),
        }))
    }

    /// One page of a node's links for a predicate (for "show more").
    pub fn links(&self, iri: &str, p: &str, incoming: bool, offset: usize, limit: usize) -> anyhow::Result<Value> {
        NamedNode::new(iri)?;
        NamedNode::new(p)?;
        let side = self.side_of(iri);
        let pattern = if incoming { format!("?x <{}> <{}>", p, iri) } else { format!("<{}> <{}> ?x", iri, p) };
        let items: Vec<Value> = self
            .rows(&format!("SELECT ?x WHERE {{ {} FILTER(isIRI(?x)) }} ORDER BY ?x LIMIT {} OFFSET {}", Self::scope(side, &pattern), limit, offset))
            .into_iter()
            .filter_map(|r| match r.get("x") {
                Some(Term::NamedNode(x)) => Some(self.brief(x.as_str(), side)),
                _ => None,
            })
            .collect();
        Ok(json!({ "items": items, "offset": offset }))
    }

    // ------------------------------------------------------------------------
    // Search, view, aggregates, delta
    // ------------------------------------------------------------------------

    pub fn search(&self, q: &str, limit: usize) -> Value {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return json!({ "items": [] });
        }
        let mut hits: Vec<(u8, usize, &str)> = self
            .search_index
            .iter()
            .filter(|(l, _)| l.contains(&q))
            .map(|(l, k)| (if *l == q { 0 } else if l.starts_with(&q) { 1 } else { 2 }, l.len(), k.as_str()))
            .collect();
        let total = hits.len();
        hits.sort();
        let items: Vec<Value> = hits.into_iter().take(limit).map(|(_, _, k)| self.brief(unkey(k), self.side_of(unkey(k)))).collect();
        json!({ "items": items, "total": total })
    }

    /// Nodes and links to draw: the whole graph when small, otherwise the
    /// entities of `overview_classes` (from both builds) and links among them.
    pub fn view(&self, overview_classes: &[String], max_full: usize) -> Value {
        let full = self.triples_current <= max_full || overview_classes.is_empty();
        let mut nodes: BTreeMap<String, Side> = BTreeMap::new();
        let mut edges: BTreeSet<(String, String, String, &'static str)> = BTreeSet::new();
        let sides: Vec<Side> = if self.analysis.is_some() { vec![Side::Current, Side::Baseline] } else { vec![Side::Current] };
        for side in &sides {
            let g = self.index_for(*side);
            for (k, ts) in &g.types {
                let keep = full || ts.iter().any(|t| overview_classes.contains(t));
                if keep && !ts.iter().any(|t| t.starts_with("owl:")) {
                    nodes.entry(unkey(k).to_string()).or_insert(*side);
                }
            }
        }
        for side in &sides {
            let values = if full { String::new() } else {
                format!("VALUES ?c {{ {} }} ?s a ?c .", overview_classes.iter().map(|c| format!("<{}>", expand(c))).collect::<Vec<_>>().join(" "))
            };
            let q = format!("SELECT ?s ?p ?o WHERE {{ {} }}", Self::scope(*side, &format!("{} ?s ?p ?o . FILTER(isIRI(?o) && ?p != <{}>)", values, RDF_TYPE)));
            for row in self.rows(&q) {
                if let (Some(Term::NamedNode(s)), Some(Term::NamedNode(p)), Some(Term::NamedNode(o))) = (row.get("s"), row.get("p"), row.get("o")) {
                    if nodes.contains_key(o.as_str()) && nodes.contains_key(s.as_str()) {
                        let tag = if *side == Side::Baseline { "baseline" } else { "current" };
                        edges.insert((s.as_str().to_string(), p.as_str().to_string(), o.as_str().to_string(), tag));
                    }
                }
            }
        }
        // An edge present in both builds is drawn once.
        let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
        let mut edge_json = Vec::new();
        for (s, p, o, tag) in &edges {
            if seen.insert((s.clone(), p.clone(), o.clone())) {
                let both = edges.contains(&(s.clone(), p.clone(), o.clone(), if *tag == "current" { "baseline" } else { "current" }));
                let status = if self.analysis.is_none() || both { "unchanged" } else if *tag == "current" { "added" } else { "removed" };
                edge_json.push(json!({ "source": s, "target": o, "p": diff::compact(&key(p)), "status": status }));
            }
        }
        let node_json: Vec<Value> = nodes.iter().map(|(iri, side)| self.brief(iri, *side)).collect();
        json!({ "mode": if full { "full" } else { "overview" }, "nodes": node_json, "edges": edge_json })
    }

    // ------------------------------------------------------------------------
    // Classes: what any graph can be browsed by, grounded or not
    // ------------------------------------------------------------------------

    /// Every class with instances in the current build, largest first.
    pub fn classes(&self) -> Vec<Value> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for ts in self.cur().types.values() {
            for t in ts.iter().filter(|t| !t.starts_with("owl:")) {
                *counts.entry(t.as_str()).or_default() += 1;
            }
        }
        let mut v: Vec<(&str, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v.into_iter()
            .map(|(c, n)| {
                let iri = expand(c);
                let label = self.tbox.get(&iri).map(|t| t.0.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| local_name(&iri).to_string());
                json!({ "iri": iri, "short": c, "label": label, "count": n })
            })
            .collect()
    }

    /// How classes link: (subject class, predicate, object class) with the number
    /// of triples, for the `limit` most frequent combinations.
    pub fn class_links(&self, limit: usize) -> Vec<Value> {
        let types = &self.cur().types;
        let mut counts: HashMap<(&str, String, &str), usize> = HashMap::new();
        for q in self.store.quads_for_pattern(None, None, None, Some(oxigraph::model::GraphNameRef::DefaultGraph)).flatten() {
            let Term::NamedNode(o) = &q.object else { continue };
            if q.predicate.as_str() == RDF_TYPE {
                continue;
            }
            let oxigraph::model::NamedOrBlankNode::NamedNode(s) = &q.subject else { continue };
            let (Some(st), Some(ot)) = (types.get(&key(s.as_str())), types.get(&key(o.as_str()))) else { continue };
            for a in st.iter().filter(|t| !t.starts_with("owl:")) {
                for b in ot.iter().filter(|t| !t.starts_with("owl:")) {
                    *counts.entry((a.as_str(), q.predicate.as_str().to_string(), b.as_str())).or_default() += 1;
                }
            }
        }
        let mut v: Vec<_> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter()
            .take(limit)
            .map(|((a, p, b), n)| json!({
                "source": expand(a), "target": expand(b), "p": diff::compact(&key(&p)),
                "label": self.tbox.get(&p).map(|t| t.0.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| local_name(&p).to_string()),
                "count": n,
            }))
            .collect()
    }

    /// One page of a class's members, sorted by name, optionally filtered by a
    /// case-insensitive substring of the name.
    pub fn class_members(&self, class: &str, q: &str, offset: usize, limit: usize) -> Value {
        let g = self.cur();
        let c = diff::compact(&key(class));
        let q = q.trim().to_lowercase();
        let mut v: Vec<(String, &String)> = g
            .types
            .iter()
            .filter(|(_, ts)| ts.contains(&c))
            .map(|(k, _)| (g.labels.get(k).cloned().unwrap_or_else(|| local_name(unkey(k)).to_string()), k))
            .filter(|(l, _)| q.is_empty() || l.to_lowercase().contains(&q))
            .collect();
        v.sort();
        let total = v.len();
        let items: Vec<Value> = v.into_iter().skip(offset).take(limit).map(|(_, k)| self.brief(unkey(k), Side::Current)).collect();
        json!({ "items": items, "total": total, "offset": offset })
    }

    pub fn delta_summary(&self) -> Option<Value> {
        let a = self.analysis.as_ref()?;
        let mut sections = Vec::new();
        for (class, title) in diff::SECTIONS {
            let n = |m: &BTreeMap<&'static str, Vec<String>>| m.get(class).map_or(0, |v| v.len());
            let (ad, rm, ch) = (n(&a.added), n(&a.removed), n(&a.changed));
            if ad + rm + ch > 0 {
                sections.push(json!({ "class": class, "title": title, "added": ad, "removed": rm, "changed": ch }));
            }
        }
        let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
        for s in a.changed.get("cx:Function").into_iter().flatten() {
            *kinds.entry(diff::change_kind(&a.diffs[s])).or_default() += 1;
        }
        Some(json!({
            "baseline": a.a.version, "current": a.b.version,
            "triples": { "baseline": a.a.triples, "current": a.b.triples },
            "entities_differ": a.diffs.len(),
            "moves": a.moves.len(),
            "sections": sections,
            "function_change_kinds": kinds,
        }))
    }

    pub fn delta_list(&self, class: &str, status: &str, offset: usize, limit: usize) -> Value {
        let Some(a) = self.analysis.as_ref() else { return json!({ "items": [] }) };
        let src = match status {
            "added" => &a.added,
            "removed" => &a.removed,
            _ => &a.changed,
        };
        let mut v: Vec<&String> = src.get(class).map(|v| v.iter().collect()).unwrap_or_default();
        let g = if status == "removed" { &a.a } else { &a.b };
        v.sort_by_cached_key(|k| g.label(k));
        let total = v.len();
        let items: Vec<Value> = v
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|k| {
                let mut b = self.brief(unkey(k), if status == "removed" { Side::Baseline } else { Side::Current });
                if status == "changed" {
                    b["change"] = json!(diff::change_kind(&a.diffs[k]));
                }
                b["area"] = json!(g.area_of(k));
                b
            })
            .collect();
        json!({ "items": items, "total": total, "offset": offset })
    }
}

/// Bulk-load N-Triples into `store`, leaving out lines that are not valid
/// N-Triples instead of refusing the file (the parser resumes at the next line).
/// Returns the text the label/type index should read — without the skipped
/// lines, so it describes exactly what the store holds — and how many were skipped.
fn load_lenient(store: &Store, text: String, graph: Option<NamedNode>) -> anyhow::Result<(String, usize)> {
    // Checked parsing: an IRI the store accepted but the API would refuse
    // (spaces, brackets) is skipped here rather than failing on selection.
    let parser = || {
        let p = RdfParser::from_format(RdfFormat::NTriples);
        match &graph {
            Some(g) => p.with_default_graph(g.clone()),
            None => p,
        }
    };
    let errors = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&errors);
    let mut loader = store.bulk_loader().on_parse_error(move |e| match e {
        RdfParseError::Syntax(_) => {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        e => Err(e),
    });
    loader.load_from_slice(parser(), text.as_bytes())?;
    loader.commit()?;
    if errors.load(Ordering::Relaxed) == 0 {
        return Ok((text, 0));
    }
    // Rare path: find the exact lines once, so the index skips the same ones.
    let bad: BTreeSet<u64> = parser()
        .for_slice(text.as_bytes())
        .filter_map(|r| r.err().and_then(|e| e.location()).map(|l| l.start.line))
        .collect();
    let clean: Vec<&str> = text.lines().enumerate().filter(|(i, _)| !bad.contains(&(*i as u64))).map(|(_, l)| l).collect();
    Ok((clean.join("\n") + "\n", bad.len()))
}

/// What an entity is drawn and listed as: its report section when ontosys
/// knows the class, otherwise its own (first) class.
fn kind_of(g: &Graph, k: &str) -> String {
    match g.section(k) {
        "other" => g.types.get(k).and_then(|ts| ts.iter().find(|t| !t.starts_with("owl:"))).map(|t| expand(t)).unwrap_or_else(|| "other".into()),
        s => s.to_string(),
    }
}

/// The last segment of an IRI (after `#`, else after `/`).
pub fn local_name(iri: &str) -> &str {
    let t = iri.trim_end_matches(['/', '#']);
    t.rsplit(['#', '/']).next().filter(|s| !s.is_empty()).unwrap_or(t)
}

/// `pg:Foo` → full IRI (only the prefixes ontosys emits).
pub fn expand(c: &str) -> String {
    for (short, ns) in [
        ("cx:", "https://ontosys.io/ns/c#"),
        ("pg:", "https://ontosys.io/ns/pg#"),
        ("pgcat:", "https://ontosys.io/ns/pgcat#"),
        ("rdf:", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
        ("rdfs:", "http://www.w3.org/2000/01/rdf-schema#"),
        ("owl:", "http://www.w3.org/2002/07/owl#"),
    ] {
        if let Some(local) = c.strip_prefix(short) {
            return format!("{}{}", ns, local);
        }
    }
    c.to_string()
}
