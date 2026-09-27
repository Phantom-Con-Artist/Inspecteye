//! Partial type intelligence (C# `partial class/struct/interface/record`).
//!
//! A partial type is one logical type whose declaration is split across
//! files. Grouping the parts by namespace-qualified name and kind lets
//! size, members, findings, changes and dependencies be reported for the
//! type as a whole instead of per file.

use crate::intelligence::{RelationshipKind, ResolvedRelationship, SymbolKind};
use crate::issues::Severity;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// A symbol as needed for grouping.
#[derive(Debug, Clone)]
pub struct PartialSymbolInput {
    pub id: i64,
    pub file_id: i64,
    pub path: String,
    pub kind: SymbolKind,
    pub qualified_name: String,
    pub is_partial: bool,
    pub start_line: usize,
    pub end_line: usize,
}

/// A diagnostic, attributed by enclosing symbol.
#[derive(Debug, Clone)]
pub struct PartialDiagnosticInput {
    pub path: String,
    pub symbol: Option<String>,
    pub severity: Severity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartialPart {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub members: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartialTypeGroup {
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub parts: Vec<PartialPart>,
    /// Sum of the parts' declaration lines.
    pub lines: usize,
    pub members: usize,
    pub errors: usize,
    pub warnings: usize,
    /// Recorded changes of the type or its members (symbol history).
    pub changes: usize,
    /// Distinct files outside the group's own files that its members depend on.
    pub dependencies: usize,
    /// Distinct files outside the group's own files that depend on its members.
    pub dependents: usize,
}

pub fn partial_type_groups(
    symbols: &[PartialSymbolInput],
    diagnostics: &[PartialDiagnosticInput],
    changed_symbols: &[(String, String)],
    relationships: &[ResolvedRelationship],
) -> Vec<PartialTypeGroup> {
    let mut groups: BTreeMap<(String, &'static str), Vec<&PartialSymbolInput>> = BTreeMap::new();
    for s in symbols.iter().filter(|s| {
        s.is_partial
            && matches!(
                s.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface
            )
    }) {
        groups
            .entry((s.qualified_name.clone(), s.kind.as_str()))
            .or_default()
            .push(s);
    }

    let mut out = Vec::new();
    for ((name, _), parts) in groups {
        let prefix = format!("{name}.");
        let belongs = |path: &str, qualified: &str| {
            (qualified == name || qualified.starts_with(&prefix))
                && parts.iter().any(|p| p.path == path)
        };
        // Members are symbols nested inside one of the parts.
        let member_ids: BTreeSet<i64> = symbols
            .iter()
            .filter(|s| s.qualified_name.starts_with(&prefix))
            .filter(|s| {
                parts.iter().any(|p| {
                    p.file_id == s.file_id
                        && p.start_line <= s.start_line
                        && s.end_line <= p.end_line
                })
            })
            .map(|s| s.id)
            .collect();
        let group_symbol_ids: BTreeSet<i64> = member_ids
            .iter()
            .copied()
            .chain(parts.iter().map(|p| p.id))
            .collect();
        let group_files: BTreeSet<i64> = parts.iter().map(|p| p.file_id).collect();

        let part_list: Vec<PartialPart> = parts
            .iter()
            .map(|p| PartialPart {
                path: p.path.clone(),
                start_line: p.start_line,
                end_line: p.end_line,
                members: symbols
                    .iter()
                    .filter(|s| member_ids.contains(&s.id) && s.file_id == p.file_id)
                    .count(),
            })
            .collect();

        let in_group = |d: &&PartialDiagnosticInput| {
            d.symbol.as_deref().is_some_and(|sym| belongs(&d.path, sym))
        };
        let errors = diagnostics
            .iter()
            .filter(in_group)
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diagnostics
            .iter()
            .filter(in_group)
            .filter(|d| d.severity == Severity::Warning)
            .count();
        let changes = changed_symbols
            .iter()
            .filter(|(path, sym)| belongs(path, sym))
            .count();

        let mut dependencies: BTreeSet<i64> = BTreeSet::new();
        let mut dependents: BTreeSet<i64> = BTreeSet::new();
        for r in relationships
            .iter()
            .filter(|r| r.kind != RelationshipKind::DependsOn)
        {
            let from_group = r
                .source_symbol_id
                .is_some_and(|id| group_symbol_ids.contains(&id));
            let to_group = r
                .target_symbol_id
                .is_some_and(|id| group_symbol_ids.contains(&id));
            if let (true, Some(t)) = (from_group, r.target_file_id) {
                if !group_files.contains(&t) {
                    dependencies.insert(t);
                }
            }
            if to_group && !group_files.contains(&r.source_file_id) {
                dependents.insert(r.source_file_id);
            }
        }

        out.push(PartialTypeGroup {
            qualified_name: name.clone(),
            kind: parts[0].kind,
            lines: part_list
                .iter()
                .map(|p| p.end_line + 1 - p.start_line)
                .sum(),
            members: member_ids.len(),
            parts: part_list,
            errors,
            warnings,
            changes,
            dependencies: dependencies.len(),
            dependents: dependents.len(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(
        id: i64,
        file: i64,
        kind: SymbolKind,
        name: &str,
        partial: bool,
        lines: (usize, usize),
    ) -> PartialSymbolInput {
        PartialSymbolInput {
            id,
            file_id: file,
            path: format!("f{file}.cs"),
            kind,
            qualified_name: name.to_string(),
            is_partial: partial,
            start_line: lines.0,
            end_line: lines.1,
        }
    }

    #[test]
    fn test_parts_group_by_qualified_name_and_aggregate() {
        let symbols = vec![
            sym(1, 1, SymbolKind::Class, "N.Order", true, (1, 10)),
            sym(2, 1, SymbolKind::Method, "N.Order.Place", false, (2, 4)),
            sym(3, 2, SymbolKind::Class, "N.Order", true, (1, 20)),
            sym(4, 2, SymbolKind::Method, "N.Order.Cancel", false, (3, 9)),
            sym(5, 2, SymbolKind::Class, "N.Other", false, (21, 30)),
        ];
        let diags = vec![PartialDiagnosticInput {
            path: "f2.cs".into(),
            symbol: Some("N.Order.Cancel".into()),
            severity: Severity::Warning,
        }];
        let changes = vec![("f1.cs".to_string(), "N.Order.Place".to_string())];
        let groups = partial_type_groups(&symbols, &diags, &changes, &[]);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.qualified_name, "N.Order");
        assert_eq!(g.parts.len(), 2);
        assert_eq!(g.lines, 30);
        assert_eq!(g.members, 2);
        assert_eq!((g.errors, g.warnings, g.changes), (0, 1, 1));
    }
}
