//! # Survey Command
//!
//! Reads every `.ontosys/` folder under the given roots and records one
//! snapshot per folder in a private, append-only ledger outside every
//! repository (`~/.ontosys/survey/ledger.jsonl` by default), so progression
//! can be measured over time. It writes nothing into the repositories it
//! reads. `--rebuild REPO` measures a repository its owner has not rebuilt:
//! it clones HEAD to a temporary directory, builds there, and records that.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
];

/// Median, 90th percentile and maximum of a distribution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    pub median: u64,
    pub p90: u64,
    pub max: u64,
}

/// One folder, at one moment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub at: String,
    pub repo: String,
    /// "folder" (read as the owner left it) or "rebuild" (built in a clone).
    pub source: String,
    pub staleness: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_dirty: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ontosys_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ontosys_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs_sha256: Option<String>,
    /// The repository's cost overlay and claims, as the build read them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_sha256: Option<String>,
    pub functions_by_language: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyclomatic: Option<Spread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cognitive: Option<Spread>,
    pub by_basis: BTreeMap<String, u64>,
    pub by_reason: BTreeMap<String, u64>,
    pub loops_by_class: BTreeMap<String, u64>,
    pub findings_by_kind: BTreeMap<String, u64>,
    pub unlock_top10: Vec<(String, u64)>,
    pub unparsed_by_extension: BTreeMap<String, u64>,
    /// The 100 functions with the highest cyclomatic number (IRI, value).
    pub top_cyclomatic: Vec<(String, u64)>,
}

/// Repositories under `roots` (to `depth` levels) that hold a `.ontosys/`.
pub fn discover(roots: &[PathBuf], depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let walker = walkdir::WalkDir::new(root)
            .max_depth(depth)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                e.depth() == 0 || !(SKIP_DIRS.contains(&name.as_ref()) || name == ".ontosys")
            });
        for e in walker.flatten() {
            if e.file_type().is_dir() && e.path().join(".ontosys").is_dir() {
                out.push(e.path().to_path_buf());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn spread(mut v: Vec<u64>) -> Option<Spread> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let n = v.len();
    Some(Spread {
        median: v[n / 2],
        p90: v[(n * 9 / 10).min(n - 1)],
        max: v[n - 1],
    })
}

fn counts(v: Option<&serde_json::Value>) -> BTreeMap<String, u64> {
    v.and_then(|x| x.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default()
}

fn git_head(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Read one repository's folder. Reads only; writes nothing.
pub fn snapshot(repo: &Path, source: &str, installed_version: &str) -> Snapshot {
    let mut s = Snapshot {
        at: chrono::Utc::now().to_rfc3339(),
        repo: repo.display().to_string(),
        source: source.into(),
        ..Default::default()
    };
    let data = repo.join(".ontosys/data");
    let Some(meta) = std::fs::read_to_string(data.join("build-meta.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        s.staleness.push("no-meta".into());
        return s;
    };
    let text = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(str::to_string);
    s.git_head = text("git_head");
    s.git_dirty = meta.get("git_dirty").and_then(|v| v.as_bool());
    s.ontosys_version = text("ontosys_version");
    s.ontosys_commit = text("ontosys_commit");
    s.built_at = text("timestamp");
    s.graph_sha256 = text("graph_sha256");
    s.complexity_sha256 = text("complexity_sha256");
    s.inputs_sha256 = text("inputs_sha256");
    s.unparsed_by_extension = counts(meta.get("unparsed_by_extension"));
    let cx = meta.get("complexity");
    let cx_text = |k: &str| cx.and_then(|c| c.get(k)).and_then(|v| v.as_str()).map(str::to_string);
    s.overlay_sha256 = cx_text("overlay_sha256");
    s.claims_sha256 = cx_text("claims_sha256");
    if let Some(tier_a) = cx.and_then(|c| c.get("tier_a")).and_then(|t| t.as_object()) {
        for (lang, st) in tier_a {
            if let Some(n) = st.get("analysed").and_then(|v| v.as_u64()) {
                s.functions_by_language.insert(lang.clone(), n);
            }
        }
    }
    s.by_basis = counts(cx.and_then(|c| c.get("by_basis")));
    s.by_reason = counts(cx.and_then(|c| c.get("by_reason")));
    s.loops_by_class = counts(cx.and_then(|c| c.get("loops_by_class")));
    s.findings_by_kind = counts(cx.and_then(|c| c.get("findings_by_kind")));
    s.unlock_top10 = cx
        .and_then(|c| c.get("unlock"))
        .and_then(|u| u.as_array())
        .map(|a| {
            a.iter()
                .take(10)
                .filter_map(|e| {
                    Some((
                        e.get("name")?.as_str()?.to_string(),
                        e.get("functions")?.as_u64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    // Distributions from graph.nt (streamed; only two predicates are read).
    let (mut cyclo, mut cogn): (Vec<(String, u64)>, Vec<u64>) = (Vec::new(), Vec::new());
    if let Ok(f) = std::fs::File::open(data.join("graph.nt")) {
        use std::io::BufRead;
        for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
            let mut parts = line.splitn(3, ' ');
            let (Some(subj), Some(pred), Some(obj)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let value = || obj.split('"').nth(1).and_then(|v| v.parse::<u64>().ok());
            match pred {
                "<https://ontosys.io/ns/code#cyclomatic>" => {
                    if let Some(v) = value() {
                        cyclo.push((subj.trim_matches(|c| c == '<' || c == '>').to_string(), v));
                    }
                }
                "<https://ontosys.io/ns/code#cognitive>" => {
                    if let Some(v) = value() {
                        cogn.push(v);
                    }
                }
                _ => {}
            }
        }
    }
    s.cyclomatic = spread(cyclo.iter().map(|x| x.1).collect());
    s.cognitive = spread(cogn);
    cyclo.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    cyclo.truncate(100);
    s.top_cyclomatic = cyclo;
    if s.ontosys_version.as_deref() != Some(installed_version) {
        s.staleness.push("stale-tool".into());
    }
    if let (Some(recorded), Some(now)) = (
        s.git_head.as_deref().filter(|h| !h.is_empty()),
        git_head(repo),
    ) {
        if recorded != now {
            s.staleness.push("stale-tree".into());
        }
    }
    if s.staleness.is_empty() {
        s.staleness.push("current".into());
    }
    s
}

/// Append snapshots to the ledger, one JSON object per line.
pub fn append(ledger: &Path, snaps: &[Snapshot]) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(dir) = ledger.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger)?;
    for s in snaps {
        writeln!(f, "{}", serde_json::to_string(s)?)?;
    }
    Ok(())
}

fn total(m: &BTreeMap<String, u64>) -> u64 {
    m.values().sum()
}

fn delta(a: u64, b: u64) -> String {
    let d = b as i64 - a as i64;
    format!("{a} -> {b} ({}{d})", if d >= 0 { "+" } else { "" })
}

/// Progression per folder: the latest snapshot against the previous one
/// made from the same ontosys commit (so tool changes are not read as code
/// changes).
pub fn report(ledger: &Path) -> anyhow::Result<String> {
    use std::fmt::Write;
    let text = std::fs::read_to_string(ledger).unwrap_or_default();
    let mut by_repo: BTreeMap<String, Vec<Snapshot>> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let s: Snapshot = serde_json::from_str(line)?;
        by_repo.entry(s.repo.clone()).or_default().push(s);
    }
    let mut r = String::new();
    for (repo, snaps) in &by_repo {
        let latest = snaps.last().expect("non-empty");
        writeln!(r, "{repo}  [{}]", latest.staleness.join(", "))?;
        let previous = snaps[..snaps.len() - 1]
            .iter()
            .rev()
            .find(|s| s.ontosys_commit.is_some() && s.ontosys_commit == latest.ontosys_commit);
        let Some(prev) = previous else {
            writeln!(r, "  no earlier snapshot from the same ontosys build")?;
            continue;
        };
        writeln!(
            r,
            "  functions {}",
            delta(
                total(&prev.functions_by_language),
                total(&latest.functions_by_language)
            )
        )?;
        // The owner's own inputs to the estimates, not the tool's.
        if prev.overlay_sha256 != latest.overlay_sha256 {
            writeln!(r, "  overlay changed (.ontosys/cost-catalog.ttl)")?;
        }
        if prev.claims_sha256 != latest.claims_sha256 {
            writeln!(r, "  claims changed (.ontosys/claims/complexity.ttl)")?;
        }
        if let (Some(a), Some(b)) = (&prev.cyclomatic, &latest.cyclomatic) {
            writeln!(
                r,
                "  cyclomatic median {}, p90 {}, max {}",
                delta(a.median, b.median),
                delta(a.p90, b.p90),
                delta(a.max, b.max)
            )?;
        }
        if let (Some(a), Some(b)) = (&prev.cognitive, &latest.cognitive) {
            writeln!(
                r,
                "  cognitive median {}, p90 {}, max {}",
                delta(a.median, b.median),
                delta(a.p90, b.p90),
                delta(a.max, b.max)
            )?;
        }
        for (k, b) in &latest.by_basis {
            writeln!(
                r,
                "  basis {k} {}",
                delta(prev.by_basis.get(k).copied().unwrap_or(0), *b)
            )?;
        }
        for (k, b) in &latest.by_reason {
            let a = prev.by_reason.get(k).copied().unwrap_or(0);
            if a != *b {
                writeln!(r, "  unknown by {k} {}", delta(a, *b))?;
            }
        }
        let before: BTreeMap<&str, u64> = prev
            .unlock_top10
            .iter()
            .map(|(n, c)| (n.as_str(), *c))
            .collect();
        for (n, c) in &latest.unlock_top10 {
            match before.get(n.as_str()) {
                Some(a) if a != c => writeln!(r, "  unlock {n} {}", delta(*a, *c))?,
                None => writeln!(r, "  unlock {n} new ({c})")?,
                _ => {}
            }
        }
        let old: BTreeMap<&str, u64> = prev
            .top_cyclomatic
            .iter()
            .map(|(n, c)| (n.as_str(), *c))
            .collect();
        let mut growth: Vec<(i64, &str)> = latest
            .top_cyclomatic
            .iter()
            .filter_map(|(n, c)| {
                old.get(n.as_str())
                    .map(|a| (*c as i64 - *a as i64, n.as_str()))
            })
            .filter(|(d, _)| *d > 0)
            .collect();
        growth.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        for (d, n) in growth.into_iter().take(10) {
            writeln!(r, "  cyclomatic +{d}  {n}")?;
        }
    }
    Ok(r)
}

/// Build a repository's HEAD in a temporary clone and read that, leaving
/// the owner's tree untouched.
pub fn rebuild(repo: &Path, exe: &Path, installed_version: &str) -> anyhow::Result<Snapshot> {
    use std::process::Command;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "ontosys-survey-rebuild-{}-{nanos}",
        std::process::id()
    ));
    let result = (|| -> anyhow::Result<Snapshot> {
        let ok = Command::new("git")
            .args(["clone", "--quiet", "--local", "--no-hardlinks"])
            .arg(repo)
            .arg(&tmp)
            .status()?
            .success();
        anyhow::ensure!(ok, "git clone of {} failed", repo.display());
        match std::fs::read(repo.join(".ontosys/config.json")) {
            Ok(config) => {
                std::fs::create_dir_all(tmp.join(".ontosys/data"))?;
                std::fs::write(tmp.join(".ontosys/config.json"), config)?;
            }
            Err(_) => {
                let ok = Command::new(exe)
                    .arg("init")
                    .current_dir(&tmp)
                    .status()?
                    .success();
                anyhow::ensure!(ok, "ontosys init failed in the clone");
            }
        }
        let ok = Command::new(exe)
            .arg("build")
            .current_dir(&tmp)
            .status()?
            .success();
        anyhow::ensure!(ok, "ontosys build failed in the clone");
        let mut s = snapshot(&tmp, "rebuild", installed_version);
        s.repo = repo.display().to_string();
        Ok(s)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// `ontosys survey`.
pub async fn run(
    roots: Vec<PathBuf>,
    depth: usize,
    show_report: bool,
    rebuild_repo: Option<PathBuf>,
    ledger: Option<PathBuf>,
) -> anyhow::Result<()> {
    let ledger = match ledger {
        Some(l) => l,
        None => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("no HOME; pass --ledger"))?;
            home.join(".ontosys/survey/ledger.jsonl")
        }
    };
    let version = env!("CARGO_PKG_VERSION");
    let mut snaps = Vec::new();
    if let Some(repo) = &rebuild_repo {
        snaps.push(rebuild(repo, &std::env::current_exe()?, version)?);
    }
    for repo in discover(&roots, depth) {
        snaps.push(snapshot(&repo, "folder", version));
    }
    if !snaps.is_empty() {
        append(&ledger, &snaps)?;
        for s in &snaps {
            println!(
                "{:<60} {:<8} {:<22} {:>8} functions  {}",
                s.repo,
                s.source,
                s.staleness.join(","),
                total(&s.functions_by_language),
                s.ontosys_version.as_deref().unwrap_or("-")
            );
        }
        println!(
            "{} snapshot(s) appended to {}",
            snaps.len(),
            ledger.display()
        );
    }
    if show_report {
        print!("{}", report(&ledger)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ontosys-survey-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_folder(repo: &Path, version: &str, head: &str, cyclo: &[u64]) {
        let data = repo.join(".ontosys/data");
        fs::create_dir_all(&data).unwrap();
        let meta = serde_json::json!({
            "ontosys_version": version, "ontosys_commit": "c0ffee", "timestamp": "2026-11-09T00:00:00Z",
            "git_head": head, "graph_sha256": "aa", "complexity_sha256": "bb", "inputs_sha256": "cc",
            "unparsed_by_extension": {".sh": 2},
            "complexity": {
                "tier_a": {"python": {"analysed": cyclo.len(), "attached": cyclo.len()}},
                "by_basis": {"ExactShape": 1, "Partial": 2}, "by_reason": {"UncostedCall": 2},
                "loops_by_class": {"Size": 3}, "findings_by_kind": {"SortInLoop": 1},
                "unlock": [{"name": "frob", "functions": 2}]
            }
        });
        fs::write(
            data.join("build-meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        let nt: String = cyclo
            .iter()
            .enumerate()
            .map(|(i, c)| format!("<urn:f{i}> <https://ontosys.io/ns/code#cyclomatic> \"{c}\"^^<http://www.w3.org/2001/XMLSchema#integer> .\n"))
            .collect();
        fs::write(data.join("graph.nt"), nt).unwrap();
    }

    #[test]
    fn survey_finds_folders_and_skips_vendor_dirs() {
        let root = tmp("find");
        for d in [
            "a/.ontosys",
            "b/node_modules/c/.ontosys",
            "x/target/y/.ontosys",
            "deep/1/2/3/4/5/.ontosys",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let found = discover(&[root.clone()], 4);
        assert_eq!(found, vec![root.join("a")]);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn survey_writes_nothing_into_scanned_repos() {
        let root = tmp("ro");
        let repo = root.join("r");
        fake_folder(&repo, "0.9.0", "abc", &[1, 2, 3]);
        let before: Vec<(PathBuf, Vec<u8>)> = walkdir::WalkDir::new(&root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().to_path_buf(), fs::read(e.path()).unwrap()))
            .collect();
        let s = snapshot(&repo, "folder", "0.9.0");
        let after: Vec<(PathBuf, Vec<u8>)> = walkdir::WalkDir::new(&root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().to_path_buf(), fs::read(e.path()).unwrap()))
            .collect();
        assert_eq!(before, after);
        assert_eq!(s.functions_by_language.get("python"), Some(&3));
        assert_eq!(
            s.cyclomatic,
            Some(Spread {
                median: 2,
                p90: 3,
                max: 3
            })
        );
        assert_eq!(s.unlock_top10, vec![("frob".to_string(), 2)]);
        assert_eq!(s.top_cyclomatic[0], ("urn:f2".to_string(), 3));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ledger_is_append_only_jsonl() {
        let root = tmp("ledger");
        let ledger = root.join("sub/ledger.jsonl");
        let a = Snapshot {
            repo: "a".into(),
            ..Default::default()
        };
        append(&ledger, &[a.clone()]).unwrap();
        let first = fs::read_to_string(&ledger).unwrap();
        append(
            &ledger,
            &[Snapshot {
                repo: "b".into(),
                ..Default::default()
            }],
        )
        .unwrap();
        let both = fs::read_to_string(&ledger).unwrap();
        assert!(both.starts_with(&first));
        assert_eq!(both.lines().count(), 2);
        let back: Snapshot = serde_json::from_str(both.lines().next().unwrap()).unwrap();
        assert_eq!(back, a);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn staleness_classes() {
        let root = tmp("stale");
        let old = root.join("old");
        fake_folder(&old, "0.7.0", "abc", &[1]);
        assert!(snapshot(&old, "folder", "0.9.0")
            .staleness
            .contains(&"stale-tool".to_string()));
        let none = root.join("none");
        fs::create_dir_all(none.join(".ontosys")).unwrap();
        assert_eq!(
            snapshot(&none, "folder", "0.9.0").staleness,
            vec!["no-meta".to_string()]
        );
        let cur = root.join("cur");
        fake_folder(&cur, "0.9.0", "", &[1]);
        let s = snapshot(&cur, "folder", "0.9.0");
        assert!(!s.staleness.contains(&"stale-tool".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn report_compares_same_tool_commit_only() {
        let root = tmp("report");
        let ledger = root.join("ledger.jsonl");
        let snap = |commit: &str, fns: u64, at: &str| Snapshot {
            at: at.into(),
            repo: "r".into(),
            ontosys_commit: Some(commit.into()),
            functions_by_language: BTreeMap::from([("python".to_string(), fns)]),
            cyclomatic: Some(Spread {
                median: 2,
                p90: 5,
                max: fns,
            }),
            ..Default::default()
        };
        append(
            &ledger,
            &[
                snap("c1", 10, "1"),
                snap("c2", 50, "2"),
                snap("c1", 12, "3"),
                snap("c2", 55, "4"),
            ],
        )
        .unwrap();
        let r = report(&ledger).unwrap();
        assert!(r.contains("functions 50 -> 55 (+5)"), "{r}");
        assert!(!r.contains("12 -> 55"), "never across tool commits: {r}");
        fs::remove_dir_all(&root).ok();
    }

    /// Final review I6: an overlay or claims edit is an input change, named
    /// as such, not mistaken for the tool's.
    #[test]
    fn report_names_overlay_and_claims_changes() {
        let root = tmp("report-inputs");
        let ledger = root.join("ledger.jsonl");
        let snap = |overlay: &str, claims: &str, at: &str| Snapshot {
            at: at.into(),
            repo: "r".into(),
            ontosys_commit: Some("c1".into()),
            overlay_sha256: Some(overlay.into()),
            claims_sha256: Some(claims.into()),
            ..Default::default()
        };
        append(&ledger, &[snap("o1", "k1", "1"), snap("o2", "k1", "2")]).unwrap();
        let r = report(&ledger).unwrap();
        assert!(r.contains("overlay changed"), "{r}");
        assert!(!r.contains("claims changed"), "{r}");
        let meta = serde_json::json!({"complexity": {"overlay_sha256": "aa", "claims_sha256": "bb"}});
        let dir = root.join("repo/.ontosys/data");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("build-meta.json"), meta.to_string()).unwrap();
        let s = snapshot(&root.join("repo"), "here", "0.9.0");
        assert_eq!((s.overlay_sha256.as_deref(), s.claims_sha256.as_deref()), (Some("aa"), Some("bb")));
        fs::remove_dir_all(&root).ok();
    }
}
