//! # complexity.nt
//!
//! Written by the same rules as graph.nt: canonical order (sorted N-Triples
//! lines, deduplicated), no blank nodes, refused unless a strict N-Triples
//! loader would accept it.

use crate::ontology::TripleSet;
use std::path::Path;

/// Write `triples` canonically to `path`; returns the sha256 of the bytes.
/// Nothing is written when the text would not pass a strict loader.
pub fn write_complexity(path: &Path, mut triples: TripleSet) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    triples.canonicalize();
    let nt = triples.to_ntriples();
    let bad = crate::ontology::strict_violations(&nt);
    if !bad.is_empty() {
        let shown: Vec<String> = bad
            .iter()
            .take(5)
            .map(|v| format!("line {}: {}", v.line, v.reason))
            .collect();
        anyhow::bail!(
            "complexity.nt is not strict N-Triples: {} line(s) refused, e.g. {}",
            bad.len(),
            shown.join("; ")
        );
    }
    std::fs::write(path, &nt)?;
    Ok(Sha256::digest(nt.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::{Iri, Literal, Triple, OC};

    fn t(s: &str, n: i64) -> Triple {
        Triple::new(
            Iri::new(format!("urn:t:{s}")),
            OC.iri("fanIn"),
            Literal::integer(n),
        )
    }

    #[test]
    fn complexity_nt_is_deterministic_and_strict() {
        let dir = std::env::temp_dir().join(format!("ontosys-cx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.nt"), dir.join("b.nt"));
        let sha_a =
            write_complexity(&a, [t("x", 1), t("y", 2), t("x", 1)].into_iter().collect()).unwrap();
        let sha_b = write_complexity(&b, [t("y", 2), t("x", 1)].into_iter().collect()).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        assert_eq!(sha_a, sha_b);
        assert_eq!(sha_a.len(), 64);
        assert_eq!(
            std::fs::read_to_string(&a).unwrap().lines().count(),
            2,
            "deduplicated"
        );
        let bad: TripleSet = [Triple::new(
            Iri::new("not an iri"),
            OC.iri("fanIn"),
            Literal::integer(1),
        )]
        .into_iter()
        .collect();
        assert!(
            write_complexity(&dir.join("c.nt"), bad).is_err(),
            "a non-strict file is refused"
        );
        assert!(!dir.join("c.nt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
