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
const LOGIC_JS: &str = include_str!("viz/logic.js");
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

    let current = if repo_path.is_file() {
        repo_path.to_path_buf()
    } else {
        let root = find_git_root(repo_path).unwrap_or_else(|| repo_path.to_path_buf());
        super::diff::resolve_graph(&root)?
    };
    let baseline = compare.as_deref().map(super::diff::resolve_graph).transpose()?;
    info(&format!("Graph:    {}", current.display()));
    if let Some(b) = &baseline {
        info(&format!("Baseline: {}", b.display()));
    }
    info("Loading into the SPARQL store...");

    let state = tokio::task::spawn_blocking(move || build_state(&current, baseline.as_deref())).await??;

    success(&format!(
        "Loaded {} triples{} in {} ms; view: {} ({} nodes)",
        state.loaded.triples_current,
        if state.loaded.triples_baseline > 0 { format!(" + baseline {}", state.loaded.triples_baseline) } else { String::new() },
        state.loaded.load_ms,
        state.view["mode"].as_str().unwrap_or("?"),
        state.view["nodes"].as_array().map_or(0, |a| a.len()),
    ));

    let app = router(Arc::new(state));

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

/// Load the store and precompute what every page load needs.
fn build_state(current: &Path, baseline: Option<&Path>) -> anyhow::Result<AppState> {
    let loaded = Loaded::load(current, baseline)?;
    let overview: Vec<String> = if loaded.tbox.contains_key(&expand("pg:SystemCatalog")) {
        crate::grounding::postgres::OVERVIEW_CLASSES.iter().map(|c| format!("pg:{}", c)).collect()
    } else {
        Vec::new()
    };
    let view = loaded.view(&overview, MAX_FULL_VIEW);
    let aggregates = aggregates(&loaded);
    let meta: Value = current
        .parent()
        .map(|d| d.join("build-meta.json"))
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
}

fn router(state: Shared) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/logic.js", get(|| async { ([(axum::http::header::CONTENT_TYPE, "text/javascript")], LOGIC_JS) }))
        .route("/favicon.ico", get(|| async { ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], FAVICON) }))
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
        .with_state(state)
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

// ----------------------------------------------------------------------------
// API tests — the HTTP contract the explorer relies on, over a small fixture
// (tests/fixtures/{current,baseline}.nt; regenerate with tests/fixtures/gen.py)
// ----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::collections::BTreeSet;
    use std::sync::OnceLock;
    use tower::ServiceExt;

    const ID: &str = "https://ontosys.io/id/fixture/";

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    fn app() -> Router {
        static APP: OnceLock<Router> = OnceLock::new();
        APP.get_or_init(|| {
            let st = build_state(&fixture("current.nt"), Some(&fixture("baseline.nt"))).expect("fixture loads");
            router(Arc::new(st))
        })
        .clone()
    }

    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) })
            .collect()
    }

    async fn call(req: Request<Body>) -> (StatusCode, String, Vec<u8>) {
        let res = app().oneshot(req).await.unwrap();
        let status = res.status();
        let ctype = res.headers().get("content-type").map(|v| v.to_str().unwrap_or("").to_string()).unwrap_or_default();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec();
        (status, ctype, bytes)
    }

    async fn get(uri: &str) -> (StatusCode, Value) {
        let (s, _, b) = call(Request::get(uri).body(Body::empty()).unwrap()).await;
        (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
    }

    async fn node(iri: &str, n: usize) -> Value {
        let (s, v) = get(&format!("/api/node?iri={}&n={}", enc(iri), n)).await;
        assert_eq!(s, StatusCode::OK, "{}", v);
        v
    }

    fn group<'a>(v: &'a Value, dir: &str, p_suffix: &str) -> &'a Value {
        v[if dir == "in" { "incoming" } else { "outgoing" }]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["p"].as_str().unwrap().ends_with(p_suffix))
            .unwrap_or_else(|| panic!("no {} group {} in {}", dir, p_suffix, v))
    }

    #[tokio::test]
    async fn info_reports_both_builds() {
        let (s, v) = get("/api/info").await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["triples"], 125);
        assert_eq!(v["baseline_triples"], 128);
        assert_eq!(v["version"], "fix2");
        assert_eq!(v["baseline"], "fix1");
    }

    #[tokio::test]
    async fn view_includes_removed_added_changed_without_duplicate_links() {
        let (_, v) = get("/api/view").await;
        assert_eq!(v["mode"], "full");
        let status = |iri: &str| v["nodes"].as_array().unwrap().iter().find(|n| n["iri"] == iri).map(|n| n["status"].clone());
        assert_eq!(status(&format!("{}pg/catalog/pg_demo/column/c", ID)), Some(json!("removed")));
        assert_eq!(status(&format!("{}fn/src/demo.c/f21", ID)), Some(json!("removed")));
        assert_eq!(status(&format!("{}fn/src/demo.c/f20", ID)), Some(json!("added")));
        assert_eq!(status(&format!("{}fn/src/demo.c/f01", ID)), Some(json!("changed")));
        assert_eq!(status(&format!("{}pg/sqlstate/22012", ID)), Some(json!("unchanged")));
        assert!(!v["nodes"].as_array().unwrap().iter().any(|n| n["types"].to_string().contains("owl:")), "vocabulary is not drawn");
        let edges = v["edges"].as_array().unwrap();
        let unique: BTreeSet<String> = edges.iter().map(|e| format!("{}|{}|{}", e["source"], e["p"], e["target"])).collect();
        assert_eq!(unique.len(), edges.len(), "each link once");
        let removed_edge = edges.iter().any(|e| e["target"] == format!("{}pg/catalog/pg_demo/column/c", ID) && e["status"] == "removed");
        assert!(removed_edge, "the removed column's link is drawn as removed");
    }

    #[tokio::test]
    async fn node_reports_large_fanout_with_total_and_sample() {
        let v = node(&format!("{}pg/sqlstate/22012", ID), 5).await;
        let g = group(&v, "in", "#mayRaise");
        assert_eq!(g["total"], 20, "true total, for the group node");
        assert_eq!(g["items"].as_array().unwrap().len(), 5, "sample limited by n");
        assert_eq!(g["label"], "may raise");
        let grounded: Vec<&str> = v["grounding"].as_array().unwrap().iter().filter_map(|x| x["groundedIn"].as_str()).collect();
        assert!(grounded.contains(&"src/backend/utils/errcodes.txt"));
    }

    #[tokio::test]
    async fn node_lists_small_fanout_completely() {
        let v = node(&format!("{}pg/catalog/pg_demo", ID), 30).await;
        let g = group(&v, "out", "#hasColumn");
        assert_eq!(g["total"], 2);
        let labels: Vec<&str> = g["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert_eq!(labels, vec!["pg_demo.a", "pg_demo.b"]);
    }

    #[tokio::test]
    async fn links_page_through_a_group_without_overlap() {
        let mut seen = BTreeSet::new();
        for off in [0, 7, 14] {
            let (s, v) = get(&format!("/api/links?iri={}&p={}&dir=in&offset={}&limit=7",
                enc(&format!("{}pg/sqlstate/22012", ID)), enc("https://ontosys.io/ns/pg#mayRaise"), off)).await;
            assert_eq!(s, StatusCode::OK);
            for i in v["items"].as_array().unwrap() {
                assert!(seen.insert(i["iri"].as_str().unwrap().to_string()), "page overlap at offset {}", off);
            }
        }
        assert_eq!(seen.len(), 20);
    }

    #[tokio::test]
    async fn removed_entity_is_served_from_the_baseline() {
        let v = node(&format!("{}pg/catalog/pg_demo/column/c", ID), 10).await;
        assert_eq!(v["side"], "baseline");
        assert_eq!(v["status"], "removed");
        assert!(!v["delta"]["changes"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn changed_entity_carries_its_delta() {
        let v = node(&format!("{}fn/src/demo.c/f01", ID), 10).await;
        assert_eq!(v["status"], "changed");
        let calls = v["delta"]["changes"].as_array().unwrap().iter().find(|c| c["p"] == "cx:calls").expect("calls changed");
        assert_eq!(calls["removed"], json!(["f03"]));
        assert_eq!(calls["added"], json!(["f02"]));
    }

    #[tokio::test]
    async fn search_ranks_exact_then_prefix_across_both_builds() {
        let (_, v) = get("/api/search?q=f0&limit=50").await;
        assert!(v["total"].as_u64().unwrap() >= 9);
        let (_, v) = get("/api/search?q=pg_demo").await;
        assert_eq!(v["items"][0]["label"], "pg_demo", "exact match first");
        let (_, v) = get("/api/search?q=f21").await;
        assert_eq!(v["items"][0]["status"], "removed", "baseline-only entities are findable");
    }

    #[tokio::test]
    async fn sparql_select_ask_post_and_errors() {
        let q = "SELECT (COUNT(?f) AS ?n) WHERE { ?f a <https://ontosys.io/ns/c#Function> }";
        let (s, v) = get(&format!("/api/sparql?query={}", enc(q))).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["rows"][0]["n"]["value"], "20");
        let (_, v) = get(&format!("/api/sparql?query={}", enc("ASK { ?s ?p ?o }"))).await;
        assert_eq!(v["boolean"], true);
        let (s, _, b) = call(Request::post("/api/sparql").body(Body::from(
            "SELECT (COUNT(*) AS ?removed) WHERE { GRAPH <urn:ontosys:baseline> { ?s ?p ?o } MINUS { ?s ?p ?o } }")).unwrap()).await;
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["rows"][0]["removed"]["value"], "11", "matches the line diff of the fixture files");
        let (s, v) = get(&format!("/api/sparql?query={}", enc("SELEC nope"))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(v["error"].as_str().unwrap().contains("syntax"));
    }

    #[tokio::test]
    async fn delta_summary_and_lists() {
        let (_, v) = get("/api/delta").await;
        assert_eq!(v["baseline"], "fix1");
        assert_eq!(v["current"], "fix2");
        let f = v["sections"].as_array().unwrap().iter().find(|s| s["class"] == "cx:Function").unwrap();
        assert_eq!((f["added"].as_u64(), f["removed"].as_u64(), f["changed"].as_u64()), (Some(1), Some(1), Some(1)));
        let (_, v) = get("/api/delta/list?class=cx:Function&status=removed").await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["label"], "f21");
    }

    #[tokio::test]
    async fn bad_iri_is_rejected() {
        let (s, v) = get("/api/node?iri=not%20an%20iri").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(v["error"].is_string());
    }

    #[tokio::test]
    async fn ui_assets_are_served() {
        let (s, ctype, b) = call(Request::get("/").body(Body::empty()).unwrap()).await;
        let html = String::from_utf8(b).unwrap();
        assert_eq!(s, StatusCode::OK);
        assert!(ctype.starts_with("text/html"));
        for marker in ["id=\"viewmode\"", "id=\"plabels\"", "src=\"/logic.js\""] {
            assert!(html.contains(marker), "page lacks {}", marker);
        }
        let (s, ctype, b) = call(Request::get("/logic.js").body(Body::empty()).unwrap()).await;
        assert_eq!(s, StatusCode::OK);
        assert!(ctype.starts_with("text/javascript"));
        assert!(String::from_utf8(b).unwrap().contains("GROUP_MAX"));
    }
}
