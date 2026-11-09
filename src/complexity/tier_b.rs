//! # Tier B
//!
//! Relationships between functions: fan-in, fan-out, external callees, call
//! cycles (strongly connected components of size > 1) and loop depth
//! through repository calls. Linear in edges.

use super::model::{Model, Target};
use crate::ontology::Iri;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TierB {
    pub fan_in: usize,
    pub fan_out: usize,
    pub external_calls: usize,
    /// Index into the cycles returned beside these facts.
    pub cycle: Option<usize>,
    pub transitive_loop_depth: Option<u32>,
}

/// Tier B for every function in the model, and the call cycles (members as
/// model indices, sorted by IRI; cycles sorted by their first member's IRI).
pub fn tier_b(m: &Model) -> (Vec<TierB>, Vec<Vec<usize>>) {
    let n = m.fns.len();
    let mut out = vec![TierB::default(); n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut self_call = vec![false; n];
    let mut indirect = vec![false; n];
    for (i, f) in m.fns.iter().enumerate() {
        let mut callees = BTreeSet::new();
        let mut external = BTreeSet::new();
        for d in &f.defs {
            for (site, t) in d.tier_a.calls.iter().zip(&d.targets) {
                match t {
                    Target::Repo(j) if *j == i => self_call[i] = true,
                    Target::Repo(j) => {
                        callees.insert(*j);
                    }
                    Target::Indirect => {
                        indirect[i] = true;
                        external.insert(site.callee.as_str());
                    }
                    _ => {
                        external.insert(site.callee.as_str());
                    }
                }
            }
        }
        out[i].fan_out = callees.len();
        out[i].external_calls = external.len();
        succ[i] = callees.into_iter().collect();
    }
    for i in 0..n {
        for &j in &succ[i] {
            out[j].fan_in += 1;
        }
    }

    // Tarjan, iteratively: components come out callees-first.
    let components = tarjan(&succ);
    let mut cycles: Vec<Vec<usize>> = components
        .iter()
        .filter(|c| c.len() > 1)
        .map(|c| {
            let mut c = c.clone();
            c.sort_by(|a, b| m.fns[*a].iri.as_str().cmp(m.fns[*b].iri.as_str()));
            c
        })
        .collect();
    cycles.sort_by(|a, b| m.fns[a[0]].iri.as_str().cmp(m.fns[b[0]].iri.as_str()));
    for (k, c) in cycles.iter().enumerate() {
        for &i in c {
            out[i].cycle = Some(k);
        }
    }

    for comp in &components {
        if comp.len() > 1 {
            continue; // absent for every member of a cycle
        }
        let i = comp[0];
        if self_call[i] || indirect[i] {
            continue;
        }
        let mut depth = 0u32;
        let mut known = true;
        'defs: for d in &m.fns[i].defs {
            depth = depth.max(d.tier_a.max_loop_nesting);
            for (site, t) in d.tier_a.calls.iter().zip(&d.targets) {
                let k = site.loop_path.len() as u32;
                let callee = match t {
                    Target::Repo(j) => match out[*j].transitive_loop_depth {
                        Some(x) => x,
                        None => {
                            known = false;
                            break 'defs;
                        }
                    },
                    _ => 0,
                };
                depth = depth.max(k + callee);
            }
        }
        out[i].transitive_loop_depth = known.then_some(depth);
    }
    (out, cycles)
}

/// Strongly connected components, callees before callers, without recursion.
fn tarjan(succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = succ.len();
    let (mut index, mut low) = (vec![usize::MAX; n], vec![0usize; n]);
    let mut on_stack = vec![false; n];
    let (mut stack, mut comps, mut next) = (Vec::new(), Vec::new(), 0usize);
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&mut (v, ref mut pos)) = work.last_mut() {
            if *pos < succ[v].len() {
                let w = succ[v][*pos];
                *pos += 1;
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    comps.push(comp);
                }
            }
        }
    }
    comps
}

/// `<project>/cycle/<first 32 hex of sha256 over the sorted member IRIs>`.
pub fn cycle_iri(project: &str, m: &Model, members: &[usize]) -> Iri {
    use sha2::{Digest, Sha256};
    let mut iris: Vec<&str> = members.iter().map(|&i| m.fns[i].iri.as_str()).collect();
    iris.sort_unstable();
    let digest: String = Sha256::digest(iris.join("\n").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    crate::ontology::instance_iri(project, "cycle", &digest[..32])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::complexity::engine::{CallKind, CallSite, TierA};
    use crate::complexity::model::{DefRec, Target};
    use crate::complexity::Grammar;

    pub(crate) fn site(callee: &str, depth: u32) -> CallSite {
        CallSite {
            kind: CallKind::Direct,
            callee: callee.into(),
            receiver: None,
            args: vec![],
            line: 1,
            start_byte: 0,
            args_range: (0, 0),
            loop_path: (0..depth).collect(),
            conditional: false,
            in_condition_of: None,
        }
    }

    /// A model of named functions; each call is (callee name, loop depth,
    /// target); `loops` is the function's own loop nesting.
    pub(crate) fn model(fns: &[(&str, u32, Vec<(&str, u32, Target)>)]) -> Model {
        let mut m = Model::default();
        for (name, loops, calls) in fns {
            let tier_a = TierA {
                max_loop_nesting: *loops,
                calls: calls.iter().map(|(c, d, _)| site(c, *d)).collect(),
                ..Default::default()
            };
            let targets = calls.iter().map(|(_, _, t)| t.clone()).collect();
            m.add_def(
                Iri::new(format!("urn:t:{name}")),
                name,
                "f.c",
                Grammar::C,
                DefRec {
                    tier_a,
                    definition_sha256: None,
                    targets,
                },
            );
        }
        m
    }

    #[test]
    fn fan_in_and_out_count_distinct_repo_callers_and_callees() {
        use Target::*;
        let m = model(&[
            (
                "a",
                0,
                vec![
                    ("b", 0, Repo(1)),
                    ("b", 1, Repo(1)),
                    ("c", 0, Repo(2)),
                    ("printf", 0, External("printf".into())),
                    ("a", 0, Repo(0)),
                ],
            ),
            ("b", 0, vec![("c", 0, Repo(2)), ("fp", 0, Indirect)]),
            ("c", 0, vec![]),
        ]);
        let (tb, cycles) = tier_b(&m);
        assert_eq!(
            (tb[0].fan_out, tb[0].fan_in, tb[0].external_calls),
            (2, 0, 1),
            "self excluded from fan-out"
        );
        assert_eq!((tb[2].fan_in, tb[2].fan_out), (2, 0));
        assert_eq!(tb[1].external_calls, 1);
        assert!(cycles.is_empty());
    }

    #[test]
    fn mutual_recursion_forms_one_cycle_with_stable_iri() {
        use Target::*;
        let fwd = model(&[
            ("x", 0, vec![("y", 0, Repo(1))]),
            ("y", 0, vec![("x", 0, Repo(0))]),
            ("z", 0, vec![("x", 0, Repo(0))]),
        ]);
        let rev = model(&[
            ("z", 0, vec![("x", 0, Repo(2))]),
            ("y", 0, vec![("x", 0, Repo(2))]),
            ("x", 0, vec![("y", 0, Repo(1))]),
        ]);
        let (tb, cycles) = tier_b(&fwd);
        assert_eq!(cycles, vec![vec![0, 1]]);
        assert_eq!(
            (tb[0].cycle, tb[1].cycle, tb[2].cycle),
            (Some(0), Some(0), None)
        );
        let (_, cycles_rev) = tier_b(&rev);
        assert_eq!(
            cycle_iri("p", &fwd, &cycles[0]),
            cycle_iri("p", &rev, &cycles_rev[0])
        );
        assert!(cycle_iri("p", &fwd, &cycles[0])
            .as_str()
            .contains("/cycle/"));
    }

    #[test]
    fn transitive_loop_depth_adds_callee_depth() {
        use Target::*;
        let m = model(&[
            (
                "a",
                1,
                vec![("b", 2, Repo(1)), ("strlen", 3, External("strlen".into()))],
            ),
            ("b", 1, vec![]),
        ]);
        let (tb, _) = tier_b(&m);
        assert_eq!(tb[1].transitive_loop_depth, Some(1));
        assert_eq!(
            tb[0].transitive_loop_depth,
            Some(3),
            "max(own 1, 2 + 1, 3 + 0 for an external)"
        );
    }

    #[test]
    fn transitive_loop_depth_is_absent_through_a_cycle_recursion_or_indirect_call() {
        use Target::*;
        let m = model(&[
            ("x", 0, vec![("y", 0, Repo(1))]),
            ("y", 0, vec![("x", 0, Repo(0))]),
            ("caller", 0, vec![("x", 0, Repo(0))]),
            ("rec", 0, vec![("rec", 0, Repo(3))]),
            ("ptr", 0, vec![("fp", 0, Indirect)]),
        ]);
        let (tb, _) = tier_b(&m);
        for i in 0..5 {
            assert_eq!(tb[i].transitive_loop_depth, None, "function {i}");
        }
    }

    #[test]
    fn tier_b_is_linear() {
        let n = 50_000;
        let mut m = Model::default();
        for i in 0..n {
            let calls: Vec<usize> = (1..=4).map(|k| (i + k) % n).filter(|&j| j > i).collect();
            let tier_a = TierA {
                calls: calls.iter().map(|_| site("f", 1)).collect(),
                ..Default::default()
            };
            let targets = calls.iter().map(|&j| Target::Repo(j)).collect();
            m.add_def(
                Iri::new(format!("urn:t:{i}")),
                &format!("f{i}"),
                "f.c",
                Grammar::C,
                DefRec {
                    tier_a,
                    definition_sha256: None,
                    targets,
                },
            );
        }
        let t = std::time::Instant::now();
        let (tb, _) = tier_b(&m);
        assert!(t.elapsed().as_secs_f64() < 1.0, "{:?}", t.elapsed());
        assert_eq!(tb[n - 1].transitive_loop_depth, Some(0));
        assert!(
            tb[0].transitive_loop_depth.unwrap() > 10_000,
            "a long chain"
        );
    }
}
