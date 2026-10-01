//! # Vocabulary (TBox)
//!
//! Class and property definitions for the C layer (`cx:`) and the PostgreSQL
//! layer (`pg:`), emitted into every graph so the data carries its own schema.
//! Each PostgreSQL class names, via `pg:groundedIn`, the repository file whose
//! contents define its instances — the reference for what the term means.

use crate::ontology::*;

/// (namespace, local name, kind, label, comment, parent class, grounded in)
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
    ("cx", "Macro", 'C', "macro", "A #define in a header.", "", ""),
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
    ("cx", "emitsMessage", 'D', "emits message", "A primary diagnostic text the function can produce.", "", ""),
    ("cx", "emitsDetail", 'D', "emits detail", "", "", ""),
    ("cx", "emitsHint", 'D', "emits hint", "", "", ""),
    ("cx", "emitsContext", 'D', "emits context", "", "", ""),
    ("cx", "parseErrors", 'D', "parse errors", "tree-sitter ERROR/MISSING nodes left after macro erasure; 0 means a clean parse.", "", ""),
    // ---- PostgreSQL: structure -------------------------------------------
    ("pg", "CodeArea", 'C', "code area", "A directory of the source tree that groups code with one purpose.", "", "directory layout + README files"),
    ("pg", "BackendSubsystem", 'C', "backend subsystem", "Part of the server process (postgres).", "CodeArea", "src/backend/*/README"),
    ("pg", "ClientProgram", 'C', "client program", "A standalone executable such as psql or pg_dump.", "CodeArea", "src/bin/*"),
    ("pg", "ClientLibrary", 'C', "client library", "A library linked by client applications (libpq, ecpg).", "CodeArea", "src/interfaces/*"),
    ("pg", "ProceduralLanguage", 'C', "procedural language", "A server-side language handler (PL/pgSQL, PL/Perl, ...).", "CodeArea", "src/pl/*"),
    ("pg", "Extension", 'C', "extension", "A loadable contrib module with a control file.", "CodeArea", "contrib/*/*.control"),
    ("pg", "TestModule", 'C', "test module", "Code that exists to test the server.", "CodeArea", "src/test/*"),
    ("pg", "SharedCode", 'C', "shared code", "Code linked into both server and clients.", "CodeArea", "src/common, src/port, src/fe_utils"),
    ("pg", "PublicHeaders", 'C', "header tree", "Headers shared by all components.", "CodeArea", "src/include"),
    ("pg", "partOf", 'O', "part of", "Code area → enclosing code area.", "", ""),
    ("pg", "inArea", 'O', "in area", "File → the code area (directory) it belongs to.", "", ""),
    ("pg", "fileDescription", 'D', "file description", "The one-line purpose from the file's header comment.", "", "file header comments"),
    ("pg", "groundedIn", 'D', "grounded in", "Repository file(s) whose content defines this term's instances.", "", ""),
    // ---- catalogs --------------------------------------------------------
    ("pg", "SystemCatalog", 'C', "system catalog", "A table the server uses to describe itself.", "", "src/include/catalog/pg_*.h CATALOG()"),
    ("pg", "CatalogColumn", 'C', "catalog column", "A column of a system catalog, in attribute order.", "", "src/include/catalog/pg_*.h"),
    ("pg", "CatalogIndex", 'C', "catalog index", "An index on a system catalog (DECLARE_*INDEX).", "", "src/include/catalog/pg_*.h"),
    ("pg", "CatalogRow", 'C', "bootstrap catalog row", "A row present in every new cluster.", "", "src/include/catalog/*.dat"),
    ("pg", "BuiltinFunction", 'C', "built-in SQL function", "A pg_proc row: a function callable from SQL.", "CatalogRow", "src/include/catalog/pg_proc.dat"),
    ("pg", "DataType", 'C', "built-in data type", "A pg_type row.", "CatalogRow", "src/include/catalog/pg_type.dat"),
    ("pg", "Operator", 'C', "built-in operator", "A pg_operator row.", "CatalogRow", "src/include/catalog/pg_operator.dat"),
    ("pg", "Cast", 'C', "built-in cast", "A pg_cast row.", "CatalogRow", "src/include/catalog/pg_cast.dat"),
    ("pg", "AccessMethod", 'C', "access method", "A pg_am row: a table or index storage strategy.", "CatalogRow", "src/include/catalog/pg_am.dat"),
    ("pg", "Aggregate", 'C', "built-in aggregate", "A pg_aggregate row.", "CatalogRow", "src/include/catalog/pg_aggregate.dat"),
    ("pg", "OperatorClass", 'C', "operator class", "", "CatalogRow", "src/include/catalog/pg_opclass.dat"),
    ("pg", "OperatorFamily", 'C', "operator family", "", "CatalogRow", "src/include/catalog/pg_opfamily.dat"),
    ("pg", "Collation", 'C', "collation", "", "CatalogRow", "src/include/catalog/pg_collation.dat"),
    ("pg", "LanguageEntry", 'C', "language entry", "A pg_language row.", "CatalogRow", "src/include/catalog/pg_language.dat"),
    ("pg", "inCatalog", 'O', "in catalog", "Row → its catalog.", "", ""),
    ("pg", "hasColumn", 'O', "has column", "", "", ""),
    ("pg", "hasIndex", 'O', "has index", "", "", ""),
    ("pg", "lookupCatalog", 'O', "looks up", "Column → catalog its OID values reference (BKI_LOOKUP).", "", ""),
    ("pg", "representedBy", 'O', "represented by", "Domain concept → the C struct that stores it.", "", ""),
    ("pg", "implementedBy", 'O', "implemented by", "SQL function → C function named by prosrc.", "", "pg_proc.dat prosrc"),
    ("pg", "implementsSQLFunction", 'O', "implements SQL function", "Inverse of implementedBy.", "", "pg_proc.dat prosrc"),
    ("pg", "argumentType", 'O', "argument type", "SQL function → a type in its argument list.", "", ""),
    // ---- settings, errors, waits, locks, grammar -------------------------
    ("pg", "ConfigParameter", 'C', "configuration parameter", "A GUC setting (postgresql.conf / SET).", "", "src/backend/utils/misc/guc_parameters.dat"),
    ("pg", "boundToVariable", 'O', "bound to variable", "Setting → the C global that holds its value.", "", ""),
    ("pg", "checkHook", 'O', "check hook", "", "", ""),
    ("pg", "assignHook", 'O', "assign hook", "", "", ""),
    ("pg", "showHook", 'O', "show hook", "", "", ""),
    ("pg", "readsSetting", 'O', "reads setting", "Function body references the setting's C variable.", "", ""),
    ("pg", "SQLState", 'C', "SQLSTATE", "An error condition code reported to clients.", "", "src/backend/utils/errcodes.txt"),
    ("pg", "SQLStateClass", 'C', "SQLSTATE class", "The two-character class of SQLSTATE codes.", "", "src/backend/utils/errcodes.txt"),
    ("pg", "inClass", 'O', "in class", "", "", ""),
    ("pg", "mayRaise", 'O', "may raise", "Function body references the condition's ERRCODE_ macro.", "", ""),
    ("pg", "reportsAtLevel", 'D', "reports at level", "Severity passed to ereport/elog (ERROR, FATAL, PANIC, WARNING, ...).", "", ""),
    ("pg", "WaitEvent", 'C', "wait event", "A named reason a process can be waiting (pg_stat_activity).", "", "src/backend/utils/activity/wait_event_names.txt"),
    ("pg", "waitsOn", 'O', "waits on", "Function body references the WAIT_EVENT_ constant.", "", ""),
    ("pg", "LWLock", 'C', "lightweight lock", "An individually named LWLock in shared memory.", "", "src/include/storage/lwlocklist.h"),
    ("pg", "LWLockTranche", 'C', "LWLock tranche", "A group of LWLocks sharing a name.", "", "src/include/storage/lwlocklist.h"),
    ("pg", "usesLock", 'O', "uses lock", "Function body references the named LWLock.", "", ""),
    ("pg", "usesLockTranche", 'O', "uses lock tranche", "", "", ""),
    ("pg", "Keyword", 'C', "SQL keyword", "A word of the SQL grammar with its reservation category.", "", "src/include/parser/kwlist.h"),
    // ---- nodes and hooks -------------------------------------------------
    ("pg", "NodeType", 'C', "node type", "A tagged tree node: parse trees, plans, paths, executor state.", "", "src/include/nodes/*.h (structs whose first field is NodeTag)"),
    ("pg", "specializes", 'O', "specializes", "Node type → the node type embedded as its first field (inheritance).", "", ""),
    ("pg", "createsNode", 'O', "creates node", "Function body calls makeNode(X).", "", ""),
    ("pg", "inspectsNode", 'O', "inspects node", "Function body tests or casts to X (IsA, castNode, T_X).", "", ""),
    ("pg", "operatesOn", 'O', "operates on", "Function takes or returns a pointer to node type X.", "", ""),
    ("pg", "Hook", 'C', "extension hook", "A global function pointer extensions set to change server behaviour.", "", "src/include/**/*.h extern PGDLLIMPORT *_hook_type"),
    ("pg", "touchesHook", 'O', "touches hook", "Function body reads (invokes) or writes (installs) the hook.", "", ""),
    // ---- derived function roles -----------------------------------------
    ("pg", "SQLCallableFunction", 'C', "SQL-callable C function", "Named by prosrc of a built-in function.", "", "pg_proc.dat"),
    ("pg", "FmgrV1Function", 'C', "fmgr V1 function", "Takes PG_FUNCTION_ARGS: the calling convention for SQL-callable C.", "", ""),
    ("pg", "TypeSupportFunction", 'C', "type support function", "Implements input/output/send/receive/typmod/analyze for a type.", "SQLCallableFunction", "pg_type.dat"),
    ("pg", "OperatorImplementation", 'C', "operator implementation", "Implements an operator or its selectivity estimator.", "SQLCallableFunction", "pg_operator.dat"),
    ("pg", "AggregateSupportFunction", 'C', "aggregate support function", "Transition/final/combine/serialize function of an aggregate.", "SQLCallableFunction", "pg_aggregate.dat"),
    ("pg", "IndexSupportFunction", 'C', "index support function", "Access-method handler or support procedure.", "SQLCallableFunction", "pg_am.dat, pg_amproc.dat"),
    ("pg", "SettingHookFunction", 'C', "setting hook function", "check/assign/show hook of a configuration parameter.", "", "guc_parameters.dat"),
    ("pg", "ExtensionEntryPoint", 'C', "extension entry point", "_PG_init: runs when a module is loaded.", "", ""),
];

pub fn emit(out: &mut TripleSet) {
    for (ns, local, kind, label, comment, parent, grounded) in TERMS {
        let ns_ref = if *ns == "cx" { &CX } else { &PG };
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
        if !grounded.is_empty() {
            out.add(Triple::new(iri.clone(), PG.iri("groundedIn"), Literal::string(*grounded)));
        }
    }
}
