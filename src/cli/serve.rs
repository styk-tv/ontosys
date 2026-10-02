//! # Serve Command
//!
//! Explorer for a built graph. The full `graph.nt` is loaded into an in-memory
//! SPARQL store (Oxigraph); with `--compare` a second build is loaded as the
//! named graph `urn:ontosys:baseline` and every entity carries its delta.
//!
//! API (all JSON, read-only):
//!   /api/info                 build, sizes, baseline
//!   /api/view                 nodes + links to draw (whole graph when small, overview otherwise)
//!   /api/node?iri=            one entity: properties, grouped links in/out, grounding, delta
//!   /api/links?iri=&p=&dir=   one page of a node's links for a predicate
//!   /api/search?q=            label search over both builds
//!   /api/aggregates           SPARQL aggregates over the full graph, computed at startup
//!   /api/delta, /api/delta/list?class=&status=
//!   /api/sparql?query=        any read-only SPARQL query (GET or POST body)

use super::store::{expand, Loaded};
use super::*;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
    routing::get,
    Router,
};
use console::style;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

/// Above this many triples the view shows the overview classes only.
const MAX_FULL_VIEW: usize = 250_000;

const INDEX_HTML: &str = include_str!("viz/index.html");
const FAVICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="4" cy="4" r="3" fill="#2f6fd6"/><circle cx="12" cy="7" r="3" fill="#0f8b8d"/><circle cx="6" cy="13" r="2.5" fill="#e8823a"/></svg>"##;

struct AppState {
    loaded: Loaded,
    info: Value,
    view: Value,
    aggregates: Value,
}

type Shared = Arc<AppState>;
type Params = Query<HashMap<String, String>>;

pub async fn run(repo_path: &Path, port: u16, open_browser: bool, compare: Option<PathBuf>) -> anyhow::Result<()> {
    println!("\n{}", style("OntoSys Explorer").cyan().bold());
    println!("{}\n", style("═".repeat(50)).dim());

    let root = find_git_root(repo_path).unwrap_or_else(|| repo_path.to_path_buf());
    let current = super::diff::resolve_graph(&root)?;
    let baseline = compare.as_deref().map(super::diff::resolve_graph).transpose()?;
    info(&format!("Graph:    {}", current.display()));
    if let Some(b) = &baseline {
        info(&format!("Baseline: {}", b.display()));
    }
    info("Loading into the SPARQL store...");

    let meta_path = current.parent().map(|d| d.join("build-meta.json"));
    let state = tokio::task::spawn_blocking(move || -> anyhow::Result<AppState> {
        let loaded = Loaded::load(&current, baseline.as_deref())?;
        let overview: Vec<String> = if loaded.tbox.contains_key(&expand("pg:SystemCatalog")) {
            crate::grounding::postgres::OVERVIEW_CLASSES.iter().map(|c| format!("pg:{}", c)).collect()
        } else {
            Vec::new()
        };
        let view = loaded.view(&overview, MAX_FULL_VIEW);
        let aggregates = aggregates(&loaded);
        let meta: Value = meta_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));
        let info = json!({
            "project": meta.get("project"),
            "version": loaded.cur().version,
            "baseline": loaded.base().map(|b| b.version.clone()),
            "triples": loaded.triples_current,
            "baseline_triples": loaded.triples_baseline,
            "load_ms": loaded.load_ms,
            "view_mode": view["mode"],
            "build": meta,
        });
        Ok(AppState { loaded, info, view, aggregates })
    })
    .await??;

    success(&format!(
        "Loaded {} triples{} in {} ms; view: {} ({} nodes)",
        state.loaded.triples_current,
        if state.loaded.triples_baseline > 0 { format!(" + baseline {}", state.loaded.triples_baseline) } else { String::new() },
        state.loaded.load_ms,
        state.view["mode"].as_str().unwrap_or("?"),
        state.view["nodes"].as_array().map_or(0, |a| a.len()),
    ));

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/favicon.ico", get(|| async {
            ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], FAVICON)
        }))
        .route("/api/info", get(|State(s): State<Shared>| async move { Json(s.info.clone()) }))
        .route("/api/view", get(|State(s): State<Shared>| async move { Json(s.view.clone()) }))
        .route("/api/graph", get(|State(s): State<Shared>| async move { Json(s.view.clone()) }))
        .route("/api/aggregates", get(|State(s): State<Shared>| async move { Json(s.aggregates.clone()) }))
        .route("/api/delta", get(|State(s): State<Shared>| async move { Json(s.loaded.delta_summary().unwrap_or(json!(null))) }))
        .route("/api/delta/list", get(delta_list))
        .route("/api/node", get(node))
        .route("/api/links", get(links))
        .route("/api/search", get(search))
        .route("/api/sparql", get(sparql_get).post(sparql_post))
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!();
    success(&format!("Explorer on http://localhost:{}", port));
    println!("  {} Press {} to stop", style("→").dim(), style("Ctrl+C").yellow());
    println!();
    if open_browser {
        let _ = open::that(format!("http://localhost:{}", port));
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn bad(msg: impl std::fmt::Display) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg.to_string() }))).into_response()
}

fn num(p: &HashMap<String, String>, k: &str, default: usize, max: usize) -> usize {
    p.get(k).and_then(|v| v.parse().ok()).unwrap_or(default).min(max)
}

async fn blocking<F>(s: Shared, f: F) -> Response
where
    F: FnOnce(&Loaded) -> anyhow::Result<Value> + Send + 'static,
{
    match tokio::task::spawn_blocking(move || f(&s.loaded)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e),
    }
}

async fn node(State(s): State<Shared>, Query(p): Params) -> Response {
    let Some(iri) = p.get("iri").cloned() else { return bad("iri required") };
    let n = num(&p, "n", 25, 500);
    blocking(s, move |l| l.node(&iri, n)).await
}

async fn links(State(s): State<Shared>, Query(p): Params) -> Response {
    let (Some(iri), Some(pred)) = (p.get("iri").cloned(), p.get("p").cloned()) else { return bad("iri and p required") };
    let incoming = p.get("dir").map_or(false, |d| d == "in");
    let (offset, limit) = (num(&p, "offset", 0, usize::MAX), num(&p, "limit", 50, 500));
    blocking(s, move |l| l.links(&iri, &pred, incoming, offset, limit)).await
}

async fn search(State(s): State<Shared>, Query(p): Params) -> Response {
    let q = p.get("q").cloned().unwrap_or_default();
    let limit = num(&p, "limit", 30, 200);
    blocking(s, move |l| Ok(l.search(&q, limit))).await
}

async fn delta_list(State(s): State<Shared>, Query(p): Params) -> Response {
    let class = p.get("class").cloned().unwrap_or_default();
    let status = p.get("status").cloned().unwrap_or_else(|| "changed".into());
    let (offset, limit) = (num(&p, "offset", 0, usize::MAX), num(&p, "limit", 50, 500));
    blocking(s, move |l| Ok(l.delta_list(&class, &status, offset, limit))).await
}

async fn sparql_get(State(s): State<Shared>, Query(p): Params) -> Response {
    let Some(q) = p.get("query").cloned() else { return bad("query required") };
    let limit = num(&p, "limit", 1000, 10_000);
    blocking(s, move |l| l.sparql(&q, limit)).await
}

async fn sparql_post(State(s): State<Shared>, Query(p): Params, body: String) -> Response {
    let q = if let Some(rest) = body.strip_prefix("query=") {
        urlencoding_decode(rest)
    } else {
        body
    };
    let limit = num(&p, "limit", 1000, 10_000);
    blocking(s, move |l| l.sparql(&q, limit)).await
}

fn urlencoding_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ----------------------------------------------------------------------------
// Aggregates — real numbers over the full graph, computed once at startup
// ----------------------------------------------------------------------------

const PREFIXES: &str = "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
PREFIX cx: <https://ontosys.io/ns/c#>
PREFIX pg: <https://ontosys.io/ns/pg#>
PREFIX pgcat: <https://ontosys.io/ns/pgcat#>
";

/// (id, title, what it measures, SPARQL body)
const AGGREGATES: &[(&str, &str, &str, &str)] = &[
    ("classes", "Entities by class", "Every typed entity in the current build, by its ontology class.",
     "SELECT ?class (COUNT(?s) AS ?count) WHERE { ?s a ?class FILTER(STRSTARTS(STR(?class), \"https://ontosys.io/ns/\")) } GROUP BY ?class ORDER BY DESC(?count)"),
    ("areas", "C functions per code area", "Functions defined in files of each directory.",
     "SELECT ?area (COUNT(?f) AS ?functions) WHERE { ?f a cx:Function ; cx:definedIn ?file . ?file pg:inArea ?area } GROUP BY ?area ORDER BY DESC(?functions) LIMIT 30"),
    ("raised", "Most raised error conditions", "SQLSTATEs by number of functions whose body references the ERRCODE_ macro.",
     "SELECT ?sqlstate (COUNT(DISTINCT ?f) AS ?functions) WHERE { ?f pg:mayRaise ?sqlstate } GROUP BY ?sqlstate ORDER BY DESC(?functions) LIMIT 30"),
    ("settings", "Most read settings", "Configuration parameters by number of functions reading their C variable.",
     "SELECT ?setting (COUNT(DISTINCT ?f) AS ?readers) WHERE { ?f pg:readsSetting ?setting } GROUP BY ?setting ORDER BY DESC(?readers) LIMIT 30"),
    ("created", "Most created node types", "Node types by number of functions calling makeNode(X).",
     "SELECT ?node (COUNT(DISTINCT ?f) AS ?creators) WHERE { ?f pg:createsNode ?node } GROUP BY ?node ORDER BY DESC(?creators) LIMIT 30"),
    ("inspected", "Most inspected node types", "Node types tested or cast (IsA, castNode, T_X).",
     "SELECT ?node (COUNT(DISTINCT ?f) AS ?inspectors) WHERE { ?f pg:inspectsNode ?node } GROUP BY ?node ORDER BY DESC(?inspectors) LIMIT 30"),
    ("called", "Most called functions", "Functions by number of distinct callers.",
     "SELECT ?function (COUNT(DISTINCT ?caller) AS ?callers) WHERE { ?caller cx:calls ?function . ?function a cx:Function } GROUP BY ?function ORDER BY DESC(?callers) LIMIT 30"),
    ("complex", "Most complex functions", "Cyclomatic complexity (1 + branch points).",
     "SELECT ?function ?complexity WHERE { ?function cx:complexity ?complexity } ORDER BY DESC(?complexity) LIMIT 30"),
    ("hooks", "Hooks by functions touching them", "Functions that read or install each extension hook.",
     "SELECT ?hook (COUNT(DISTINCT ?f) AS ?functions) WHERE { ?f pg:touchesHook ?hook } GROUP BY ?hook ORDER BY DESC(?functions) LIMIT 40"),
    ("lookups", "Most referenced catalogs", "Catalogs by number of columns that look them up (BKI_LOOKUP).",
     "SELECT ?catalog (COUNT(?column) AS ?lookups) WHERE { ?column pg:lookupCatalog ?catalog } GROUP BY ?catalog ORDER BY DESC(?lookups) LIMIT 30"),
    ("sqlimpl", "SQL functions per implementing file", "Built-in SQL functions whose C implementation lives in each file.",
     "SELECT ?file (COUNT(DISTINCT ?sql) AS ?sqlFunctions) WHERE { ?f pg:implementsSQLFunction ?sql ; cx:definedIn ?file } GROUP BY ?file ORDER BY DESC(?sqlFunctions) LIMIT 30"),
];

fn aggregates(l: &Loaded) -> Value {
    let mut out = Vec::new();
    for (id, title, about, body) in AGGREGATES {
        let query = format!("{}{}", PREFIXES, body);
        match l.sparql(&query, 100) {
            Ok(r) if r["rows"].as_array().map_or(false, |a| !a.is_empty()) => out.push(json!({
                "id": id, "title": title, "about": about, "query": query,
                "vars": r["vars"], "rows": r["rows"], "elapsed_ms": r["elapsed_ms"],
            })),
            Ok(_) => {}
            Err(e) => out.push(json!({ "id": id, "title": title, "error": e.to_string() })),
        }
    }
    json!({ "items": out, "prefixes": PREFIXES })
}
