//! Tracing a symbol through versions of a file.
//!
//! Line numbers do not identify code across history, so a symbol is found in
//! each version by its structural identity (kind + qualified name), and a
//! version counts as a change of the symbol only when the symbol's own text
//! differs.

use crate::analysis::language::LanguageRegistry;
use crate::intelligence::{extract_symbols, ExtractedSymbol, SymbolChangeKind};
use serde::Serialize;
use std::path::Path;

/// How a symbol changed in one version relative to the previous one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolRevision {
    /// Index into the versions passed to [`trace_symbol`].
    pub version: usize,
    pub change: SymbolChangeKind,
    /// The symbol in this version (absent when removed).
    pub symbol: Option<ExtractedSymbol>,
}

/// Finds a symbol by identity (`method:Svc.run`), qualified name
/// (`Svc.run`) or plain name (`run`), in that order of preference.
pub fn find_symbol<'a>(symbols: &'a [ExtractedSymbol], query: &str) -> Option<&'a ExtractedSymbol> {
    symbols
        .iter()
        .find(|s| s.identity == query)
        .or_else(|| symbols.iter().find(|s| s.qualified_name == query))
        .or_else(|| symbols.iter().find(|s| s.name == query))
}

/// Parses a version and extracts its symbols; `None` when the language is
/// unsupported or the content is missing.
pub fn symbols_of(
    registry: &LanguageRegistry,
    language: &str,
    path: &str,
    content: Option<&str>,
) -> Option<Vec<ExtractedSymbol>> {
    let content = content?;
    let lang = registry.get(language)?;
    let parsed = registry
        .parse(language, Path::new(path), content.to_string())
        .ok()?;
    Some(extract_symbols(&parsed, lang))
}

/// Versions are ordered oldest to newest, each as (path, content). Returns
/// one entry per version in which the symbol was added, modified or removed.
pub fn trace_symbol(
    registry: &LanguageRegistry,
    language: &str,
    versions: &[(String, Option<String>)],
    query: &str,
) -> Vec<SymbolRevision> {
    let mut out = Vec::new();
    let mut previous: Option<ExtractedSymbol> = None;
    // Once found, follow the same identity so a later same-named symbol
    // elsewhere in the file is not mistaken for it.
    let mut identity: Option<String> = None;
    for (i, (path, content)) in versions.iter().enumerate() {
        let Some(symbols) = symbols_of(registry, language, path, content.as_deref()) else {
            if previous.take().is_some() {
                out.push(SymbolRevision {
                    version: i,
                    change: SymbolChangeKind::Removed,
                    symbol: None,
                });
            }
            continue;
        };
        let current = find_symbol(&symbols, identity.as_deref().unwrap_or(query)).cloned();
        match (&previous, &current) {
            (None, Some(c)) => {
                identity = Some(c.identity.clone());
                out.push(SymbolRevision {
                    version: i,
                    change: SymbolChangeKind::Added,
                    symbol: current.clone(),
                });
            }
            (Some(p), Some(c)) if p.body_hash != c.body_hash => out.push(SymbolRevision {
                version: i,
                change: SymbolChangeKind::Modified,
                symbol: current.clone(),
            }),
            (Some(_), None) => out.push(SymbolRevision {
                version: i,
                change: SymbolChangeKind::Removed,
                symbol: None,
            }),
            _ => {}
        }
        previous = current;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_ignores_moves_and_other_symbols() {
        let registry = LanguageRegistry::default();
        let v = |s: &str| ("a.py".to_string(), Some(s.to_string()));
        let versions = vec![
            v("def other():\n    pass\n"),
            v("def auth():\n    return 1\n"),
            // Only moved down and another function edited: not a change of auth.
            v("def other():\n    return 2\n\ndef auth():\n    return 1\n"),
            v("def other():\n    return 2\n\ndef auth():\n    return 2\n"),
            v("def other():\n    return 2\n"),
        ];
        let got: Vec<(usize, SymbolChangeKind)> =
            trace_symbol(&registry, "python", &versions, "auth")
                .into_iter()
                .map(|r| (r.version, r.change))
                .collect();
        assert_eq!(
            got,
            [
                (1, SymbolChangeKind::Added),
                (3, SymbolChangeKind::Modified),
                (4, SymbolChangeKind::Removed)
            ]
        );
    }
}
