//! Explicit, countable dependency metrics. No weighting or scoring: each
//! number is a count of distinct files, so it can be checked by hand.

use crate::intelligence::{RelationshipKind, ResolvedRelationship};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDegree {
    pub file_id: i64,
    /// Distinct project files this file depends on (out-degree).
    pub dependencies: usize,
    /// Distinct project files that depend on this file (in-degree).
    pub dependents: usize,
}

impl FileDegree {
    pub fn total(&self) -> usize {
        self.dependencies + self.dependents
    }
}

/// Degrees over `depends_on` edges, for every file appearing in one.
pub fn file_degrees(relationships: &[ResolvedRelationship]) -> Vec<FileDegree> {
    let mut out_edges: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    let mut in_edges: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    for r in relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::DependsOn)
    {
        if let Some(target) = r.target_file_id {
            out_edges
                .entry(r.source_file_id)
                .or_default()
                .insert(target);
            in_edges.entry(target).or_default().insert(r.source_file_id);
        }
    }
    let files: BTreeSet<i64> = out_edges.keys().chain(in_edges.keys()).copied().collect();
    files
        .into_iter()
        .map(|id| FileDegree {
            file_id: id,
            dependencies: out_edges.get(&id).map_or(0, |s| s.len()),
            dependents: in_edges.get(&id).map_or(0, |s| s.len()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::Certainty;

    fn dep(source: i64, target: i64) -> ResolvedRelationship {
        ResolvedRelationship {
            kind: RelationshipKind::DependsOn,
            source_file_id: source,
            source_symbol_id: None,
            target_file_id: Some(target),
            target_symbol_id: None,
            target_name: String::new(),
            certainty: Certainty::Fact,
            line: 1,
            evidence: String::new(),
        }
    }

    #[test]
    fn test_degrees_count_distinct_files() {
        let rels = vec![dep(1, 2), dep(1, 3), dep(3, 2), dep(1, 2)];
        let d = file_degrees(&rels);
        let get = |id| d.iter().find(|x| x.file_id == id).unwrap().clone();
        assert_eq!((get(1).dependencies, get(1).dependents), (2, 0));
        assert_eq!((get(2).dependencies, get(2).dependents), (0, 2));
        assert_eq!(get(3).total(), 2);
    }
}

/// Groups of files that import each other in a cycle (strongly connected
/// components with more than one file), each sorted, largest first.
pub fn import_cycles(edges: &[(i64, i64)]) -> Vec<Vec<i64>> {
    use std::collections::{BTreeSet, HashMap};
    let mut graph: HashMap<i64, BTreeSet<i64>> = HashMap::new();
    for &(a, b) in edges {
        if a != b {
            graph.entry(a).or_default().insert(b);
            graph.entry(b).or_default();
        }
    }
    let mut nodes: Vec<i64> = graph.keys().copied().collect();
    nodes.sort_unstable();

    // Iterative Tarjan: no recursion depth limit on long import chains.
    let mut index: HashMap<i64, usize> = HashMap::new();
    let mut low: HashMap<i64, usize> = HashMap::new();
    let mut on_stack: HashMap<i64, bool> = HashMap::new();
    let mut stack: Vec<i64> = Vec::new();
    let mut next = 0;
    let mut out = Vec::new();
    for &start in &nodes {
        if index.contains_key(&start) {
            continue;
        }
        let mut work: Vec<(i64, Vec<i64>, usize)> = Vec::new();
        let succ = |n: i64| graph[&n].iter().copied().collect::<Vec<_>>();
        index.insert(start, next);
        low.insert(start, next);
        next += 1;
        stack.push(start);
        on_stack.insert(start, true);
        work.push((start, succ(start), 0));
        while let Some((node, children, i)) = work.last_mut() {
            if *i < children.len() {
                let child = children[*i];
                *i += 1;
                let node = *node;
                if !index.contains_key(&child) {
                    index.insert(child, next);
                    low.insert(child, next);
                    next += 1;
                    stack.push(child);
                    on_stack.insert(child, true);
                    work.push((child, succ(child), 0));
                } else if on_stack.get(&child).copied().unwrap_or(false) {
                    let l = low[&node].min(index[&child]);
                    low.insert(node, l);
                }
                continue;
            }
            let node = *node;
            work.pop();
            if let Some((parent, _, _)) = work.last() {
                let l = low[parent].min(low[&node]);
                low.insert(*parent, l);
            }
            if low[&node] == index[&node] {
                let mut component = Vec::new();
                while let Some(n) = stack.pop() {
                    on_stack.insert(n, false);
                    component.push(n);
                    if n == node {
                        break;
                    }
                }
                if component.len() > 1 {
                    component.sort_unstable();
                    out.push(component);
                }
            }
        }
    }
    out.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    out
}

#[cfg(test)]
mod cycle_tests {
    use super::import_cycles;

    #[test]
    fn test_cycles_are_found_and_chains_are_not() {
        let edges = [
            (1, 2),
            (2, 3),
            (3, 1),
            (3, 4),
            (4, 5),
            (6, 7),
            (7, 6),
            (8, 8),
        ];
        assert_eq!(import_cycles(&edges), vec![vec![1, 2, 3], vec![6, 7]]);
        assert!(import_cycles(&[(1, 2), (2, 3)]).is_empty());
    }

    #[test]
    fn test_long_chain_does_not_overflow() {
        let edges: Vec<(i64, i64)> = (0..100_000).map(|i| (i, i + 1)).collect();
        assert!(import_cycles(&edges).is_empty());
    }
}
