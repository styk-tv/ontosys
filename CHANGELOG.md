# Changelog

## 0.9 — generic, with function complexity

- **No project-specific code.** The PostgreSQL grounding layer is removed
  (`pg:`/`pgcat:` terms, its views in serve, diff and the explorer). The C
  prepass derives loop and erased macros from the scanned tree's own
  `#define`s; C call resolution uses paths only. Any repository, or a
  carved-out part of one, builds the same way.
- **Complexity** for C, Rust, Python, JavaScript, TypeScript and TSX:
  structure per function in graph.nt; call graph, growth estimates (sound
  upper and lower classes, every unknown with its reason), loops, blockers,
  findings and claims in the new `data/complexity.nt`; the vocabulary and
  SHACL shapes in `ontology/code.ttl`; a cost catalog with a per-repository
  overlay; an unlock list in build-meta.
- **`ontosys survey`**: read every `.ontosys/` on a machine into a private
  ledger; progression report; rebuild in a temporary clone.
- build-meta names the ontosys commit that wrote it.
- C: `#define`s are extracted from `.c` files too; function-pointer
  parameters keep their names.

## 0.8

- Line-based parsers no longer claim an end line they do not know.
- build-meta records `git_head`, `git_dirty`, `graph_sha256`,
  `inputs_sha256` and `unparsed_by_extension`; `data/inputs.sha256` lists
  every file the build read.

## 0.7

- Non-C IRIs are file-scoped and percent-encoded; a build refuses to write a
  graph a strict N-Triples loader would refuse.
- Email addresses in literals can be redacted.
