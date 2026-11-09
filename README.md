# OntoSys

A multi-language code knowledge graph system that extracts semantic information from Git repositories, maps to formal ontologies (BFO 2020 / CodeOntology), and provides interactive visualization.

## Features

- **Multi-Language Support**: C (tree-sitter syntax trees), Rust, Python, TypeScript, JavaScript, plus a Markdown docs layer
- **Generic C**: macros that defeat a syntax tree (loop headers, annotations) are derived from the scanned tree's own `#define`s — no project-specific code, any repository or any carved-out part of one
- **Function Complexity**: structure (cyclomatic, cognitive, nesting, loops), call-graph relationships, growth estimates (O, Ω) read from the syntax, each with the assumptions it rests on and every unknown counted by its reason, and readings tied to a definition's hash — see [Complexity](#complexity) and [Known Limitations](#known-limitations)
- **Survey**: `ontosys survey` reads every `.ontosys/` folder on a machine into a private ledger and reports progression
- **Semantic Diff**: `ontosys diff` compares two builds entity by entity, in ontology terms
- **Deterministic Output**: version-independent IRIs and canonical triple order — identical sources give byte-identical graphs
- **Formal Ontologies**: Based on CodeOntology with BFO 2020 foundations
- **Type-Safe Pipeline**: Compile-time guarantees via type-state pattern
- **Interactive Visualization**: Full-screen graph browser with filtering
- **RDF Export**: Turtle, N-Triples, JSON-LD formats

## Quick Start

```bash
# Build the CLI
cargo build --release

# Navigate to any git repository
cd /path/to/your/project

# Initialize .ontosys/ folder
ontosys init

# Build the knowledge graph
ontosys build

# Start the visualization server
ontosys serve --open
```

## CLI Commands

```
USAGE:
    ontosys [OPTIONS] <COMMAND>

COMMANDS:
    init      Initialize .ontosys/ folder in a git repository
    build     Build/rebuild the knowledge graph from source files
    stats     Show statistics about the knowledge graph
    serve     Explore the graph in the browser (SPARQL-backed; --compare for a delta)
    query     Run a SPARQL query against the built graph
    export    Export the graph to a file
    watch     Watch for file changes and rebuild automatically
    diff      Semantic diff of two builds (repositories or graph.nt files)
    survey    Read every .ontosys/ under some roots into a ledger; report progression
    clean     Clean the .ontosys folder

OPTIONS:
    -p, --path <PATH>    Path to repository (defaults to current directory)
    -v, --verbose        Verbose output
```

### Examples

```bash
# Initialize with verbose output
ontosys init -v

# Build only specific languages
ontosys build --languages rust,python

# Build with 8 parallel workers
ontosys build --jobs 8

# Start server on custom port
ontosys serve --port 8080 --open

# SPARQL over the built graph (table, csv or json)
ontosys query 'PREFIX cx: <https://ontosys.io/ns/c#>
  SELECT ?f ?c WHERE { ?f cx:complexity ?c } ORDER BY DESC(?c) LIMIT 10'

# Explore a build, with another build of the same project as the baseline
ontosys serve --open --compare ../project-v1

# Compare two builds of the same project semantically
ontosys diff ../project-v1 ../project-v2 --out report.md --json report.json

# Export as Turtle
ontosys export ./graph.ttl --format turtle

# Show stats
ontosys stats
```

## .ontosys/ Folder Structure

```
.ontosys/
├── config.json       # Configuration
├── cost-catalog.ttl  # Optional: this repository's own costs and size caps
├── claims/
│   └── complexity.ttl # Optional: readings of functions' growth
├── data/
│   ├── graph.ttl     # RDF in Turtle format
│   ├── graph.nt      # RDF in N-Triples format
│   ├── complexity.nt # Call graph, growth estimates, loops, blockers, findings, claims
│   ├── code.ttl      # The vocabulary (and SHACL shapes) of every code# term
│   ├── prepass.json  # C: the loop and erased macros derived from the tree
│   ├── graph.json    # JSON-LD for visualization (an overview above 250k triples)
│   ├── docs.json     # Comments + markdown docs metadata
│   ├── inputs.sha256 # sha256sum list of every file the build read
│   └── build-meta.json
├── cache/            # Parsed AST cache
├── exports/          # Exported files
└── viz/              # Visualization assets
```

## Supported Languages

| Language | Extensions | Extraction |
|----------|-----------|------------|
| C | `.c`, `.h` | tree-sitter syntax tree: functions (signature, header comment, calls, identifiers, diagnostic messages, body hash), prototypes, structs/fields, enums, typedefs, macros, globals, includes |
| Rust | `.rs` | Declarations (line-based) |
| Python | `.py` | Declarations (line-based), including module-level assignments: `UPPER_CASE` names as constants, others as variables |
| TypeScript | `.ts`, `.tsx` | Declarations (line-based) |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | Declarations (line-based) |
| Markdown | `.md` | Docs layer: ATX-heading outline + verbatim content into `docs.json` |

C is parsed into a real syntax tree with tree-sitter-c. The other languages
use line-based heuristic extraction of declarations (functions,
structs/classes, traits, enums, constants, imports, …) — not a full syntax
tree. Every declaration carries its location: `filePath` relative to the
repository and start line. The end line is written only where the parser
knows it (C, and multi-line Python module assignments); a line-based parser
never guesses a one-line span. Python lines inside strings, brackets or
backslash continuations are never read as declarations; module-level
assignments follow the rule of Python's own `ast` (each target of `A = B = …`
and `NAME: T = …`; not tuple, attribute or augmented targets, nor anything
nested in a block).

The project is what git considers the repository: files ignored by
`.gitignore`, `.git/info/exclude` or the global excludes (a virtualenv, build
output) are not read; hidden but tracked directories such as `.github/` are.

`build-meta.json` names what a build read and wrote: `ontosys_version`,
`timestamp`, `git_head` and `git_dirty` (HEAD's commit, and whether the
working tree differed from it, `.ontosys/` excluded), `graph_sha256` (of
`graph.nt` as written; `graph.nt` is sorted and deterministic, so the same
tree, version and config give the same bytes), `inputs_sha256` (of
`inputs.sha256`), and `unparsed_by_extension` (files inside the repository
that no parser read, so silence is never mistaken for coverage). From the
repository root, `sha256sum -c .ontosys/data/inputs.sha256` names every input
that changed since the build. It also names the tool: `ontosys_commit` and
`ontosys_dirty` (the ontosys build that wrote the folder), `complexity_sha256`
and `complexity_method`, and a `complexity` summary (per language: functions
analysed and attached; functions by estimate basis, by reason an upper bound
is missing and by assumption; loops by bound class; findings; claims; and the
unlock list).

## Complexity

ontosys keeps facts about a function apart from estimates about it. All terms
are in `https://ontosys.io/ns/code#` and declared, with SHACL shapes, in
`ontology/code.ttl` (copied to `.ontosys/data/code.ttl`).

- **Structure (graph.nt)**, per function: `code:statementCount`,
  `code:cyclomatic` (1 + decision points), `code:cognitive`,
  `code:maxNesting`, `code:loopCount`, `code:maxLoopNesting`,
  `code:selfCalls` and `code:definitionSha256` (the definition's bytes as they
  are in the file, from its first decorator or attribute). One engine walks
  any tree-sitter function body; a profile per grammar (C, Rust, Python,
  JavaScript, TypeScript, TSX) names its decision points, loops and calls.
  Named nested functions are their own functions; closures, lambdas and arrow
  functions count inside the function that contains them. For Rust, Python
  and JS/TS, the facts attach to the line parsers' declarations by the line
  of each function's name; what cannot be attached is counted by cause.
- **Relationships (complexity.nt)**: `code:fanIn`, `code:fanOut`,
  `code:externalCallCount`, call cycles (`code:CallCycle`), and loop depth
  through repository calls. C macro calls link to their definitions
  (`code:resolvesTo`).
- **Growth (complexity.nt)**, one `code:GrowthEstimate` per function: an
  upper class (O(1) … O(n^k), O(b^n), rounded up, exact form kept) only when
  nothing in the body is unknown; a lower class counting only work that runs
  on every path; the size expressions as written; every assumption the bounds
  rest on; and, when there is no upper bound, every reason why
  (`code:blockedBy`: an uncosted, indirect, ambiguous or cyclic call, a
  blocked callee, a data-dependent or unbounded loop, unstructured flow, or
  parse errors). Loops, blockers and findings (a linear call in a loop
  condition, list membership, concatenation or a sort inside a loop, nested
  loops over one collection, recursion that recomputes) are evidence nodes.

  The estimates are read from the syntax, not proved. A counted loop is
  classified by its start, the direction of its step and its whole bound
  (`for (i = n; i > 0; i--)` is linear in `n`; a bound of `2 * n` is not a
  constant); a loop that runs while a value holds counts only when its body
  advances that value along a structure (`p = p->next`, `stack.pop()`), and
  a bound whose collection grows in the body is data-dependent. A loop that
  may run zero times (an unknown start, a generator expression, a constant
  range that is empty) adds nothing to the lower bound; neither does code
  after a `continue` or `break` in its loop, inside a lambda, or after a
  `return` inside a loop. Calling a generator or async function costs its
  caller no lower bound. A function handed to a higher-order call (`qsort`'s
  comparator, `map(f, xs)`, a lambda given to `sort`) is charged as many
  times as the catalog says the call may run it; a callback the analysis
  cannot name is an indirect call. Each classified loop records the
  `LoopShape` assumption: its bound holds only if nothing outside the text
  changes the loop's variables.
- **Costs** come from `ontology/cost-catalog.ttl` (worst case first, for any
  implementation; average and amortized costs are recorded, never used as
  bounds), from C macro bodies, and from a repository's own
  `.ontosys/cost-catalog.ttl`. An overlay entry states the cost of one of the
  repository's own functions or macros for its callers (recorded as the
  `OverlayCost` assumption; the function's own estimate stays derived from
  its body), and a size cap applies to a size only when every name in it is
  capped:

  ```turtle
  @prefix code: <https://ontosys.io/ns/code#> .
  [] a code:SizeCap ; code:sizeName "npos" ; code:cappedBy "MAXNUMPOS" .
  [] a code:CostEntry ; code:language "c" ; code:operation "find_entry" ;
     code:worstClass code:OLogN ; code:sizeOf "arg1" ; code:source "read 2026-11" .
  ```

  The build-meta unlock list names which uncatalogued callees keep the most
  functions without an upper bound, so the most useful entries are obvious.
  build-meta also records the overlay's and the claims' sha256 (their edits
  are input changes, not tool changes) and the time each phase took
  (`timings_ms`, with `call_edges`).
- **Claims**: a reading of a function, written to
  `.ontosys/claims/complexity.ttl` with the `code:definitionSha256` it was made
  against, is copied into complexity.nt on every build as current while the
  definition is byte-for-byte unchanged, and stale after.

## Survey

```bash
ontosys survey ~/code --depth 4            # read every .ontosys/ below ~/code
ontosys survey --report                    # progression per folder
ontosys survey --rebuild ~/code/some-repo  # measure HEAD in a temporary clone
```

`survey` only reads. Each folder gets one snapshot appended to
`~/.ontosys/survey/ledger.jsonl` (`--ledger` to choose): which ontosys built
it, from which tree, its digests, functions per language, complexity spreads
and summary, and its staleness (`stale-tool`, `stale-tree`, `no-meta`,
`current`). `--report` compares each folder's latest snapshot with the
previous one made by the same ontosys commit, so a tool change is never read
as a code change. `--rebuild` builds a repository's HEAD in a temporary clone
and leaves the owner's tree untouched.

C extraction details:

- **Body hash** — FNV-1a over the non-comment tokens of a function body: it
  changes when the code changes, not when comments, whitespace or line
  positions do.
- **Call resolution** — by paths only, so any layout resolves the same way: a
  definition in the caller's own file, then a static inline in a header, then
  the single definition whose directory shares the most leading directories
  with the caller's, then the single definition anywhere; anything else
  becomes `cx:ExternalSymbol`.
- **Parse recovery** — a length-preserving prepass derived from the scanned
  tree's own `#define`s: a function-like macro whose expansion opens a
  `for`/`while` header (`foreach(lc, list)`) is rewritten to
  `while (lc, list)`, and a macro that expands to nothing or only to compiler
  attributes is blanked (preprocessor lines keep their bytes, so include
  guards survive). `#ifdef` alternatives must agree, and macros defined
  through other macros are followed. The derived lists are written to
  `prepass.json`, with counts in `build-meta.json`, including indented
  `NAME(args)` statements whose macro no scanned file defines (a carved-out
  section's blind spot). Files that still have errors are re-parsed with
  conditional-compilation lines and bare `MACRO(args)` lines blanked, and the
  cleaner parse is kept. Each file records `cx:parseErrors`.

## Semantic Diff

```bash
ontosys diff path/to/old-checkout path/to/new-checkout --out report.md --json report.json
```

Both arguments may be repositories (their `.ontosys/data/graph.nt` is used) or
`.nt` files. Because instance IRIs are built from the project name and
repository paths / natural keys only, the same entity has the same IRI in both
builds, and the comparison is a streaming merge of two sorted files.
Differences are grouped by entity and reported by kind — types, macros,
globals, files, then C functions per directory — with each changed function
classified as a *contract*, *role*, *dependencies*, *implementation* or
*docs* change. Line-number
facts are ignored, and functions that move between files unchanged are
reported as moves.

The project name in IRIs comes from `project` in `.ontosys/config.json`, else
the basename of `remote.origin.url`, else the directory name — so two
worktrees of one repository produce comparable graphs.

## Loading into an RDF Store

`graph.nt` is canonical N-Triples — byte-sorted, de-duplicated, no blank
nodes — so its SHA-256 is a content address for the graph. It is
byte-identical to pgRDF's own canonical export of the loaded graph, which
makes the producer's digest usable for content-addressed admission and makes
"was the load lossless?" a hash comparison:

```bash
sha256sum .ontosys/data/graph.nt      # equals the store's canonical-N-Triples digest after loading
```

pgRDF, from SQL (the file must be readable by the database server):

```sql
SELECT pgrdf.add_graph('urn:ontosys:myproject:v1');
SELECT pgrdf.load_turtle('/path/visible/to/server/graph.ttl',
                         pgrdf.graph_id('urn:ontosys:myproject:v1'));
```

or through the pgRDF MCP server's `pgrdf_import` (`file` + `expect_sha256`).
Load each version into its own named graph and scope every query with
`GRAPH <…>`. Cross-version questions are cheapest as single-triple `MINUS`:

```sparql
SELECT ?s ?p ?o WHERE {
  GRAPH <urn:ontosys:myproject:v1> { ?s ?p ?o }
  MINUS { GRAPH <urn:ontosys:myproject:v2> { ?s ?p ?o } }
}
```

On PostgreSQL 19 (≈1.1M triples per version) this reproduces `ontosys diff`'s
line-level delta exactly (22,584 removed / 15,188 added / 1,102,763 common
between 19beta3 and 19beta4). Unbounded property paths over `cx:calls`
(`cx:calls+`) are expensive on graphs this size — the exact function-only
closure of PostgreSQL 19 is ≈21.5M pairs — so prefer bounded hops, or compute
reachability outside the store.

## Known Limitations

- **Call resolution across link units.** Resolution sees paths, not build
  targets: the nearest-directory and last-resort rules ignore which binary the
  caller links into, and that a callee may be a function-like macro visible
  through the caller's includes, so a call can link to a same-named function
  of another program. Planned fix: no cross-link-unit fallback (unresolved →
  `cx:ExternalSymbol`), macro calls on their own predicate, link units from
  build introspection when available, and `cx:callsExternal` for external
  symbols so `cx:calls` is function→function only.
- **Macro-generated and `#ifdef`-split definitions** are not seen (≈0.08% of
  function definitions on PostgreSQL 19, measured against an independent
  count).
- **Growth estimates are syntactic.** They rest on the assumptions each
  estimate records, and on these limits:
  - *One size variable.* Every size is `n`, so `O(n·m)` reads `O(n²)`; the
    size expressions name each one.
  - *Receivers of unknown type* take the worst catalog entry of the name:
    `x in y` is `O(n²)` because `y` may be a string (substring search).
  - *Element comparisons cost O(1).* Sorting strings, or `in` over a list of
    strings, does not count the length of each element.
  - *Operators are not costed*: comparing sequences (`a == b` on lists),
    `del`, slice assignment, `+` and `*` on sequences, spread (`...xs`),
    dispatch to a class's `__len__`/`__str__`/`__eq__` through `len()`,
    `str()` or `==`, and big-integer arithmetic (`pow` is uncatalogued).
  - *Aliases and callees.* A collection grown through another name, or a
    loop variable changed by a callee, is not seen; `LoopShape` says so.
  - *Accumulation* is modelled for catalog calls that grow an argument
    (`strcat`, `strncat`); string `+=` in a loop is a finding, not a bound.
  - *Iterators.* A `for` over an iterator is linear in its size, which may be
    infinite (`itertools.count()`); `next()` is O(1).
  - *Name resolution.* Rust, Python and JS/TS calls resolve by a unique name
    within the language, so a same-named function elsewhere may be bound; C
    picks the nearest directory (see above). A name defined by a function the
    scan could not attach is uncosted.
  - *Early exits through callees* (`exit`, `abort`, `panic!`, `longjmp`,
    exceptions) do not lower the lower bound.
  - *Macros.* Rust macro arguments are re-read as expressions; a macro whose
    arguments are not expressions but hold a call is an indirect call. A C
    loop macro is classified by its own expanded header; one whose name is
    under five characters cannot be rewritten in place (listed as
    `short_loop_macros` in build-meta), so its uses stay parse errors.
- The `type_state_demo` and `ingest_repo` examples do not compile (they
  predate the current module layout).

## Ontology Mappings

### Node Types

| Code Construct | RDF Type | Properties |
|----------------|----------|------------|
| Function/Method | `co:Function` | name, visibility, isAsync, parameters, returnType |
| Class/Struct | `code:Struct` | name, visibility, fields, generics |
| Trait/Interface | `code:Trait` | name, supertraits, methods |
| Enum | `code:Enum` | name, variants |
| Module | `co:Module` | name, visibility |
| Import | `code:Import` | path, alias |

### Relationships

| Predicate | Domain | Range | Description |
|-----------|--------|-------|-------------|
| `code:contains` | Container | Entity | Containment relationship |
| `code:calls` | Function | Function | Call graph edge |
| `code:implements` | Struct | Trait | Implementation relationship |
| `code:hasField` | Struct | Field | Field membership |
| `code:hasParameter` | Function | Parameter | Parameter membership |

## Explorer (`ontosys serve`)

`serve` loads the full `graph.nt` into an embedded, in-memory SPARQL store
([Oxigraph](https://github.com/oxigraph/oxigraph)) and opens a browser
explorer over it. With `--compare <other checkout or graph.nt>` a second build
is loaded as the named graph `urn:ontosys:baseline`, and every entity carries
its delta — the same computation `ontosys diff` reports.

- **Explore**: search every label in both builds; the selected entity shows
  what its class means (label and comment from the vocabulary), its
  change between builds, its properties, and its links grouped by predicate,
  outgoing and incoming. Each link is a step: it brings the target onto the
  canvas, connects it and focuses it, with back/forward history and a
  shareable URL.
- **Canvas**: the whole graph when small; otherwise a **class map**: one node per class, sized by its number of
  entities, linked by the relations between classes with their counts;
  clicking a class draws its entities a page at a time. Delta colouring and a
  "changed only" view.
- **Browse**: every class with its count; a class lists its entities by name,
  filterable, a page at a time — the way through any graph.
- **Stats**: SPARQL aggregates over the full graph, computed at startup —
  entities by class, triples by predicate, most linked entities, most called
  and most complex functions — every row clickable.
- **Delta**: added / removed / changed per kind, browsable lists.
- **SPARQL**: any read-only query, with examples; IRIs in results are links.
- **Groups and links**: a predicate with more than 12 targets is drawn as one
  group node ("← may raise · 20") that draws its members page by page when
  clicked; links are coloured by family (structure, reference, behaviour,
  SQL surface, type) with the same colour on the panel's group headers;
  *Link names* shows predicate names on the selection's links (default), on
  all links, or off — and always on hover.
- **View**: *all* (everything drawn so far; the settled layout stays frozen so
  selecting never moves the picture), *trail* (only the selections you walked
  through plus the current neighbourhood) or *focus* (only the current
  selection and its neighbours, laid out for reading). The choice is
  remembered; *Relayout* recomputes the layout on demand.
- Light / medium / dark themes (medium by default).

The browser only ever talks to the `serve` API; it never reads graph files or
a database directly. The store behind the API is embedded Oxigraph — pure
Rust, in memory, built without RocksDB, so no C++ toolchain is involved. A
pgRDF-backed store (querying a pgRDF database through the same API) is the
planned second backend.

`--overview-above <triples>` (default 250,000) sets when the overview replaces
the whole graph. Lines that are not valid N-Triples are skipped and counted
(shown on the canvas and at startup) rather than refusing the file.

HTTP API (JSON): `/api/info`, `/api/view`, `/api/classes`,
`/api/class?c=&q=&offset=`, `/api/node?iri=`,
`/api/links?iri=&p=&dir=`, `/api/search?q=`, `/api/aggregates`, `/api/delta`,
`/api/delta/list?class=&status=`, `/api/sparql?query=` (GET or POST).

PostgreSQL 19 (≈1.1M triples per build): both builds load in ≈3.5 s and use
≈2.4 GB of memory; aggregates take milliseconds, and the whole-graph
`baseline MINUS current` comparison ≈0.8 s.

## Type-Level Safety Patterns

This project demonstrates advanced Rust patterns:

### Type-State Pattern

```rust
// Pipeline states prevent invalid transitions at compile time
let pipeline = Pipeline::new()
    .discover(repo_path)?     // Uninitialized → Discovered
    .parse()?                 // Discovered → Parsed
    .build()?                 // Parsed → Built
    .validate()?              // Built → Validated
    .ready();                 // Validated → Ready

// This won't compile:
// Pipeline::new().parse()?  // ❌ Can't parse without discovering!
```

### Capability Tokens

```rust
// Zero-cost proofs that operations are authorized
mod capabilities {
    pub struct CanParse(());  // Private = unforgeable

    pub(crate) fn grant_parse() -> CanParse { CanParse(()) }
}

fn parse_file(_proof: &CanParse, source: &str) -> AST {
    // Caller MUST have capability token
}
```

### Sealed Traits

```rust
mod private { pub trait Sealed {} }

// Only types in this crate can implement CodeEntity
pub trait CodeEntity: private::Sealed {
    fn class_iri() -> Iri;
}
```

## Namespace Prefixes

```turtle
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix co: <http://codeontology.org/ontology/> .
@prefix code: <http://example.org/code/> .
@prefix data: <http://example.org/data/> .
@prefix prov: <http://www.w3.org/ns/prov#> .
@prefix doap: <http://usefulinc.com/ns/doap#> .
@prefix cx: <https://ontosys.io/ns/c#> .          # C constructs
@prefix oc: <https://ontosys.io/ns/code#> .       # complexity (ontology/code.ttl)
```

Instance IRIs: `https://ontosys.io/id/<project>/<kind>/<repository path or natural key>`.

## Tests

```bash
cargo test --all-targets                    # includes the explorer's HTTP API contract tests
node --test tests/ui/logic.test.cjs         # explorer decision logic (grouping, link identity, families, views)
cargo build --release && cd tests/ui && npm ci && npx playwright install chromium && npx playwright test
```

The API and end-to-end tests run against a small two-build fixture in
`tests/fixtures/` (`current.nt` / `baseline.nt`, regenerated by
`python3 tests/fixtures/gen.py`): a SQLSTATE raised by 20 functions (above the
group threshold), a catalog with two columns, and one added, removed and
changed entity each. The end-to-end suite covers selection stability (nothing
else moves, no duplicate links), group nodes, link families and names, the
"Changed only" fading regression, remembered view choice, and baseline-only
entities. CI runs all three layers.

## Requirements

- Rust 1.75+ (for building)
- Git repository (for initialization)

## Install

A release publishes one self-contained binary per architecture — no archive to
unpack, no runtime dependencies.

```bash
# amd64 / arm64 detected automatically
curl -fsSL https://raw.githubusercontent.com/styk-tv/ontosys/main/install.sh | sh

# or pick the asset directly
#   https://github.com/styk-tv/ontosys/releases/latest/download/ontosys-linux-amd64
#   https://github.com/styk-tv/ontosys/releases/latest/download/ontosys-linux-arm64
```

While the repository is private those asset URLs need credentials; the installer
uses the `gh` CLI or `$GH_TOKEN` automatically. Verify against `SHA256SUMS`,
published with every release.

## Building from Source

```bash
# Clone the repository
git clone https://github.com/styk-tv/ontosys
cd ontosys

# Build release binary
cargo build --release

# Install globally (optional)
cargo install --path .
```

## Configuration

Edit `.ontosys/config.json` to customize:

```json
{
  "project": "myproject",
  "languages": ["rust", "python", "typescript", "javascript", "c"],
  "exclude_patterns": [
    "**/node_modules/**",
    "**/target/**",
    "**/__pycache__/**"
  ],
  "include_private": true,
  "extract_docs": true,
  "extract_calls": true
}
```

`project` is optional: it fixes the name used in instance IRIs (default: the
`remote.origin.url` basename, else the directory name). `c` is added by
`ontosys init` when the repository contains `.c` files. `exclude_patterns` is
not applied yet; use `.gitignore` (or `.git/info/exclude`) to leave files out.

## References

### Ontologies

- [BFO 2020](https://basic-formal-ontology.org/) - Basic Formal Ontology
- [CodeOntology](http://codeontology.org/) - OOP source code ontology
- [PROV-O](https://www.w3.org/TR/prov-o/) - Provenance tracking
- [DOAP](http://usefulinc.com/ns/doap#) - Project descriptions

### Rust Patterns

- [Type-State Pattern](https://cliffle.com/blog/rust-typestate/)
- [Session Types](https://arxiv.org/abs/1905.01738)
- [Capability-Based Security](https://en.wikipedia.org/wiki/Capability-based_security)

## License

MIT — see [LICENSE](LICENSE).
