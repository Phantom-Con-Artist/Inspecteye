//! Impact analysis: which files may be affected by a change.
//!
//! The answer is a set of *dependents*, each with the chain of relationships
//! that connects it to the change. Being a dependent is evidence of
//! exposure, not of breakage: a caller of a modified function may be fine.
//! Nothing here claims that a file is broken.

use crate::intelligence::{certainty_rank, RelationshipKind, ResolvedRelationship};
use crate::issues::Certainty;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// Where a change starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactSeed {
    pub file_id: i64,
    /// Changed symbols; `None` means the whole file is considered changed.
    pub symbol_ids: Option<Vec<i64>>,
    /// Symbols that no longer exist; references to them are called out.
    pub removed_symbol_ids: Vec<i64>,
}

/// One relationship in the chain from an affected file to the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImpactStep {
    pub kind: RelationshipKind,
    /// The file that depends (source of the relationship).
    pub dependent_file_id: i64,
    /// The file it depends on.
    pub dependency_file_id: i64,
    pub target_symbol_id: Option<i64>,
    pub line: usize,
    pub certainty: Certainty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AffectedFile {
    pub file_id: i64,
    /// 1 = depends directly on a changed file/symbol.
    pub depth: usize,
    /// Direct relationships to the file one level closer to the change
    /// (all of them, not just the first found).
    pub via: Vec<ImpactStep>,
    /// Weakest certainty along the path: a chain is only as strong as its
    /// weakest link.
    pub certainty: Certainty,
    /// True when it references a symbol that was removed.
    pub references_removed_symbol: bool,
}

pub fn compute_impact(
    seeds: &[ImpactSeed],
    relationships: &[ResolvedRelationship],
    max_depth: usize,
) -> Vec<AffectedFile> {
    // Incoming edges by target file, excluding summaries and self-edges.
    let mut incoming: HashMap<i64, Vec<&ResolvedRelationship>> = HashMap::new();
    for r in relationships {
        let Some(target) = r.target_file_id else {
            continue;
        };
        if target == r.source_file_id || r.kind == RelationshipKind::DependsOn {
            continue;
        }
        incoming.entry(target).or_default().push(r);
    }

    let seed_files: HashSet<i64> = seeds.iter().map(|s| s.file_id).collect();
    let mut affected: BTreeMap<i64, AffectedFile> = BTreeMap::new();
    let mut queue: VecDeque<(i64, usize, Certainty)> = VecDeque::new();

    // Depth 1: relationships into the changed files (or changed symbols).
    for seed in seeds {
        let changed: Option<HashSet<i64>> = seed.symbol_ids.as_ref().map(|ids| {
            ids.iter()
                .chain(&seed.removed_symbol_ids)
                .copied()
                .collect()
        });
        let removed: HashSet<i64> = seed.removed_symbol_ids.iter().copied().collect();
        for r in incoming.get(&seed.file_id).into_iter().flatten() {
            if seed_files.contains(&r.source_file_id) {
                continue;
            }
            let relevant = match (&changed, r.target_symbol_id) {
                (None, _) => true,
                (Some(set), Some(sym)) => set.contains(&sym),
                // A symbol-level change does not reach file-level links such
                // as plain imports, except build outputs of the file.
                (Some(_), None) => r.kind == RelationshipKind::GeneratedFrom,
            };
            if !relevant {
                continue;
            }
            let hits_removed = r.target_symbol_id.is_some_and(|s| removed.contains(&s));
            record(&mut affected, &mut queue, r, 1, None, hits_removed);
        }
    }

    // Deeper levels: anything depending on an affected file.
    while let Some((file, depth, via_certainty)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for r in incoming.get(&file).into_iter().flatten() {
            if seed_files.contains(&r.source_file_id) {
                continue;
            }
            record(
                &mut affected,
                &mut queue,
                r,
                depth + 1,
                Some(via_certainty),
                false,
            );
        }
    }

    let mut out: Vec<AffectedFile> = affected.into_values().collect();
    out.sort_by_key(|a| (a.depth, certainty_rank(a.certainty), a.file_id));
    out
}

fn record(
    affected: &mut BTreeMap<i64, AffectedFile>,
    queue: &mut VecDeque<(i64, usize, Certainty)>,
    r: &ResolvedRelationship,
    depth: usize,
    upstream: Option<Certainty>,
    hits_removed: bool,
) {
    let step = ImpactStep {
        kind: r.kind,
        dependent_file_id: r.source_file_id,
        dependency_file_id: r.target_file_id.unwrap_or_default(),
        target_symbol_id: r.target_symbol_id,
        line: r.line,
        certainty: r.certainty,
    };
    let certainty = match upstream {
        Some(u) => weaker(u, r.certainty),
        None => r.certainty,
    };
    match affected.get_mut(&r.source_file_id) {
        // Already reached at a shallower depth: that path is the explanation.
        Some(existing) if existing.depth < depth => {}
        Some(existing) => {
            existing.via.push(step);
            existing.references_removed_symbol |= hits_removed;
            if certainty_rank(certainty) < certainty_rank(existing.certainty) {
                existing.certainty = certainty;
            }
        }
        None => {
            affected.insert(
                r.source_file_id,
                AffectedFile {
                    file_id: r.source_file_id,
                    depth,
                    via: vec![step],
                    certainty,
                    references_removed_symbol: hits_removed,
                },
            );
            queue.push_back((r.source_file_id, depth, certainty));
        }
    }
}

fn weaker(a: Certainty, b: Certainty) -> Certainty {
    if certainty_rank(a) >= certainty_rank(b) {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(
        kind: RelationshipKind,
        from: i64,
        to: i64,
        sym: Option<i64>,
        c: Certainty,
    ) -> ResolvedRelationship {
        ResolvedRelationship {
            kind,
            source_file_id: from,
            source_symbol_id: None,
            target_file_id: Some(to),
            target_symbol_id: sym,
            target_name: String::new(),
            certainty: c,
            line: 1,
            evidence: String::new(),
        }
    }

    fn whole(file: i64) -> ImpactSeed {
        ImpactSeed {
            file_id: file,
            symbol_ids: None,
            removed_symbol_ids: vec![],
        }
    }

    #[test]
    fn test_direct_and_transitive_dependents_with_weakest_link() {
        use Certainty::*;
        let rels = vec![
            rel(RelationshipKind::Imports, 2, 1, None, Fact),
            rel(RelationshipKind::Calls, 3, 2, Some(20), Possibility),
            rel(RelationshipKind::Imports, 4, 3, None, Fact),
        ];
        let got = compute_impact(&[whole(1)], &rels, 5);
        let summary: Vec<(i64, usize, Certainty)> = got
            .iter()
            .map(|a| (a.file_id, a.depth, a.certainty))
            .collect();
        assert_eq!(
            summary,
            [(2, 1, Fact), (3, 2, Possibility), (4, 3, Possibility)]
        );
    }

    #[test]
    fn test_depth_limit_and_cycles() {
        use Certainty::*;
        let rels = vec![
            rel(RelationshipKind::Imports, 2, 1, None, Fact),
            rel(RelationshipKind::Imports, 3, 2, None, Fact),
            rel(RelationshipKind::Imports, 2, 3, None, Fact),
        ];
        let got = compute_impact(&[whole(1)], &rels, 1);
        assert_eq!(got.iter().map(|a| a.file_id).collect::<Vec<_>>(), [2]);
        let got = compute_impact(&[whole(1)], &rels, 10);
        assert_eq!(got.len(), 2, "cycle does not loop forever");
    }

    #[test]
    fn test_symbol_seeds_only_reach_users_of_those_symbols() {
        use Certainty::*;
        let rels = vec![
            rel(RelationshipKind::Calls, 2, 1, Some(10), Inference),
            rel(RelationshipKind::Calls, 3, 1, Some(11), Inference),
            rel(RelationshipKind::Imports, 4, 1, None, Fact),
            rel(RelationshipKind::GeneratedFrom, 5, 1, None, Fact),
        ];
        let seed = ImpactSeed {
            file_id: 1,
            symbol_ids: Some(vec![]),
            removed_symbol_ids: vec![10],
        };
        let got = compute_impact(&[seed], &rels, 1);
        let ids: Vec<i64> = got.iter().map(|a| a.file_id).collect();
        assert_eq!(ids, [5, 2]);
        assert!(
            got.iter()
                .find(|a| a.file_id == 2)
                .unwrap()
                .references_removed_symbol
        );
    }
}
