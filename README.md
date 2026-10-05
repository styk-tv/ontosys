# OntoSys

A multi-language code knowledge graph system that extracts semantic information from Git repositories, maps to formal ontologies (BFO 2020 / CodeOntology), and provides interactive visualization.

## Features

- **Multi-Language Support**: C (tree-sitter syntax trees), Rust, Python, TypeScript, JavaScript, plus a Markdown docs layer
- **Project Grounding**: attaches meaning to code from the project's own declarative sources (PostgreSQL built in)
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
├── data/
│   ├── graph.ttl     # RDF in Turtle format
│   ├── graph.nt      # RDF in N-Triples format
│   ├── graph.json    # JSON-LD for visualization (an overview above 250k triples)
│   ├── docs.json     # Comments + markdown docs metadata
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
repository, start and end line. Python lines inside strings, brackets or
backslash continuations are never read as declarations; module-level
assignments follow the rule of Python's own `ast` (each target of `A = B = …`
and `NAME: T = …`; not tuple, attribute or augmented targets, nor anything
nested in a block).

The project is what git considers the repository: files ignored by
`.gitignore`, `.git/info/exclude` or the global excludes (a virtualenv, build
output) are not read; hidden but tracked directories such as `.github/` are.

C extraction details:

- **Body hash** — FNV-1a over the non-comment tokens of a function body: it
  changes when the code changes, not when comments, whitespace or line
  positions do.
- **Call resolution** — a call resolves to a static function in the same file,
  then a static inline in a header, then the single definition in the same
  build component, then shared code; unresolved names become
  `cx:ExternalSymbol`.
- **Parse recovery** — a project profile may supply a length-preserving
  prepass (annotation macros blanked, iteration macros such as
  `foreach(lc, list)` rewritten to `while (lc, list)`). Files that still have
  errors are re-parsed with conditional-compilation lines and bare
  `MACRO(args)` lines blanked, and the cleaner parse is kept. Each file
  records `cx:parseErrors`.

## Project Grounding

Grounding attaches meaning to the C graph from files a project already
maintains as declarative data — nothing is inferred from names or generated by
a model. It is enabled automatically when the project is recognised.

**PostgreSQL** (detected by `src/include/catalog/pg_proc.dat`):

| Source | Grounds |
|---|---|
| directory layout, `README`, `contrib/*/*.control`, file header comments | code areas (backend subsystems, client programs, extensions, …) and file purposes |
| `src/include/catalog/pg_*.h` (`CATALOG(...)`) | system catalogs, columns (with their comments), lookups, indexes, syscaches |
| `src/include/catalog/*.dat` | built-in functions, types, operators, casts, aggregates, access methods, … |
| `pg_proc.dat` `prosrc` | SQL function → implementing C function |
| `src/backend/utils/misc/guc_parameters.dat` | configuration parameters → C variable, check/assign/show hooks |
| `src/backend/utils/errcodes.txt` | SQLSTATEs |
| `src/backend/utils/activity/wait_event_names.txt` | wait events |
| `src/include/storage/lwlocklist.h` | LWLocks and tranches |
| `src/include/parser/kwlist.h` | SQL keywords and reservation categories |
| structs whose first field is `NodeTag` | node types and their inheritance |
| `extern PGDLLIMPORT *_hook_type` | extension hooks |

Each C function then gets a profile in those terms: `pg:implementsSQLFunction`,
`pg:mayRaise` (SQLSTATE), `pg:reportsAtLevel`, `pg:readsSetting`,
`pg:createsNode` / `pg:inspectsNode` / `pg:operatesOn`, `pg:waitsOn`,
`pg:usesLock`, `pg:touchesHook`, plus derived roles such as
`pg:TypeSupportFunction`, `pg:OperatorImplementation`,
`pg:AggregateSupportFunction`, `pg:IndexSupportFunction`,
`pg:SettingHookFunction` and `pg:ExtensionEntryPoint`. Every class in the
vocabulary carries `pg:groundedIn`, naming the file its instances come from.

PostgreSQL knowledge currently lives in `src/grounding/postgres.rs`; the
intended direction is a declarative per-project profile so other projects can
be grounded without code changes.

## Semantic Diff

```bash
ontosys diff path/to/old-checkout path/to/new-checkout --out report.md --json report.json
```

Both arguments may be repositories (their `.ontosys/data/graph.nt` is used) or
`.nt` files. Because instance IRIs are built from the project name and
repository paths / natural keys only, the same entity has the same IRI in both
builds, and the comparison is a streaming merge of two sorted files.
Differences are grouped by entity and reported by kind — SQL surface,
settings, error conditions, catalogs, node types, then C functions per code
area — with each changed function classified as a *contract*, *SQL role*,
*behaviour*, *dependencies*, *implementation* or *docs* change. Line-number
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

- **Call resolution across link units.** The last-resort rule — "the single
  non-static definition anywhere" — ignores which binary the caller links
  into, and ignores that a callee may be a function-like macro visible
  through the caller's includes. On PostgreSQL 19 this mislinks ≈626 calls
  (extension and PL code calling `pfree`/`palloc`/`pstrdup` resolve to the
  frontend `src/common/fe_memutils.c` instead of the backend allocator) and
  398 calls to the `pg_fatal` macro (resolved to `pg_upgrade`'s function of
  the same name). Planned fix: no cross-link-unit fallback (unresolved →
  `cx:ExternalSymbol`), macro calls on their own predicate, link units from
  build introspection when available, and `cx:callsExternal` for external
  symbols so `cx:calls` is function→function only.
- **Macro-generated and `#ifdef`-split definitions** are not seen (≈0.08% of
  function definitions on PostgreSQL 19, measured against an independent
  count).
- **Grounding is code, not configuration.** PostgreSQL knowledge lives in
  `src/grounding/postgres.rs`; the intended direction is a declarative
  profile so other projects can be grounded without code changes. Because
  output is deterministic, a profile-driven build must reproduce today's
  `graph.nt` byte for byte on the same sources.
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

- **Explore**: search every label in both builds; the selected entity shows its
  grounding (what its class means and which repository file defines it), its
  change between builds, its properties, and its links grouped by predicate,
  outgoing and incoming. Each link is a step: it brings the target onto the
  canvas, connects it and focuses it, with back/forward history and a
  shareable URL.
- **Canvas**: the whole graph when small; otherwise an overview of the
  grounded structure (for PostgreSQL: code areas, catalogs and columns, node
  types, SQLSTATEs) that grows as you explore. A graph with no grounding opens
  on a **class map** instead: one node per class, sized by its number of
  entities, linked by the relations between classes with their counts;
  clicking a class draws its entities a page at a time. Delta colouring and a
  "changed only" view.
- **Browse**: every class with its count; a class lists its entities by name,
  filterable, a page at a time — the way through any graph, grounded or not.
- **Stats**: SPARQL aggregates over the full graph, computed at startup —
  entities by class, triples by predicate, most linked entities, functions per code area, most raised error conditions,
  most read settings, most created node types, most called and most complex
  functions, … — every row clickable.
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
@prefix pg: <https://ontosys.io/ns/pg#> .         # PostgreSQL grounding
@prefix pgcat: <https://ontosys.io/ns/pgcat#> .   # PostgreSQL catalog columns
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
  "project": "postgres",
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
