//! # PostgreSQL `.dat` reader
//!
//! The bootstrap catalog contents (`src/include/catalog/*.dat`) and the GUC
//! table (`src/backend/utils/misc/guc_parameters.dat`) are Perl array-of-hash
//! literals that PostgreSQL's own `Catalog.pm` evaluates. This reads the subset
//! those files use: `#` comments, `[ { key => 'value', ... }, ... ]`, single- or
//! double-quoted values with `\\` / `\'` escapes, and bare tokens.

#[derive(Debug, Clone, Default)]
pub struct DatRecord {
    pub fields: Vec<(String, String)>,
}

impl DatRecord {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

pub fn parse(src: &str) -> Vec<DatRecord> {
    let s: Vec<char> = src.chars().collect();
    let n = s.len();
    let mut i = 0;
    let mut out = Vec::new();
    let mut cur: Option<DatRecord> = None;
    let mut pending_key: Option<String> = None;

    while i < n {
        let c = s[i];
        match c {
            '#' => {
                while i < n && s[i] != '\n' {
                    i += 1;
                }
            }
            '{' => {
                cur = Some(DatRecord { fields: Vec::new() });
                i += 1;
            }
            '}' => {
                if let Some(r) = cur.take() {
                    out.push(r);
                }
                pending_key = None;
                i += 1;
            }
            '=' if i + 1 < n && s[i + 1] == '>' => i += 2,
            '\'' | '"' => {
                let q = c;
                i += 1;
                let mut v = String::new();
                while i < n && s[i] != q {
                    if s[i] == '\\' && i + 1 < n && (s[i + 1] == q || s[i + 1] == '\\') {
                        v.push(s[i + 1]);
                        i += 2;
                        continue;
                    }
                    v.push(s[i]);
                    i += 1;
                }
                i += 1;
                take_token(&mut cur, &mut pending_key, v);
            }
            c if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' => {
                let start = i;
                while i < n && (s[i].is_alphanumeric() || matches!(s[i], '_' | '-' | '.')) {
                    i += 1;
                }
                let tok: String = s[start..i].iter().collect();
                take_token(&mut cur, &mut pending_key, tok);
            }
            _ => i += 1,
        }
    }
    out
}

fn take_token(cur: &mut Option<DatRecord>, pending_key: &mut Option<String>, tok: String) {
    let Some(rec) = cur.as_mut() else { return };
    match pending_key.take() {
        None => *pending_key = Some(tok),
        Some(k) => rec.fields.push((k, tok)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_records() {
        let src = r#"
[
# comment with 'quotes'
{ oid => '177', descr => 'implementation of + operator',
  proname => 'int4pl', prorettype => 'int4', proargtypes => 'int4 int4',
  prosrc => 'int4pl' },
{ name => 'x', short_desc => 'It\'s a \\ test', boot_val => 0 },
]
"#;
        let r = parse(src);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].get("proname"), Some("int4pl"));
        assert_eq!(r[0].get("proargtypes"), Some("int4 int4"));
        assert_eq!(r[1].get("short_desc"), Some("It's a \\ test"));
        assert_eq!(r[1].get("boot_val"), Some("0"));
    }
}
