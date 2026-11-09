//! # Vocabulary (TBox)
//!
//! Class and property definitions for the C layer (`cx:`), emitted into every
//! graph that has C so the data carries its own schema.

use crate::ontology::*;

/// (namespace, local name, kind, label, comment, parent class, reserved — always empty)
/// kind: 'C' class · 'O' object property · 'D' datatype property
type Term4 = (&'static str, &'static str, char, &'static str, &'static str, &'static str, &'static str);

pub const TERMS: &[Term4] = &[
    // ---- C language layer -------------------------------------------------
    ("cx", "SourceFile", 'C', "C source file", "A .c translation unit.", "", ""),
    ("cx", "HeaderFile", 'C', "C header file", "A .h file; its prototypes and types form an interface.", "", ""),
    ("cx", "SystemHeader", 'C', "system header", "An #include target outside the repository.", "", ""),
    ("cx", "Function", 'C', "C function definition", "A function with a body, identified by file and name.", "", ""),
    ("cx", "ExternalSymbol", 'C', "external symbol", "A called name with no unique definition in the repository (libc, or ambiguous).", "", ""),
    ("cx", "Prototype", 'C', "function prototype", "A function declaration in a header: the published interface.", "", ""),
    ("cx", "Struct", 'C', "struct", "A struct type with named fields.", "", ""),
    ("cx", "Union", 'C', "union", "A union type.", "", ""),
    ("cx", "Field", 'C', "field", "A member of a struct or union, in declaration order.", "", ""),
    ("cx", "Enum", 'C', "enum", "An enumeration type.", "", ""),
    ("cx", "EnumMember", 'C', "enum member", "An enumerator, in declaration order.", "", ""),
    ("cx", "Typedef", 'C', "typedef", "A type alias (including function-pointer types).", "", ""),
    ("cx", "Macro", 'C', "macro", "A #define, in a header or a source file.", "", ""),
    ("cx", "GlobalVariable", 'C', "global variable", "A file-scope variable definition.", "", ""),
    ("cx", "definedIn", 'O', "defined in", "The file containing the definition.", "", ""),
    ("cx", "declaredIn", 'O', "declared in", "The header containing the prototype.", "", ""),
    ("cx", "declares", 'O', "declares", "Prototype → the definition(s) it declares.", "", ""),
    ("cx", "includes", 'O', "includes", "File → #included file.", "", ""),
    ("cx", "calls", 'O', "calls", "Function → function or macro it calls directly.", "", ""),
    ("cx", "usesType", 'O', "uses type", "Function → struct used in its signature or body.", "", ""),
    ("cx", "hasField", 'O', "has field", "", "", ""),
    ("cx", "fieldType", 'O', "field type", "Field → the struct its type names.", "", ""),
    ("cx", "hasMember", 'O', "has member", "", "", ""),
    ("cx", "signature", 'D', "signature", "Normalized return type, name and parameter types.", "", ""),
    ("cx", "parameters", 'D', "parameters", "Normalized parameter list with names.", "", ""),
    ("cx", "bodyHash", 'D', "body hash", "FNV-1a over non-comment tokens: changes iff the code (not layout or comments) changes.", "", ""),
    ("cx", "complexity", 'D', "cyclomatic complexity", "1 + branch points.", "", ""),
    ("cx", "parseErrors", 'D', "parse errors", "tree-sitter ERROR/MISSING nodes left after macro erasure; 0 means a clean parse.", "", ""),
    // ---- catalogs --------------------------------------------------------
    // ---- settings, errors, waits, locks, grammar -------------------------
    // ---- nodes and hooks -------------------------------------------------
    // ---- derived function roles -----------------------------------------
];

pub fn emit(out: &mut TripleSet) {
    for (ns, local, kind, label, comment, parent, _grounded) in TERMS {
        debug_assert_eq!(*ns, "cx");
        let ns_ref = &CX;
        let iri = ns_ref.iri(local);
        let ty = match kind {
            'C' => owl::class(),
            'O' => owl::object_property(),
            _ => owl::datatype_property(),
        };
        out.add(Triple::new(iri.clone(), rdf::type_(), ty));
        out.add(Triple::label(iri.clone(), label));
        if !comment.is_empty() {
            out.add(Triple::new(iri.clone(), rdfs::comment(), Literal::string(*comment)));
        }
        if !parent.is_empty() {
            out.add(Triple::new(iri.clone(), rdfs::subclass_of(), ns_ref.iri(parent)));
        }
    }
}
