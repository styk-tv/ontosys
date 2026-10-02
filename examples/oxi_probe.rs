// Dev aid: measure Oxigraph in-memory load + query cost for ontosys graphs.
use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::NamedNode;
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let store = Store::new().unwrap();
    let t = Instant::now();
    let cur = std::fs::read(&a[1]).unwrap();
    let mut l = store.bulk_loader();
    l.load_from_slice(RdfParser::from_format(RdfFormat::NTriples).unchecked(), &cur).unwrap();
    l.commit().unwrap();
    println!("current loaded in {:?}", t.elapsed());
    if let Some(base) = a.get(2) {
        let t = Instant::now();
        let b = std::fs::read(base).unwrap();
        let mut l = store.bulk_loader();
        l.load_from_slice(RdfParser::from_format(RdfFormat::NTriples).unchecked().with_default_graph(NamedNode::new("urn:ontosys:baseline").unwrap()), &b).unwrap();
        l.commit().unwrap();
        println!("baseline loaded in {:?}", t.elapsed());
    }
    println!("quads {}", store.len().unwrap());
    for q in [
        "SELECT ?c (COUNT(?s) AS ?n) WHERE { ?s a ?c } GROUP BY ?c ORDER BY DESC(?n) LIMIT 5",
        "PREFIX pg: <https://ontosys.io/ns/pg#> SELECT ?st (COUNT(DISTINCT ?f) AS ?n) WHERE { ?f pg:mayRaise ?st } GROUP BY ?st ORDER BY DESC(?n) LIMIT 5",
        "SELECT (COUNT(*) AS ?removed) WHERE { GRAPH <urn:ontosys:baseline> { ?s ?p ?o } MINUS { ?s ?p ?o } }",
    ] {
        let t = Instant::now();
        if let QueryResults::Solutions(sol) = SparqlEvaluator::new().parse_query(q).unwrap().on_store(&store).execute().unwrap() {
            let rows: Vec<_> = sol.map(|r| r.unwrap()).collect();
            println!("{:>8.0?}  {} rows; first: {:?}", t.elapsed(), rows.len(), rows.first().map(|r| r.iter().map(|(v, t)| format!("{}={}", v.as_str(), t)).collect::<Vec<_>>()));
        }
    }
}
