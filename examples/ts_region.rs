// Dev aid: show the top-level tree-sitter nodes covering a line range after the
// prepass. Usage: ts_region FILE FROM TO [ROOT] — macros are derived from ROOT's
// .c/.h files when given, else from FILE alone.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, from, to) = (&a[1], a[2].parse::<usize>().unwrap(), a[3].parse::<usize>().unwrap());
    let read = |p: &std::path::Path| String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned();
    let raw = read(std::path::Path::new(path));
    let sources: Vec<(String, String)> = match a.get(4) {
        Some(root) => walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| matches!(e.path().extension().and_then(|x| x.to_str()), Some("c" | "h")))
            .map(|e| (e.path().display().to_string(), read(e.path())))
            .collect(),
        None => vec![(path.clone(), raw.clone())],
    };
    let lists = ontosys::lang_c::prepass::derive_macros(&sources);
    let src = ontosys::lang_c::prepass::prepass(&raw, &lists);
    let mut p = ontosys::lang_c::new_parser();
    let t = p.parse(&src, None).unwrap();
    let root = t.root_node();
    let mut c = root.walk();
    for n in root.children(&mut c) {
        let (s, e) = (n.start_position().row + 1, n.end_position().row + 1);
        if e >= from && s <= to {
            println!("{:>5}-{:<5} {} {}", s, e, n.kind(), if n.has_error() { "(has error)" } else { "" });
        }
    }
}
