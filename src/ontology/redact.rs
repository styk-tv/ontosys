//! # Email redaction
//!
//! Source comments and documents carry example addresses. A graph that is
//! shared should not, whatever the domain, so every address becomes `<email>`.

pub const PLACEHOLDER: &str = "<email>";

/// `s` with every email address replaced by `<email>`.
pub fn emails(s: &str) -> String {
    let local = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let domain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-');
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('@') {
        let before = &rest[..at];
        let start = before.rfind(|c: char| !local(c)).map(|i| i + before[i..].chars().next().unwrap().len_utf8()).unwrap_or(0);
        let after = &rest[at + 1..];
        let run = after.find(|c: char| !domain(c)).unwrap_or(after.len());
        let host = after[..run].trim_end_matches(|c| c == '.' || c == '-');
        let tld_ok = host.rsplit_once('.').map_or(false, |(h, tld)| {
            !h.is_empty() && !h.starts_with('.') && !h.contains("..") && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
        });
        if start < at && tld_ok {
            out.push_str(&rest[..start]);
            out.push_str(PLACEHOLDER);
            rest = &after[host.len()..];
        } else {
            out.push_str(&rest[..at + 1]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_an_address_whatever_its_domain() {
        assert_eq!(emails("write to dorothy@example.com today"), "write to <email> today");
        assert_eq!(emails("test02@example.com and a.b+c@x.co.uk"), "<email> and <email>");
    }

    #[test]
    fn keeps_trailing_punctuation_and_surrounding_markup() {
        assert_eq!(emails("(mail me: a@b.io), thanks."), "(mail me: <email>), thanks.");
        assert_eq!(emails("\"user\": \"x@y.org\","), "\"user\": \"<email>\",");
    }

    #[test]
    fn leaves_non_addresses_alone() {
        for s in ["@decorator", "a @ b", "git@host", "price @ 5.00", "no at sign", "x@y"] {
            assert_eq!(emails(s), s, "{}", s);
        }
    }

    #[test]
    fn handles_non_ascii_without_panicking() {
        assert_eq!(emails("é é@x.io ü"), "é <email> ü");
    }
}
