//! Stamps the binary with the ontosys commit it was built from, so every
//! `.ontosys/data/build-meta.json` names the exact tool that wrote it.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = git(&["rev-parse", "HEAD"])
        .filter(|c| c.len() == 40)
        .unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    println!("cargo:rustc-env=ONTOSYS_COMMIT={commit}");
    println!("cargo:rustc-env=ONTOSYS_DIRTY={dirty}");
    for path in [
        ".git/HEAD",
        ".git/index",
        ".git/refs",
        ".git/packed-refs",
        "src",
        "ontology",
        "Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
