//! # C extraction (tree-sitter)
//!
//! Parses C translation units with tree-sitter-c and extracts a language-level
//! model: functions (signature, header comment, calls, identifiers referenced,
//! diagnostic messages, a token-level body hash), header prototypes, structs and
//! their fields, enums, typedefs, macros, globals, and includes.
//!
//! The extractor knows nothing about any particular project. Project knowledge
//! (what an identifier *means*) lives in `crate::grounding`, which consumes the
//! `CFile` model produced here.
//!
//! Everything is deterministic: collections are ordered sets, and the body hash
//! is FNV-1a over the non-comment leaf tokens, so it ignores whitespace, comments
//! and line moves but changes with any token of real code.

pub mod rdf;

use std::collections::BTreeSet;
use tree_sitter::{Node, Parser};

/// A length-preserving source rewrite applied before parsing (macro erasure).
pub type Prepass = fn(&str) -> String;

#[derive(Debug, Clone, Default)]
pub struct CFile {
    /// Repository-relative path with `/` separators
    pub path: String,
    pub is_header: bool,
    /// Raw text of the comment that opens the file, if any
    pub header_comment: Option<String>,
    pub includes: BTreeSet<String>,
    pub functions: Vec<CFunction>,
    pub prototypes: Vec<CPrototype>,
    pub structs: Vec<CStruct>,
    pub enums: Vec<CEnum>,
    pub typedefs: Vec<CTypedef>,
    pub macros: Vec<CMacro>,
    pub globals: Vec<CGlobal>,
    /// Top-level `NAME(arg);` macro invocations, e.g. `PG_FUNCTION_INFO_V1(foo)`
    pub top_level_calls: BTreeSet<(String, String)>,
    /// tree-sitter ERROR / MISSING nodes left after the prepass
    pub parse_errors: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CParam {
    pub name: String,
    pub type_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct CFunction {
    pub name: String,
    pub is_static: bool,
    pub is_inline: bool,
    pub return_type: String,
    pub params: Vec<CParam>,
    pub signature: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Cleaned text of the comment block directly above the definition
    pub comment: Option<String>,
    pub body_hash: String,
    pub statements: usize,
    /// 1 + branch points (if/loops/case/?:/&&/||)
    pub complexity: usize,
    /// Names of directly called functions / function-like macros
    pub calls: BTreeSet<String>,
    /// Every identifier referenced in the body (variables, constants, macros)
    pub identifiers: BTreeSet<String>,
    /// Type names used in the body (declarations, casts, sizeof)
    pub type_refs: BTreeSet<String>,
    /// (callee, argument index, identifier) for arguments that are bare identifiers
    pub call_args: BTreeSet<(String, usize, String)>,
    /// (callee, text) for string literals passed to diagnostic calls
    pub messages: BTreeSet<(String, String)>,
    /// Levels passed as the first argument of `ereport`/`elog`-style calls
    pub error_levels: BTreeSet<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CPrototype {
    pub name: String,
    pub signature: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CField {
    pub name: String,
    pub type_text: String,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CStruct {
    pub name: String,
    pub is_union: bool,
    pub fields: Vec<CField>,
    pub comment: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CEnum {
    pub name: String,
    /// (name, value text, comment)
    pub members: Vec<(String, String, Option<String>)>,
    pub comment: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CTypedef {
    pub name: String,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CMacro {
    pub name: String,
    pub params: Option<String>,
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CGlobal {
    pub name: String,
    pub type_text: String,
    pub is_static: bool,
    pub line: usize,
}

/// Diagnostic calls whose string-literal argument is a human-readable message.
const MESSAGE_CALLS: &[&str] = &[
    "errmsg", "errmsg_internal", "errmsg_plural", "errdetail", "errdetail_internal",
    "errdetail_plural", "errdetail_log", "errhint", "errhint_plural", "errcontext",
    "elog", "pg_fatal", "pg_log_error", "pg_log_warning", "pg_log_info", "pg_log_error_detail",
    "pg_log_error_hint",
];

/// Calls whose first argument is a severity level.
const LEVEL_CALLS: &[&str] = &["ereport", "elog", "ereport_domain"];

pub fn new_parser() -> Parser {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_c::LANGUAGE.into())
        .expect("tree-sitter-c grammar is ABI-compatible with the linked tree-sitter");
    p
}

/// Parse one C file into the language-level model.
pub fn extract(parser: &mut Parser, rel_path: &str, raw: &str, prepass: Option<Prepass>) -> CFile {
    let src_owned;
    let src: &str = match prepass {
        Some(f) => {
            src_owned = f(raw);
            debug_assert_eq!(src_owned.len(), raw.len(), "prepass must preserve length");
            &src_owned
        }
        None => raw,
    };
    let mut file = CFile {
        path: rel_path.to_string(),
        is_header: rel_path.ends_with(".h"),
        ..Default::default()
    };
    let Some(mut tree) = parser.parse(src, None) else {
        file.parse_errors = 1;
        return file;
    };
    file.parse_errors = count_errors(tree.root_node());

    // tree-sitter cannot represent preprocessor conditionals that interleave
    // with statements (an `#ifdef` inside an else-if chain), and its recovery
    // can then swallow the rest of the file. Re-parse with those lines blanked
    // and keep whichever parse is cleaner. Deterministic: same input, same pick.
    let flattened;
    let mut src = src;
    if file.parse_errors > 0 {
        flattened = flatten_for_reparse(src);
        if let Some(t2) = parser.parse(&flattened, None) {
            let e2 = count_errors(t2.root_node());
            if e2 < file.parse_errors {
                file.parse_errors = e2;
                tree = t2;
                src = &flattened;
            }
        }
    }
    let root = tree.root_node();
    let b = src.as_bytes();
    {
        let mut c = root.walk();
        let first = root.named_children(&mut c).next();
        if let Some(first) = first {
            if first.kind() == "comment" {
                file.header_comment = Some(text(first, b).to_string());
            }
        }
    }

    walk_top(root, b, &mut file);
    collect_types(root, b, &mut file);

    file.functions.sort_by(|a, b| a.name.cmp(&b.name).then(a.start_line.cmp(&b.start_line)));
    file.prototypes.sort_by(|a, b| a.name.cmp(&b.name));
    file.structs.sort_by(|a, b| a.name.cmp(&b.name));
    file.enums.sort_by(|a, b| a.name.cmp(&b.name));
    file.typedefs.sort_by(|a, b| a.name.cmp(&b.name));
    file.macros.sort_by(|a, b| a.name.cmp(&b.name));
    file.globals.sort_by(|a, b| a.name.cmp(&b.name));
    file
}

// ----------------------------------------------------------------------------
// Top level
// ----------------------------------------------------------------------------

fn walk_top(node: Node, b: &[u8], file: &mut CFile) {
    let mut c = node.walk();
    let children: Vec<Node> = node.named_children(&mut c).collect();
    for ch in children {
        match ch.kind() {
            "function_definition" => {
                if let Some(f) = function(ch, b) {
                    file.functions.push(f);
                }
            }
            "declaration" => declaration(ch, b, file),
            "preproc_include" => {
                if let Some(p) = ch.child_by_field_name("path") {
                    let t = text(p, b).trim_matches(|c| c == '"' || c == '<' || c == '>');
                    file.includes.insert(t.to_string());
                }
            }
            "preproc_def" if file.is_header => {
                if let Some(n) = ch.child_by_field_name("name") {
                    file.macros.push(CMacro {
                        name: text(n, b).to_string(),
                        params: None,
                        value: ch.child_by_field_name("value").map(|v| norm_ws(text(v, b))).unwrap_or_default(),
                        line: ch.start_position().row + 1,
                    });
                }
            }
            "preproc_function_def" if file.is_header => {
                if let Some(n) = ch.child_by_field_name("name") {
                    file.macros.push(CMacro {
                        name: text(n, b).to_string(),
                        params: ch.child_by_field_name("parameters").map(|p| norm_ws(text(p, b))),
                        value: ch.child_by_field_name("value").map(|v| norm_ws(text(v, b))).unwrap_or_default(),
                        line: ch.start_position().row + 1,
                    });
                }
            }
            "expression_statement" => top_level_call(ch, b, file),
            "preproc_ifdef" | "preproc_if" | "preproc_else" | "preproc_elif" | "preproc_elifdef"
            | "linkage_specification" | "declaration_list" | "ERROR" => walk_top(ch, b, file),
            _ => {}
        }
    }
}

fn top_level_call(node: Node, b: &[u8], file: &mut CFile) {
    // `PG_FUNCTION_INFO_V1(foo);` parses either as a call statement or as a
    // declaration with a parenthesized declarator; both reach here as text.
    let t = norm_ws(text(node, b));
    let t = t.trim_end_matches(';').trim();
    if let Some(open) = t.find('(') {
        let name = t[..open].trim();
        let arg = t[open + 1..].trim_end_matches(')').trim();
        let is_macro = !name.is_empty()
            && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        let arg_ok = arg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_macro && arg_ok && t.ends_with(')') {
            file.top_level_calls.insert((name.to_string(), arg.to_string()));
        }
    }
}

fn declaration(node: Node, b: &[u8], file: &mut CFile) {
    let storage = storage_classes(node, b);
    let is_static = storage.contains("static");
    let mut c = node.walk();
    let decls: Vec<Node> = node.children_by_field_name("declarator", &mut c).collect();
    if decls.is_empty() {
        // `NAME(arg);` with an implicit type: macro invocation at file scope
        top_level_call(node, b, file);
        return;
    }
    let type_node = node.child_by_field_name("type");
    for d in decls {
        let inner = if d.kind() == "init_declarator" {
            d.child_by_field_name("declarator").unwrap_or(d)
        } else {
            d
        };
        if contains_kind(inner, "function_declarator") {
            if file.is_header {
                if let Some(name) = innermost_name(inner, b) {
                    file.prototypes.push(CPrototype {
                        name,
                        signature: norm_ws(text(node, b).trim_end_matches(';')),
                        line: node.start_position().row + 1,
                    });
                }
            }
        } else if !file.is_header && !storage.contains("extern") {
            if let (Some(name), Some(t)) = (innermost_name(inner, b), type_node) {
                // A type-less `FOO(bar);` is a macro call, not a variable.
                if t.kind() == "type_identifier" && is_upper_macro(text(t, b)) && inner.kind() == "parenthesized_declarator" {
                    file.top_level_calls.insert((text(t, b).to_string(), name));
                    continue;
                }
                let decl_text = strip_span(text(inner, b), inner, &name_node(inner).unwrap_or(inner), b);
                file.globals.push(CGlobal {
                    name,
                    type_text: norm_ws(&format!("{} {}", text(t, b), decl_text)),
                    is_static,
                    line: node.start_position().row + 1,
                });
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn function(node: Node, b: &[u8]) -> Option<CFunction> {
    let mut d = node.child_by_field_name("declarator")?;
    while d.kind() != "function_declarator" {
        d = d.child_by_field_name("declarator")?;
    }
    let name_n = d.child_by_field_name("declarator")?;
    if name_n.kind() != "identifier" {
        return None;
    }
    let name = text(name_n, b).to_string();
    let storage = storage_classes(node, b);

    let ret_start = node.child_by_field_name("type").map(|t| t.start_byte()).unwrap_or(node.start_byte());
    let return_type = norm_ws(std::str::from_utf8(&b[ret_start..name_n.start_byte()]).unwrap_or(""));

    let mut params = Vec::new();
    if let Some(pl) = d.child_by_field_name("parameters") {
        let mut c = pl.walk();
        for p in pl.named_children(&mut c) {
            match p.kind() {
                "parameter_declaration" => {
                    let (pname, ptype) = match p.child_by_field_name("declarator") {
                        Some(pd) => match name_node(pd) {
                            Some(nn) => (text(nn, b).to_string(), norm_ws(&strip_span(text(p, b), p, &nn, b))),
                            None => (String::new(), norm_ws(text(p, b))),
                        },
                        None => (String::new(), norm_ws(text(p, b))),
                    };
                    if pname.is_empty() && ptype == "void" {
                        continue;
                    }
                    params.push(CParam { name: pname, type_text: ptype });
                }
                "variadic_parameter" => params.push(CParam { name: String::new(), type_text: "...".into() }),
                _ => {}
            }
        }
    }
    let signature = format!(
        "{}{}({})",
        if return_type.ends_with('*') { return_type.clone() } else { format!("{} ", return_type) },
        name,
        params.iter().map(|p| p.type_text.clone()).collect::<Vec<_>>().join(", ")
    );

    let mut f = CFunction {
        name,
        is_static: storage.contains("static"),
        is_inline: storage.contains("inline"),
        return_type,
        params,
        signature,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        comment: leading_comment(node, b),
        complexity: 1,
        ..Default::default()
    };
    if let Some(body) = node.child_by_field_name("body") {
        analyze_body(body, b, &mut f);
    }
    Some(f)
}

fn analyze_body(body: Node, b: &[u8], f: &mut CFunction) {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        let kind = n.kind();
        if kind == "comment" {
            continue;
        }
        if n.child_count() == 0 {
            for byte in text(n, b).bytes().chain(std::iter::once(0x1f)) {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        match kind {
            "identifier" => {
                f.identifiers.insert(text(n, b).to_string());
            }
            "type_identifier" => {
                f.type_refs.insert(text(n, b).to_string());
            }
            "if_statement" | "for_statement" | "while_statement" | "do_statement" | "conditional_expression" => {
                f.complexity += 1
            }
            "case_statement" if n.child_by_field_name("value").is_some() => f.complexity += 1,
            "binary_expression" => {
                if let Some(op) = n.child_by_field_name("operator") {
                    if matches!(text(op, b), "&&" | "||") {
                        f.complexity += 1;
                    }
                }
            }
            "call_expression" => call(n, b, f),
            _ => {}
        }
        if kind.ends_with("_statement") && kind != "compound_statement" {
            f.statements += 1;
        }
        let mut c = n.walk();
        let kids: Vec<Node> = n.children(&mut c).collect();
        stack.extend(kids.into_iter().rev());
    }
    f.body_hash = format!("{:016x}", hash);
}

fn call(n: Node, b: &[u8], f: &mut CFunction) {
    let Some(func) = n.child_by_field_name("function") else { return };
    if func.kind() != "identifier" {
        return;
    }
    let callee = text(func, b).to_string();
    let Some(args) = n.child_by_field_name("arguments") else {
        f.calls.insert(callee);
        return;
    };
    let mut c = args.walk();
    let args: Vec<Node> = args.named_children(&mut c).filter(|a| a.kind() != "comment").collect();
    for (i, a) in args.iter().enumerate() {
        if matches!(a.kind(), "identifier" | "type_identifier") {
            f.call_args.insert((callee.clone(), i, text(*a, b).to_string()));
        } else if a.kind() == "type_descriptor" {
            // makeNode(Query) may parse its argument as a type descriptor
            f.call_args.insert((callee.clone(), i, norm_ws(text(*a, b))));
        }
    }
    if LEVEL_CALLS.contains(&callee.as_str()) {
        if let Some(first) = args.first() {
            if first.kind() == "identifier" {
                f.error_levels.insert(text(*first, b).to_string());
            }
        }
    }
    if MESSAGE_CALLS.contains(&callee.as_str()) {
        if let Some(s) = args.iter().find(|a| matches!(a.kind(), "string_literal" | "concatenated_string")) {
            let msg = string_value(*s, b);
            if !msg.is_empty() {
                f.messages.insert((callee.clone(), msg));
            }
        }
    }
    f.calls.insert(callee);
}

fn string_value(n: Node, b: &[u8]) -> String {
    match n.kind() {
        "string_literal" => {
            let t = text(n, b);
            t.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(t).to_string()
        }
        "concatenated_string" => {
            let mut c = n.walk();
            n.named_children(&mut c)
                .map(|p| if p.kind() == "string_literal" { string_value(p, b) } else { format!("%{{{}}}", text(p, b)) })
                .collect::<Vec<_>>()
                .join("")
        }
        _ => String::new(),
    }
}

// ----------------------------------------------------------------------------
// Types (structs, unions, enums, typedefs) — anywhere outside function bodies
// ----------------------------------------------------------------------------

fn collect_types(root: Node, b: &[u8], file: &mut CFile) {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "compound_statement" => continue,
            "struct_specifier" | "union_specifier" => {
                if let Some(body) = n.child_by_field_name("body") {
                    if let Some(name) = type_name(n, b) {
                        let mut fields = Vec::new();
                        collect_fields(body, b, &mut fields);
                        file.structs.push(CStruct {
                            name,
                            is_union: n.kind() == "union_specifier",
                            fields,
                            comment: leading_comment(decl_owner(n), b),
                            line: n.start_position().row + 1,
                        });
                    }
                }
            }
            "enum_specifier" => {
                if let Some(body) = n.child_by_field_name("body") {
                    if let Some(name) = type_name(n, b) {
                        let mut members = Vec::new();
                        let mut c = body.walk();
                        for e in body.named_children(&mut c) {
                            if e.kind() == "enumerator" {
                                if let Some(nn) = e.child_by_field_name("name") {
                                    members.push((
                                        text(nn, b).to_string(),
                                        e.child_by_field_name("value").map(|v| norm_ws(text(v, b))).unwrap_or_default(),
                                        leading_comment(e, b).or_else(|| trailing_comment(e, b)),
                                    ));
                                }
                            }
                        }
                        file.enums.push(CEnum {
                            name,
                            members,
                            comment: leading_comment(decl_owner(n), b),
                            line: n.start_position().row + 1,
                        });
                    }
                }
            }
            "type_definition" => {
                let ty = n.child_by_field_name("type");
                let has_body = ty.map_or(false, |t| t.child_by_field_name("body").is_some());
                if !has_body {
                    let mut c = n.walk();
                    for d in n.children_by_field_name("declarator", &mut c) {
                        if let Some(name) = innermost_name(d, b) {
                            file.typedefs.push(CTypedef {
                                name,
                                text: norm_ws(text(n, b).trim_end_matches(';')),
                                line: n.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        let mut c = n.walk();
        let kids: Vec<Node> = n.named_children(&mut c).collect();
        stack.extend(kids.into_iter().rev());
    }
}

fn collect_fields(list: Node, b: &[u8], out: &mut Vec<CField>) {
    let mut c = list.walk();
    let kids: Vec<Node> = list.named_children(&mut c).collect();
    for fd in kids {
        match fd.kind() {
            "field_declaration" => {
                let ty = fd.child_by_field_name("type").map(|t| text(t, b)).unwrap_or("");
                let comment = leading_comment(fd, b).or_else(|| trailing_comment(fd, b));
                let mut c2 = fd.walk();
                let decls: Vec<Node> = fd.children_by_field_name("declarator", &mut c2).collect();
                for d in decls {
                    if let Some(nn) = name_node(d) {
                        let rest = strip_span(text(d, b), d, &nn, b);
                        out.push(CField {
                            name: text(nn, b).to_string(),
                            type_text: norm_ws(&format!("{} {}", ty, rest)),
                            comment: comment.clone(),
                        });
                    }
                }
            }
            "preproc_ifdef" | "preproc_if" | "preproc_else" | "preproc_elif" | "preproc_elifdef" => {
                collect_fields(fd, b, out)
            }
            _ => {}
        }
    }
}

/// Name of a struct/union/enum: the typedef name when it is the type of a
/// `typedef`, else its tag. Anonymous inner types return None.
fn type_name(n: Node, b: &[u8]) -> Option<String> {
    if let Some(p) = n.parent() {
        if p.kind() == "type_definition" {
            let mut c = p.walk();
            let d = p.children_by_field_name("declarator", &mut c).next();
            if let Some(d) = d {
                if let Some(name) = innermost_name(d, b) {
                    return Some(name);
                }
            }
        }
    }
    n.child_by_field_name("name").map(|nn| text(nn, b).to_string())
}

fn decl_owner(n: Node) -> Node {
    match n.parent() {
        Some(p) if matches!(p.kind(), "type_definition" | "declaration") => p,
        _ => n,
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn text<'a>(n: Node, b: &'a [u8]) -> &'a str {
    n.utf8_text(b).unwrap_or("")
}

pub fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_upper_macro(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn count_errors(root: Node) -> usize {
    let mut n_err = 0;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.is_error() || n.is_missing() {
            n_err += 1;
        }
        if n.has_error() {
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }
    n_err
}

fn storage_classes(n: Node, b: &[u8]) -> BTreeSet<String> {
    let mut c = n.walk();
    n.children(&mut c)
        .filter(|ch| ch.kind() == "storage_class_specifier")
        .map(|ch| text(ch, b).to_string())
        .collect()
}

fn contains_kind(n: Node, kind: &str) -> bool {
    let mut cur = Some(n);
    while let Some(x) = cur {
        if x.kind() == kind {
            return true;
        }
        cur = x.child_by_field_name("declarator");
    }
    false
}

/// Follow the `declarator` field chain down to the declared name node.
fn name_node(n: Node) -> Option<Node> {
    let mut cur = n;
    loop {
        match cur.kind() {
            "identifier" | "field_identifier" | "type_identifier" => return Some(cur),
            _ => cur = cur.child_by_field_name("declarator")?,
        }
    }
}

fn innermost_name(n: Node, b: &[u8]) -> Option<String> {
    name_node(n).map(|nn| text(nn, b).to_string())
}

/// Text of `outer` with the span of `inner` removed (type text without the name).
fn strip_span(outer_text: &str, outer: Node, inner: &Node, _b: &[u8]) -> String {
    let s = inner.start_byte().saturating_sub(outer.start_byte());
    let e = inner.end_byte().saturating_sub(outer.start_byte());
    if e <= outer_text.len() && s <= e {
        format!("{}{}", &outer_text[..s], &outer_text[e..])
    } else {
        outer_text.to_string()
    }
}

/// The comment block that ends on the line directly above `n` (or one blank line above).
fn leading_comment(n: Node, b: &[u8]) -> Option<String> {
    let prev = n.prev_sibling()?;
    if prev.kind() != "comment" {
        return None;
    }
    let gap = n.start_position().row.saturating_sub(prev.end_position().row);
    if gap > 2 {
        return None;
    }
    // A comment sharing a line with the previous item is that item's trailing comment.
    if let Some(pp) = prev.prev_sibling() {
        if pp.kind() != "comment" && pp.end_position().row == prev.start_position().row {
            return None;
        }
    }
    clean_comment(text(prev, b))
}

fn trailing_comment(n: Node, b: &[u8]) -> Option<String> {
    let next = n.next_sibling()?;
    if next.kind() == "comment" && next.start_position().row == n.end_position().row {
        return clean_comment(text(next, b));
    }
    None
}

/// Strip comment syntax and decoration, collapse whitespace.
pub fn clean_comment(raw: &str) -> Option<String> {
    let body = raw
        .trim()
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim_start_matches("//");
    let mut words = Vec::new();
    for line in body.lines() {
        let l = line.trim().trim_start_matches('*').trim();
        if l.chars().all(|c| c == '-' || c == '=' || c == '*') {
            continue;
        }
        words.push(l);
    }
    let s = norm_ws(&words.join(" "));
    if s.is_empty() {
        None
    } else {
        Some(s.chars().take(4000).collect())
    }
}

// ----------------------------------------------------------------------------
// Macro erasure
// ----------------------------------------------------------------------------

/// Blank conditional-compilation lines and bare column-0 `MACRO(args)` lines.
///
/// Conditional directives are removed so both branches are visible as one
/// stream of code; a macro invocation used as a statement with no `;` (e.g. a
/// function-generating macro) is removed so it does not glue onto the next
/// definition. `#define`/`#include` are kept. Length and newlines are preserved.
pub fn flatten_for_reparse(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut continuing = false;
    for line in src.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let t = body.trim_start();
        let directive = t
            .strip_prefix('#')
            .map(|d| d.trim_start())
            .map(|d| {
                ["ifdef", "ifndef", "if", "elifdef", "elifndef", "elif", "else", "endif"]
                    .iter()
                    .any(|k| d.starts_with(k) && !d[k.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            })
            .unwrap_or(false);
        let bare_macro = !continuing && is_bare_macro_line(body);
        if directive || continuing || bare_macro {
            let blanked: String = line.chars().map(|c| if c == '\n' || c == '\r' { c } else { ' ' }).collect();
            out.push_str(&blanked);
            continuing = (directive || continuing) && body.ends_with('\\');
        } else {
            out.push_str(line);
        }
    }
    out
}

/// `NAME(args)` alone on a line at column 0, NAME all caps, no trailing `;`.
fn is_bare_macro_line(line: &str) -> bool {
    let Some(open) = line.find('(') else { return false };
    let name = &line[..open];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return false;
    }
    let rest = line.trim_end();
    rest.ends_with(')') && paren_group_end(rest.as_bytes(), open) == Some(rest.len())
}

/// Byte spans of identifiers in code (outside comments and string/char literals).
fn code_identifiers(s: &[u8]) -> Vec<(usize, usize)> {
    let n = s.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = s[i];
        if c == b'/' && i + 1 < n && s[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(s[i] == b'*' && s[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if c == b'/' && i + 1 < n && s[i + 1] == b'/' {
            while i < n && s[i] != b'\n' {
                i += 1;
            }
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < n && s[i] != c && s[i] != b'\n' {
                if s[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < n && (s[i].is_ascii_alphanumeric() || s[i] == b'_') {
                i += 1;
            }
            out.push((start, i));
        } else if c.is_ascii_digit() {
            while i < n && (s[i].is_ascii_alphanumeric() || s[i] == b'_' || s[i] == b'.') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Index just past the `(...)` group starting at the first non-blank after `from`.
fn paren_group_end(s: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j < s.len() && (s[j] == b' ' || s[j] == b'\t') {
        j += 1;
    }
    if j >= s.len() || s[j] != b'(' {
        return None;
    }
    let mut depth = 0usize;
    while j < s.len() {
        match s[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// Blank out annotation macros that tree-sitter cannot parse, preserving length
/// and newlines so every byte offset and line number stays valid.
///
/// An identifier is erased when it is in `words` or starts with one of
/// `prefixes`; a directly following balanced `( ... )` is erased with it.
/// String/char literals and comments are skipped.
pub fn erase_macros(src: &str, words: &[&str], prefixes: &[&str]) -> String {
    let s = src.as_bytes();
    let mut out = s.to_vec();
    let mut erased_to = 0;
    for (start, end) in code_identifiers(s) {
        if start < erased_to {
            continue;
        }
        let word = &src[start..end];
        if words.contains(&word) || prefixes.iter().any(|p| word.starts_with(p)) {
            let stop = paren_group_end(s, end).unwrap_or(end);
            for b in &mut out[start..stop] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
            erased_to = stop;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// Rewrite loop macros `NAME(args) body` to `while (args) body`.
///
/// `while (a, b, c)` is a valid C comma expression, so iteration macros such
/// as `foreach(lc, list)` parse as real loops and keep their identifiers.
/// Only names of at least five characters can be rewritten in place.
pub fn loop_macros_as_while(src: &str, names: &[&str]) -> String {
    let s = src.as_bytes();
    let mut out = s.to_vec();
    for (start, end) in code_identifiers(s) {
        let word = &src[start..end];
        if word.len() >= 5 && names.contains(&word) && paren_group_end(s, end).is_some() {
            out[start..start + 5].copy_from_slice(b"while");
            for b in &mut out[start + 5..end] {
                *b = b' ';
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erase_preserves_length_and_lines() {
        let src = "int x pg_node_attr(a, b(c));\n/* PGDLLIMPORT */ extern PGDLLIMPORT int y;\n";
        let out = erase_macros(src, &["pg_node_attr", "PGDLLIMPORT"], &[]);
        assert_eq!(out.len(), src.len());
        assert_eq!(out.lines().count(), src.lines().count());
        assert!(!out.contains("pg_node_attr"));
        assert!(out.contains("/* PGDLLIMPORT */"), "comments are left alone");
        assert!(!out.contains("extern PGDLLIMPORT"));
    }

    #[test]
    fn reparse_recovers_from_conditionals_in_statements() {
        let src = "int a(int t)\n{\n\tif (t == 1)\n\t\treturn 1;\n#ifdef X\n\telse if (t == 2)\n\t\treturn 2;\n#endif\n\telse\n\t{\n\t\treturn 0;\n\t}\n}\n\nGEN_FUNCS(foo)\nint b(void)\n{\n\treturn 3;\n}\n";
        let f = extract(&mut new_parser(), "x.c", src, None);
        let names: Vec<&str> = f.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(f.functions[1].start_line, 16, "line numbers survive the re-parse");
    }

    #[test]
    fn loop_macros_become_loops() {
        let src = "void f(List *l) { ListCell *lc; /* foreach(x) */ foreach(lc, l) { g(lc); } }";
        let out = loop_macros_as_while(src, &["foreach"]);
        assert_eq!(out.len(), src.len());
        assert!(out.contains("while  (lc, l)"));
        assert!(out.contains("/* foreach(x) */"), "comments are left alone");
        let f = extract(&mut new_parser(), "x.c", &out, None);
        assert_eq!(f.parse_errors, 0);
        assert!(f.functions[0].calls.contains("g"));
        assert_eq!(f.functions[0].complexity, 2);
    }

    #[test]
    fn extracts_function_profile() {
        let src = r#"
/*
 * add_checked - add two ints, failing on overflow
 */
static int
add_checked(int a, int b)
{
    int r;
    if (pg_add_s32_overflow(a, b, &r))
        ereport(ERROR,
                (errcode(ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE),
                 errmsg("integer out of range")));
    return r;
}
"#;
        let mut p = new_parser();
        let f = extract(&mut p, "x.c", src, None);
        assert_eq!(f.functions.len(), 1);
        let fun = &f.functions[0];
        assert_eq!(fun.name, "add_checked");
        assert!(fun.is_static);
        assert_eq!(fun.signature, "int add_checked(int, int)");
        assert_eq!(fun.params[1].name, "b");
        assert!(fun.identifiers.contains("ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE"));
        assert!(fun.error_levels.contains("ERROR"));
        assert!(fun.messages.contains(&("errmsg".to_string(), "integer out of range".to_string())));
        assert_eq!(fun.comment.as_deref(), Some("add_checked - add two ints, failing on overflow"));
        assert_eq!(fun.complexity, 2);
    }

    #[test]
    fn body_hash_ignores_comments_and_whitespace() {
        let a = "int f(void) { return 1; }";
        let b = "int f(void)\n{\n    /* one */\n    return   1;\n}\n";
        let c = "int f(void) { return 2; }";
        let h = |s| extract(&mut new_parser(), "x.c", s, None).functions[0].body_hash.clone();
        assert_eq!(h(a), h(b));
        assert_ne!(h(a), h(c));
    }
}
