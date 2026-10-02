#!/usr/bin/env python3
"""Generate the explorer test fixture: two small builds of a PostgreSQL-like
project in ontosys's canonical N-Triples form (sorted, de-duplicated).

  current.nt   "fix2": f01..f20, catalog pg_demo with columns a, b
  baseline.nt  "fix1": f01..f19 + f21, columns a, b, c; f01 calls f03 instead of f02

Exercises: a SQLSTATE raised by 20 functions (grouped: > 12), a catalog with
2 columns (not grouped), one added (f20), one removed (f21, column c) and
one changed (f01) entity.  Run from anywhere: python3 tests/fixtures/gen.py
"""
import os

CX, PG = "https://ontosys.io/ns/c#", "https://ontosys.io/ns/pg#"
RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
LABEL, COMMENT = "http://www.w3.org/2000/01/rdf-schema#label", "http://www.w3.org/2000/01/rdf-schema#comment"
OWL = "http://www.w3.org/2002/07/owl#"
XSD_INT = "http://www.w3.org/2001/XMLSchema#integer"
ID = "https://ontosys.io/id/fixture/"


def iri(x):
    return f"<{x}>"


def lit(v):
    return '"' + v.replace("\\", "\\\\").replace('"', '\\"') + '"'


def build(version):
    t = set()
    add = lambda s, p, o: t.add(f"{iri(s)} {iri(p)} {o} .")
    # vocabulary (TBox), as ontosys emits it
    for cls, label, comment, grounded in [
        (CX + "Function", "C function definition", "A function with a body, identified by file and name.", None),
        (PG + "SQLState", "SQLSTATE", "An error condition code reported to clients.", "src/backend/utils/errcodes.txt"),
        (PG + "SystemCatalog", "system catalog", "A table the server uses to describe itself.", "src/include/catalog/pg_*.h CATALOG()"),
        (PG + "CatalogColumn", "catalog column", "A column of a system catalog, in attribute order.", "src/include/catalog/pg_*.h"),
        (PG + "CodeArea", "code area", "A directory of the source tree.", "directory layout + README files"),
        (PG + "Release", "release", None, None),
    ]:
        add(cls, RDF_TYPE, iri(OWL + "Class"))
        add(cls, LABEL, lit(label))
        if comment:
            add(cls, COMMENT, lit(comment))
        if grounded:
            add(cls, PG + "groundedIn", lit(grounded))
    for prop, label in [(PG + "mayRaise", "may raise"), (PG + "hasColumn", "has column"), (CX + "calls", "calls")]:
        add(prop, RDF_TYPE, iri(OWL + "ObjectProperty"))
        add(prop, LABEL, lit(label))

    rel = ID + "pg/release/current"
    add(rel, RDF_TYPE, iri(PG + "Release"))
    add(rel, LABEL, lit("Fixture " + version))
    add(rel, PG + "version", lit(version))

    area = ID + "pg/area/src"
    add(area, RDF_TYPE, iri(PG + "CodeArea"))
    add(area, LABEL, lit("src"))

    state = ID + "pg/sqlstate/22012"
    add(state, RDF_TYPE, iri(PG + "SQLState"))
    add(state, LABEL, lit("division_by_zero"))
    add(state, PG + "sqlstateCode", lit("22012"))

    cat = ID + "pg/catalog/pg_demo"
    add(cat, RDF_TYPE, iri(PG + "SystemCatalog"))
    add(cat, LABEL, lit("pg_demo"))
    cols = ["a", "b"] + (["c"] if version == "fix1" else [])
    for c in cols:
        col = f"{cat}/column/{c}"
        add(cat, PG + "hasColumn", iri(col))
        add(col, RDF_TYPE, iri(PG + "CatalogColumn"))
        add(col, LABEL, lit(f"pg_demo.{c}"))

    fns = [f"f{i:02d}" for i in range(1, 20)] + (["f20"] if version == "fix2" else ["f21"])
    for name in fns:
        f = ID + "fn/src/demo.c/" + name
        add(f, RDF_TYPE, iri(CX + "Function"))
        add(f, LABEL, lit(name))
        add(f, PG + "mayRaise", iri(state))
        add(f, CX + "complexity", f'"{len(name)}"^^{iri(XSD_INT)}')
    f01 = ID + "fn/src/demo.c/f01"
    callee = "f02" if version == "fix2" else "f03"
    add(f01, CX + "calls", iri(ID + "fn/src/demo.c/" + callee))
    add(f01, CX + "bodyHash", lit("aaaa" if version == "fix2" else "bbbb"))
    return "".join(line + "\n" for line in sorted(t))


here = os.path.dirname(os.path.abspath(__file__))
for name, version in [("current.nt", "fix2"), ("baseline.nt", "fix1")]:
    with open(os.path.join(here, name), "w") as fh:
        fh.write(build(version))
