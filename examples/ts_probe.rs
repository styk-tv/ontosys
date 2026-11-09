// Dev aid: report where tree-sitter-c still fails on files after the generic
// prepass (macros derived from the tree's own #defines), aggregated by the
// first token of the offending line.
use std::collections::BTreeMap;
fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("repo root"));
    let mut by_tok: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut p = ontosys::lang_c::new_parser();
    let sources: Vec<(String, String)> = walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| matches!(e.path().extension().and_then(|x| x.to_str()), Some("c" | "h")))
        .map(|e| (e.path().display().to_string(), String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()).into_owned()))
        .collect();
    let lists = ontosys::lang_c::prepass::derive_macros(&sources);
    for (path, raw) in &sources {
        let path = std::path::Path::new(path);
        let src = ontosys::lang_c::prepass::prepass(raw, &lists);
        let tree = p.parse(&src, None).unwrap();
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            if n.is_error() || n.is_missing() {
                let line = src.lines().nth(n.start_position().row).unwrap_or("").trim().to_string();
                let tok: String = line.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '#')).find(|t| !t.is_empty()).unwrap_or("").to_string();
                let ent = by_tok.entry(tok).or_insert((0, String::new()));
                ent.0 += 1;
                if ent.1.is_empty() { ent.1 = format!("{}:{}: {}", path.strip_prefix(&root).unwrap().display(), n.start_position().row + 1, &line[..line.len().min(90)]); }
                continue;
            }
            if n.has_error() { let mut c = n.walk(); stack.extend(n.children(&mut c)); }
        }
    }
    let mut v: Vec<_> = by_tok.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (tok, (n, ex)) in v.into_iter().take(30) { println!("{:6} {:<28} {}", n, tok, ex); }
}
