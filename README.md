# OntoSys

A multi-language code knowledge graph system that extracts semantic information from Git repositories, maps to formal ontologies (BFO 2020 / CodeOntology), and provides interactive visualization.

## Features

- **Multi-Language Support**: Rust, Python, TypeScript, JavaScript, plus a Markdown docs layer
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
    serve     Start the visualization web server
    query     Execute a query against the graph
    export    Export the graph to a file
    watch     Watch for file changes and rebuild automatically
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

# Query for functions
ontosys query "function"

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
│   ├── graph.json    # JSON-LD for visualization
│   ├── docs.json     # Comments + markdown docs metadata
│   └── build-meta.json
├── cache/            # Parsed AST cache
├── exports/          # Exported files
└── viz/              # Visualization assets
```

## Supported Languages

| Language | Extensions | Extraction |
|----------|-----------|------------|
| Rust | `.rs` | Declarations (line-based) |
| Python | `.py` | Declarations (line-based) |
| TypeScript | `.ts`, `.tsx` | Declarations (line-based) |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | Declarations (line-based) |
| Markdown | `.md` | Docs layer: ATX-heading outline + verbatim content into `docs.json` |

Parsing is line-based heuristic extraction of declarations (functions,
structs/classes, traits, enums, imports, …) with source locations — not a
full syntax tree. Tree-sitter was evaluated and deliberately dropped (see the
note in `Cargo.toml`); re-adding a real parser happens in the same commit
that first uses it.

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

## Visualization

The built-in visualization provides:

- **Interactive Graph**: D3.js force-directed layout
- **Filtering**: By node type, search query
- **Details Panel**: Properties of selected nodes
- **Zoom & Pan**: Mouse/trackpad navigation
- **Responsive**: Full-screen adaptive layout

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
```

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
  "languages": ["rust", "python", "typescript", "javascript"],
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

MIT
