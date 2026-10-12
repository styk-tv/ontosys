//! # Strict N-Triples check
//!
//! A graph file is only usable by a strict loader if every line is valid
//! N-Triples. `violations` finds the lines that are not, so the build can
//! refuse to write them.

/// One refused line: its 1-based number and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub reason: String,
}

/// Lines of `nt` that a strict N-Triples loader would refuse.
pub fn violations(nt: &str) -> Vec<Violation> {
    nt.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|(i, l)| line_problem(l).map(|reason| Violation { line: i + 1, reason }))
        .collect()
}

/// Characters an N-Triples IRIREF may not contain.
fn forbidden_in_iri(c: char) -> bool {
    c <= ' ' || matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\')
}

/// Read `<...>` at the start of `s`; the rest of `s` follows the closing `>`.
fn iri_ref(s: &str) -> Result<&str, String> {
    let body = s.strip_prefix('<').ok_or("expected <IRI>")?;
    let end = body.find('>').ok_or("unterminated IRI")?;
    if let Some(c) = body[..end].chars().find(|&c| forbidden_in_iri(c)) {
        return Err(format!("character {:?} is not allowed in an IRI: <{}>", c, &body[..end]));
    }
    Ok(&body[end + 1..])
}

fn line_problem(line: &str) -> Option<String> {
    let rest = match iri_ref(line.trim_end()) {
        Ok(r) => r.trim_start(),
        Err(e) => return Some(format!("subject: {}", e)),
    };
    let rest = match iri_ref(rest) {
        Ok(r) => r.trim_start(),
        Err(e) => return Some(format!("predicate: {}", e)),
    };
    let rest = if rest.starts_with('<') {
        match iri_ref(rest) {
            Ok(r) => r,
            Err(e) => return Some(format!("object: {}", e)),
        }
    } else if let Some(body) = rest.strip_prefix('"') {
        let mut escaped = false;
        let mut close = None;
        for (i, c) in body.char_indices() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                close = Some(i);
                break;
            }
        }
        let Some(close) = close else { return Some("object: unterminated literal".into()) };
        let tail = &body[close + 1..];
        if let Some(dt) = tail.strip_prefix("^^") {
            match iri_ref(dt) {
                Ok(r) => r,
                Err(e) => return Some(format!("datatype: {}", e)),
            }
        } else if let Some(lang) = tail.strip_prefix('@') {
            let end = lang.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(lang.len());
            &lang[end..]
        } else {
            tail
        }
    } else {
        return Some("object: expected <IRI> or a literal".into());
    };
    if rest.trim() == "." { None } else { Some("expected a single '.' after the object".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "<http://e/s> <http://e/p> \"a literal with <angle> {braces} and a space\" .\n\
<http://e/s> <http://e/p> <http://e/o> .\n\
<http://e/s> <http://e/p> \"7\"^^<http://www.w3.org/2001/XMLSchema#integer> .\n";

    #[test]
    fn accepts_valid_lines_including_awkward_literals() {
        assert_eq!(violations(OK), vec![]);
    }

    #[test]
    fn refuses_a_space_in_a_subject_iri() {
        let nt = "<http://e/fn/a b> <http://e/p> <http://e/o> .\n";
        let v = violations(nt);
        assert_eq!(v.len(), 1, "{:?}", v);
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn refuses_braces_in_an_object_iri_and_reports_the_line_number() {
        let nt = format!("{}<http://e/s> <http://e/p> <http://e/import/std_{{A, B}}> .\n", OK);
        let v = violations(&nt);
        assert_eq!(v.iter().map(|x| x.line).collect::<Vec<_>>(), vec![4]);
    }

    #[test]
    fn refuses_a_line_that_is_not_three_terms_and_a_dot() {
        assert_eq!(violations("<http://e/s> <http://e/p>\n").len(), 1);
    }
}
