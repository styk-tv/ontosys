//! `ontosys survey --rebuild` measures a repository in a temporary clone and
//! leaves the owner's tree exactly as it was.

use std::fs;
use std::path::Path;
use std::process::Command;

fn files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().display().to_string(), fs::read(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

#[test]
fn rebuild_leaves_the_owner_tree_untouched() {
    let root = std::env::temp_dir().join(format!("ontosys-rebuild-it-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("app.py"),
        "def total(xs):\n    s = 0\n    for x in xs:\n        s += x\n    return s\n",
    )
    .unwrap();
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
    let before = files(&repo);

    let ledger = root.join("ledger.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_ontosys"))
        .args(["survey", "--rebuild"])
        .arg(&repo)
        .arg("--ledger")
        .arg(&ledger)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(files(&repo), before, "the owner's tree is byte-identical");
    assert!(!repo.join(".ontosys").exists());
    let line = fs::read_to_string(&ledger).unwrap();
    let snap: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(snap["source"], "rebuild");
    assert_eq!(snap["repo"], repo.display().to_string());
    assert_eq!(snap["functions_by_language"]["python"], 1);
    fs::remove_dir_all(&root).ok();
}
