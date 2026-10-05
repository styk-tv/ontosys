#!/usr/bin/env python3
"""Generate the generic explorer fixture: one build of a non-PostgreSQL project
in the shape the Rust / Python / TypeScript extractors emit today (classes
outside ontosys's namespaces, unlabelled imports, everything hanging off the
project node), sorted and de-duplicated like ontosys's canonical output.

  generic.nt  project "demo": 30 functions (paged: > one 24-member page),
              3 structs, 4 unlabelled imports, parameters per function,
              ontosys's vocabulary declaring PostgreSQL classes with no
              instances, plus 2 lines that are not valid N-Triples (copied from real
              extractor defects) which `serve` must skip, not refuse.

Run from anywhere: python3 tests/fixtures/gen_generic.py
"""
import os

CO, CODE = "http://codeontology.org/ontology/", "http://example.org/code/"
DATA = "http://example.org/data/project/demo/"
RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
OWL_CLASS = "http://www.w3.org/2002/07/owl#Class"
LABEL = "http://www.w3.org/2000/01/rdf-schema#label"


def iri(x):
    return f"<{x}>"


def lit(v):
    return '"' + v.replace("\\", "\\\\").replace('"', '\\"') + '"'


def build():
    t = set()
    add = lambda s, p, o: t.add(f"{iri(s)} {iri(p)} {o} .")
    # A repo with a few C files gets ontosys's whole vocabulary, PostgreSQL
    # classes included, without a single PostgreSQL entity.
    add("https://ontosys.io/ns/pg#SystemCatalog", RDF_TYPE, iri(OWL_CLASS))
    proj = DATA.rstrip("/")
    add(proj, RDF_TYPE, iri("http://usefulinc.com/ns/doap#Project"))
    add(proj, LABEL, lit("demo"))
    for i in range(30):
        f = f"{DATA}fn/handler_{i:02d}"
        add(f, RDF_TYPE, iri(CO + "Function"))
        add(f, LABEL, lit(f"handler_{i:02d}"))
        add(proj, CO + "hasMethod", iri(f))
        add(f, CODE + "filePath", lit(f"src/handlers/h{i % 3}.py"))
        for p in ("self", "request"):
            par = f"{f}/param/{p}"
            add(par, RDF_TYPE, iri(CO + "Parameter"))
            add(par, LABEL, lit(p))
            add(f, CO + "hasParameter", iri(par))
    for s in ("Router", "Route", "Config"):
        st = f"{DATA}struct/{s}"
        add(st, RDF_TYPE, iri(CODE + "Struct"))
        add(st, LABEL, lit(s))
        add(proj, CODE + "hasStruct", iri(st))
    for m in ("os.path", "typing.Any", "json", "re"):
        imp = f"{DATA}import/{m}"
        add(imp, RDF_TYPE, iri(CODE + "Import"))      # deliberately unlabelled
        add(proj, CODE + "hasImport", iri(imp))
    lines = sorted(t)
    # Two defects the extractors produce today; not valid N-Triples.
    lines.insert(5, f"{iri(DATA + 'fn/[a, setA]')} {iri(RDF_TYPE)} {iri(CO + 'Function')} .")
    lines.insert(40, f"<{DATA}fn/Lazy/generic/never>> {iri(CODE + 'position')} \"1\" .")
    return lines


if __name__ == "__main__":
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "generic.nt"), "w") as f:
        f.write("\n".join(build()) + "\n")
