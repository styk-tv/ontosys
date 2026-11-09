//! # Generic C prepass
//!
//! tree-sitter parses C without running the preprocessor, so two kinds of
//! macro defeat it: iteration macros used as a loop header (`foreach(c, l)
//! { … }`) and annotation macros that expand to nothing or to a compiler
//! attribute (`API int x;`). Both are derived here from the `#define`s of the
//! scanned files themselves, never from a list shipped with ontosys, so any C
//! project — or a carved-out part of one — gets the treatment its own headers
//! call for.

use std::collections::{BTreeMap, BTreeSet};

/// Macro names derived from the scanned tree's own definitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MacroLists {
    /// Function-like macros whose expansion opens a `for`/`while` loop.
    pub loop_macros: BTreeSet<String>,
    /// Macros whose expansion is empty or only compiler attributes.
    pub erased: BTreeSet<String>,
    /// Every macro name defined in the scanned files.
    pub defined: BTreeSet<String>,
    /// Loop macros whose names are too short to become `while` in place:
    /// derived, but not rewritten (their uses stay parse errors).
    pub short_loop_macros: BTreeSet<String>,
    /// Every loop macro's distinct definitions: (parameters, replacement).
    pub loop_defs: BTreeMap<String, Vec<(Vec<String>, String)>>,
}

impl MacroLists {
    /// The `for (…)`/`while (…)` header a loop macro expands to with these
    /// arguments, following loop macros defined through others. `None` when
    /// the name has several different definitions, or the arguments do not
    /// match its parameters.
    pub fn expand_loop_header(&self, name: &str, args: &[String]) -> Option<String> {
        let mut name = name.to_string();
        let mut args = args.to_vec();
        for _ in 0..8 {
            let defs = self.loop_defs.get(&name)?;
            let [(params, body)] = defs.as_slice() else {
                return None;
            };
            let variadic = params.last().is_some_and(|p| p == "...");
            let fixed = if variadic { params.len() - 1 } else { params.len() };
            if args.len() < fixed || (!variadic && args.len() != fixed) {
                return None;
            }
            let expanded = substitute(body, &params[..fixed], &args, variadic.then(|| args[fixed..].join(", ")));
            let mut s = expanded.trim_start();
            if let Some(rest) = s.strip_prefix("do") {
                if let Some(rest) = rest.trim_start().strip_prefix('{') {
                    s = rest.trim_start();
                }
            }
            let (word, rest) = split_ident(s);
            let rest_t = rest.trim_start();
            let end = close_paren(rest_t.as_bytes(), 0)?;
            if matches!(word, "for" | "while") {
                return Some(format!("{word} {}", &rest_t[..end]));
            }
            if !self.loop_defs.contains_key(word) {
                return None;
            }
            args = split_args(&rest_t[1..end - 1]);
            name = word.to_string();
        }
        None
    }
}

/// Top-level comma-separated arguments of a macro call, trimmed.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut cur) = (0i32, String::new());
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// A replacement text with each parameter (a whole identifier, outside
/// string and character literals) replaced by its argument; `__VA_ARGS__`
/// by the rest; `#` and `##` dropped.
fn substitute(body: &str, params: &[String], args: &[String], rest: Option<String>) -> String {
    let b = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            out.push_str(&body[start..i]);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &body[start..i];
            match params.iter().position(|p| p == word) {
                Some(k) => out.push_str(&args[k]),
                None if word == "__VA_ARGS__" => out.push_str(rest.as_deref().unwrap_or("")),
                None => out.push_str(word),
            }
        } else if c == b'#' {
            i += 1;
        } else {
            let ch = body[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// What one `#define` expands to, as far as the prepass cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    LoopHeader,
    Erasable,
    Other,
}

/// Derive the lists from `(repository-relative path, raw text)` pairs.
///
/// A name is a loop macro (or erased) only when every one of its definitions
/// in the scanned files is: `#ifdef` alternatives must agree. Definitions in
/// terms of other macros count once those are known (`foreach_ptr(…)` →
/// `foreach_internal(…)` → `for (…)`), so the classification runs to a
/// fixed point.
pub fn derive_macros(files: &[(String, String)]) -> MacroLists {
    let mut defs: BTreeMap<String, Vec<(bool, String)>> = BTreeMap::new();
    let mut with_params: BTreeMap<String, BTreeSet<(Vec<String>, String)>> = BTreeMap::new();
    for (_, text) in files {
        for (name, params, body) in defines(text) {
            defs.entry(name.clone()).or_default().push((params.is_some(), body.clone()));
            if let Some(ps) = params {
                with_params.entry(name).or_default().insert((ps, body));
            }
        }
    }
    let mut out = MacroLists {
        defined: defs.keys().cloned().collect(),
        ..Default::default()
    };
    loop {
        let mut changed = false;
        for (name, ds) in &defs {
            if out.erased.contains(name)
                || out.loop_macros.contains(name)
                || C_KEYWORDS.contains(&name.as_str())
            {
                continue;
            }
            let kinds: BTreeSet<Kind> = ds
                .iter()
                .map(|(is_fn, body)| kind_of(body, *is_fn, &out))
                .collect();
            if kinds.len() != 1 {
                continue;
            }
            match kinds.into_iter().next() {
                Some(Kind::Erasable) => {
                    out.erased.insert(name.clone());
                    changed = true;
                }
                Some(Kind::LoopHeader) if ds.iter().all(|(is_fn, _)| *is_fn) => {
                    out.loop_macros.insert(name.clone());
                    changed = true;
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    for name in &out.loop_macros {
        if let Some(ds) = with_params.get(name) {
            out.loop_defs.insert(name.clone(), ds.iter().cloned().collect());
        }
    }
    // `while` needs five characters of room: a shorter name cannot be
    // rewritten in place, so it is listed apart, not claimed as handled.
    let short: Vec<String> = out.loop_macros.iter().filter(|n| n.len() < 5).cloned().collect();
    for n in short {
        out.loop_macros.remove(&n);
        out.short_loop_macros.insert(n);
    }
    out
}

/// `(name, function-like, replacement text without comments)` for every
/// `#define` in `text`, with `\` continuation lines joined.
fn defines(text: &str) -> Vec<(String, Option<Vec<String>>, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(first) = lines.next() {
        let t = first.trim_start();
        let Some(rest) = t.strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("define") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let mut logical = rest.to_string();
        while logical.ends_with('\\') {
            logical.pop();
            match lines.next() {
                Some(next) => {
                    logical.push(' ');
                    logical.push_str(next);
                }
                None => break,
            }
        }
        let logical = strip_comments(logical.trim_start());
        let name_end = logical
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(logical.len());
        if name_end == 0 {
            continue;
        }
        let name = logical[..name_end].to_string();
        let after = &logical[name_end..];
        let (params, body) = if after.starts_with('(') {
            match close_paren(after.as_bytes(), 0) {
                Some(end) => (
                    Some(
                        after[1..end - 1]
                            .split(',')
                            .map(|p| p.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .collect(),
                    ),
                    after[end..].to_string(),
                ),
                None => continue,
            }
        } else {
            (None, after.to_string())
        };
        out.push((name, params, body.trim().to_string()));
    }
    out
}

fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            match s[i + 2..].find("*/") {
                Some(end) => i += end + 4,
                None => break,
            }
            out.push(' ');
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            break;
        } else {
            let ch = s[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Index just past the parenthesis group that opens at `open`.
fn close_paren(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn kind_of(body: &str, is_fn: bool, known: &MacroLists) -> Kind {
    if is_attribute_only(body, &known.erased) {
        return Kind::Erasable;
    }
    if !is_fn {
        return Kind::Other;
    }
    let mut s = body.trim_start();
    if let Some(rest) = s.strip_prefix("do") {
        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix('{') {
            s = rest.trim_start();
        }
    }
    let (word, rest) = split_ident(s);
    let is_header = matches!(word, "for" | "while") || known.loop_macros.contains(word);
    let rest = rest.trim_start();
    if !is_header || !rest.starts_with('(') {
        return Kind::Other;
    }
    let Some(end) = close_paren(rest.as_bytes(), 0) else {
        return Kind::Other;
    };
    // Braces count only outside parentheses: a nested `for (T x = {…}; …)`
    // header still leaves the loop open.
    let (mut opens, mut closes, mut depth) = (0usize, 0usize, 0usize);
    for c in rest[end..].bytes() {
        match c {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'{' if depth == 0 => opens += 1,
            b'}' if depth == 0 => closes += 1,
            _ => {}
        }
    }
    if opens == 0 || opens > closes {
        Kind::LoopHeader
    } else {
        Kind::Other
    }
}

/// `(leading identifier, rest)`; the identifier is empty when there is none.
fn split_ident(s: &str) -> (&str, &str) {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    (&s[..end], &s[end..])
}

/// C keywords (C23 and the common compiler spellings). A project may
/// `#define` one of them for an old compiler (`#define inline`), but erasing
/// or rewriting it would break every file that uses the keyword.
const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Bool",
    "_Complex",
    "_Generic",
    "_Imaginary",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "alignas",
    "alignof",
    "bool",
    "constexpr",
    "false",
    "nullptr",
    "static_assert",
    "thread_local",
    "true",
    "typeof",
    "typeof_unqual",
    "__inline",
    "__inline__",
    "__restrict",
    "__restrict__",
    "__volatile__",
    "__const",
    "__signed__",
    "__typeof__",
    "asm",
    "__asm__",
];

/// Keywords that only annotate a declaration (no storage class, no type):
/// a macro expanding to nothing but these erases without losing structure.
const ANNOTATION_KEYWORDS: &[&str] = &[
    "_Noreturn",
    "noreturn",
    "restrict",
    "__restrict",
    "__restrict__",
    "__forceinline",
];

/// Empty, or nothing but `__attribute__((…))` / `__declspec(…)` groups,
/// annotation-only keywords, and invocations of macros already known to erase.
fn is_attribute_only(body: &str, erased: &BTreeSet<String>) -> bool {
    let mut s = body.trim();
    while !s.is_empty() {
        let (word, rest) = split_ident(s);
        let rest = rest.trim_start();
        match word {
            // C23 attribute specifier: `[[nodiscard]]`, `[[maybe_unused, gnu::cold]]`
            "" if s.starts_with("[[") => {
                let Some(end) = s.find("]]") else {
                    return false;
                };
                s = s[end + 2..].trim_start();
            }
            w if ANNOTATION_KEYWORDS.contains(&w) => s = rest,
            "__attribute__" | "__declspec" => {
                if !rest.starts_with('(') {
                    return false;
                }
                let Some(end) = close_paren(rest.as_bytes(), 0) else {
                    return false;
                };
                s = rest[end..].trim_start();
            }
            w if !w.is_empty() && erased.contains(w) => {
                s = match rest
                    .starts_with('(')
                    .then(|| close_paren(rest.as_bytes(), 0))
                    .flatten()
                {
                    Some(end) => rest[end..].trim_start(),
                    None => rest,
                };
            }
            _ => return false,
        }
    }
    true
}

/// Length- and newline-preserving rewrite: loop macros become `while`,
/// erased macros (with their argument list) become blanks.
///
/// Preprocessor lines keep their original bytes: an include guard is defined
/// empty, and blanking it out of its own `#ifndef` would break the header.
pub fn prepass(src: &str, lists: &MacroLists) -> String {
    let loops: Vec<&str> = lists.loop_macros.iter().map(String::as_str).collect();
    let erased: Vec<&str> = lists.erased.iter().map(String::as_str).collect();
    let s = super::loop_macros_as_while(src, &loops);
    let rewritten = super::erase_macros(&s, &erased, &[]);
    let mut out = String::with_capacity(src.len());
    let mut continuing = false;
    for (orig, new) in src
        .split_inclusive('\n')
        .zip(rewritten.split_inclusive('\n'))
    {
        let directive = continuing || orig.trim_start().starts_with('#');
        out.push_str(if directive { orig } else { new });
        continuing = directive && orig.trim_end_matches(['\n', '\r']).ends_with('\\');
    }
    out
}

const KEYWORDS: &[&str] = &[
    "if", "while", "for", "switch", "return", "sizeof", "do", "else", "case", "defined",
];

/// Indented lines that are exactly `NAME(args)`, with no `;`, where NAME is
/// defined nowhere in the scanned files: a macro whose loop or statement the
/// parser cannot see.
pub fn unresolved_macro_statements(src: &str, defined: &BTreeSet<String>) -> usize {
    src.lines()
        .filter(|line| line.starts_with([' ', '\t']))
        .filter(|line| {
            let t = line.trim();
            let name_end = t
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(t.len());
            if name_end == 0 || !t[name_end..].starts_with('(') {
                return false;
            }
            let name = &t[..name_end];
            if KEYWORDS.contains(&name) || defined.contains(name) {
                return false;
            }
            close_paren(t.as_bytes(), name_end) == Some(t.len())
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(src: &str) -> MacroLists {
        derive_macros(&[("x.h".to_string(), src.to_string())])
    }

    #[test]
    fn derives_loop_macro_from_for_header() {
        let m = lists("#define foreach(c, l) for (int c = 0; c < (l); c++)\n");
        assert_eq!(m.loop_macros, BTreeSet::from(["foreach".to_string()]));
        assert!(m.defined.contains("foreach"));
    }

    #[test]
    fn derives_loop_macro_through_do_brace() {
        let m = lists(
            "#define each(x) do { for (;;)\n#define SWAP(a,b) do { t=a; a=b; b=t; } while (0)\n",
        );
        assert!(m.short_loop_macros.contains("each"), "derived; too short to rewrite");
        assert!(!m.loop_macros.contains("SWAP") && !m.short_loop_macros.contains("SWAP"));
    }

    #[test]
    fn braces_inside_a_nested_header_do_not_close_the_loop() {
        let m = lists(
            "#define foreach_internal(type, pointer, var, lst, func) \\\n\
             \tfor (type pointer var = 0, pointer var##__outerloop = (type pointer) 1; var##__outerloop; var##__outerloop = 0) \\\n\
             \t\tfor (ForEachState var##__state = {(lst), 0}; var##__state.i < 1; var##__state.i++)\n\
             #define foreach_ptr(type, var, lst) foreach_internal(type, *, var, lst, lfirst)\n",
        );
        assert!(m.loop_macros.contains("foreach_internal"));
        assert!(m.loop_macros.contains("foreach_ptr"));
    }

    #[test]
    fn a_macro_holding_a_whole_braced_loop_is_not_a_loop_header() {
        let m = lists("#define ZERO(a, n) for (int i = 0; i < (n); i++) { (a)[i] = 0; }\n");
        assert!(!m.loop_macros.contains("ZERO"));
    }

    #[test]
    fn continuation_lines_are_joined() {
        let m = lists("#define dlist_foreach(iter, lhead) \\\n\tfor ((iter).end = &(lhead)->head; \\\n\t\t(iter).cur != (iter).end; \\\n\t\t(iter).cur = (iter).cur->next)\n");
        assert!(m.loop_macros.contains("dlist_foreach"));
    }

    #[test]
    fn derives_erased_from_empty_and_attribute_bodies() {
        let m = lists(
            "#define API\n#define NORET __attribute__((noreturn))\n#define node_attr(...)\n#define X 1\n\
             #define DLL __declspec (dllimport)\n#define UNUSED /* nothing */\n",
        );
        for name in ["API", "NORET", "node_attr", "DLL", "UNUSED"] {
            assert!(m.erased.contains(name), "{name} should be erased");
        }
        assert!(!m.erased.contains("X"));
    }

    #[test]
    fn c_keywords_are_never_erased_or_rewritten() {
        let m =
            lists("#define extern\n#define inline\n#define while(x) for (;;)\n#define static\n");
        for kw in ["extern", "inline", "while", "static"] {
            assert!(
                !m.erased.contains(kw) && !m.loop_macros.contains(kw),
                "{kw} is C, not a project macro"
            );
        }
    }

    #[test]
    fn keyword_only_portability_spellings_are_erasable() {
        let m = lists(
            "#define NORET _Noreturn\n#define NORET __attribute__((noreturn))\n#define RESTRICT __restrict\n\
             #define NODISCARD __attribute__((warn_unused_result)) _Noreturn\n#define NOT_THIS register\n",
        );
        assert!(m.erased.contains("NORET"));
        assert!(m.erased.contains("RESTRICT"));
        assert!(m.erased.contains("NODISCARD"));
        assert!(
            !m.erased.contains("NOT_THIS"),
            "storage classes carry structure and stay"
        );
    }

    #[test]
    fn c23_attribute_groups_are_erasable() {
        let m = lists(
            "#define NODISCARD [[nodiscard]]\n#define NODISCARD __attribute__((warn_unused_result))\n#define NODISCARD\n\
             #define MAYBE [[maybe_unused, gnu::cold]] __attribute__((cold))\n#define ARR a[[i]]\n",
        );
        assert!(m.erased.contains("NODISCARD"));
        assert!(m.erased.contains("MAYBE"));
        assert!(!m.erased.contains("ARR"));
    }

    #[test]
    fn conflicting_definitions_need_all_to_agree() {
        let m = derive_macros(&[
            ("a.h".to_string(), "#ifdef WIN32\n#define API __declspec(dllimport)\n#else\n#define API\n#endif\n#define M\n".to_string()),
            ("b.h".to_string(), "#define M 1\n".to_string()),
        ]);
        assert!(m.erased.contains("API"));
        assert!(!m.erased.contains("M"));
    }

    #[test]
    fn prepass_preserves_length_and_lines() {
        let m = lists(
            "#define foreach(c, l) for (;;)\n#define API\n#define attr(x) __attribute__((x))\n",
        );
        let src = "API int f(List *l) attr(cold);\nint g(void) {\n\tforeach(c, l)\n\t{\n\t\th(c);\n\t}\n}\n";
        let out = prepass(src, &m);
        assert_eq!(out.len(), src.len());
        assert_eq!(out.matches('\n').count(), src.matches('\n').count());
        assert!(out.contains("\twhile  (c, l)\n"), "{out}");
        assert!(!out.contains("API"));
        assert!(!out.contains("attr(cold)"));
        assert!(!out.contains("foreach"));
    }

    #[test]
    fn statement_macro_with_no_definition_is_counted() {
        let src = "void f(void)\n{\n\tUNKNOWN_LOOP(x)\n\t{ y(); }\n\tcall(a);\n}\nstatic int\nmain_like(int argc)\n{\n}\n";
        assert_eq!(unresolved_macro_statements(src, &BTreeSet::new()), 1);
        let defined = BTreeSet::from(["UNKNOWN_LOOP".to_string()]);
        assert_eq!(unresolved_macro_statements(src, &defined), 0);
    }

    #[test]
    fn include_guards_survive_erasure() {
        let src = "#ifndef FOO_H\n#define FOO_H\n#define API\nAPI int f(void);\n#endif\n";
        let m = lists(src);
        assert!(m.erased.contains("FOO_H"));
        let out = prepass(src, &m);
        assert!(out.contains("#ifndef FOO_H\n#define FOO_H\n"), "{out}");
        assert!(out.contains("\n    int f(void);\n"), "{out}");
    }

    #[test]
    fn macros_defined_through_macros_are_followed() {
        let m = lists(
            "#define foreach_internal(a, b) for (;;)\n#define foreach_ptr(t, v, l) foreach_internal(t, l)\n\
             #define attr_unused() __attribute__((unused))\n#define ASSERT_ONLY attr_unused()\n",
        );
        assert!(m.loop_macros.contains("foreach_ptr"));
        assert!(m.erased.contains("ASSERT_ONLY"));
    }

    #[test]
    fn short_loop_macros_are_listed_apart_and_not_claimed_handled() {
        let m = lists("#define REP(i, n) for (int i = 0; i < (n); i++)\n#define each(x) for (;;)\n#define REPEAT(i, n) for (int i = 0; i < (n); i++)\n");
        assert_eq!(m.loop_macros, BTreeSet::from(["REPEAT".to_string()]));
        assert_eq!(
            m.short_loop_macros,
            BTreeSet::from(["REP".to_string(), "each".to_string()]),
            "a name under five characters cannot become `while` in place"
        );
    }

    #[test]
    fn a_loop_macro_header_expands_with_its_arguments() {
        let m = lists("#define foreach_internal(a, b) for (int a = 0; a < (b); a++)\n#define foreach_ptr(t, v, l) foreach_internal(v, l)\n#define DLIST(it, h) for ((it).cur = (h)->next; (it).cur != (h); (it).cur = (it).cur->next)\n");
        assert_eq!(
            m.expand_loop_header("foreach_ptr", &["T".into(), "x".into(), "list->n".into()]).as_deref(),
            Some("for (int x = 0; x < (list->n); x++)")
        );
        assert_eq!(
            m.expand_loop_header("DLIST", &["iter".into(), "head".into()]).as_deref(),
            Some("for ((iter).cur = (head)->next; (iter).cur != (head); (iter).cur = (iter).cur->next)")
        );
        assert_eq!(m.expand_loop_header("nope", &[]), None);
    }

    #[test]
    fn keywords_are_never_counted_as_macros() {
        let src = "\tif (x)\n\twhile (y)\n\tswitch (z)\n\tfor (;;)\n";
        assert_eq!(unresolved_macro_statements(src, &BTreeSet::new()), 0);
    }
}
