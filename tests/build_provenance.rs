//! A repository's own cost overlay and claims are inputs of its build:
//! build-meta records their hashes, and an overlay never replaces the
//! shipped catalog's version.

use sha2::{Digest, Sha256};
use std::fs;
use std::process::Command;

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn build_meta_records_overlay_and_claims() {
    let root = std::env::temp_dir().join(format!("ontosys-prov-it-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("app.c"), "int f(int n) { return n; }\n").unwrap();
    let git = |args: &[&str]| {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap()
            .status
            .success())
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@localhost",
        "commit",
        "-qm",
        "one",
    ]);
    let exe = env!("CARGO_BIN_EXE_ontosys");
    let run = |cmd: &str| {
        let out = Command::new(exe)
            .arg(cmd)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run("init");
    let overlay = "@prefix code: <https://ontosys.io/ns/code#> .\n@prefix owl: <http://www.w3.org/2002/07/owl#> .\n<urn:mine> owl:versionInfo \"mine 7\" .\n[] a code:SizeCap ; code:sizeName \"npos\" ; code:cappedBy \"MAXNUMPOS\" .\n";
    let claims = "@prefix code: <https://ontosys.io/ns/code#> .\n";
    fs::write(repo.join(".ontosys/cost-catalog.ttl"), overlay).unwrap();
    fs::create_dir_all(repo.join(".ontosys/claims")).unwrap();
    fs::write(repo.join(".ontosys/claims/complexity.ttl"), claims).unwrap();
    run("build");
    let meta: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join(".ontosys/data/build-meta.json")).unwrap())
            .unwrap();
    let c = &meta["complexity"];
    assert_eq!(c["overlay_sha256"], sha(overlay.as_bytes()));
    assert_eq!(c["overlay_version"], "mine 7");
    assert_eq!(c["claims_sha256"], sha(claims.as_bytes()));
    assert!(
        c["catalog_version"].as_str().unwrap().starts_with("v0"),
        "the shipped catalog's version: {}",
        c["catalog_version"]
    );
    // §7's cost budget is checkable from the record: each phase is timed.
    let t = &c["timings_ms"];
    for phase in ["parse", "tier_a", "tier_b", "tier_c"] {
        assert!(t[phase].is_u64(), "timings_ms.{phase}: {t}");
    }
    assert!(c["call_edges"].is_u64(), "Tier B's budget is per edge");
    fs::remove_dir_all(&root).ok();
}
