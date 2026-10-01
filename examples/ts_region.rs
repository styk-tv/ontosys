// Dev aid: show the top-level tree-sitter nodes covering a line range after the prepass.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, from, to) = (&a[1], a[2].parse::<usize>().unwrap(), a[3].parse::<usize>().unwrap());
    let raw = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    let src = ontosys::grounding::postgres::prepass(&raw);
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
