//! Project-wide resolution of raw references into typed relationships.
//!
//! Resolution is deliberately conservative. A name is linked to a symbol
//! only when the evidence points at one declaration:
//!
//! 1. a declaration in the same file (inference),
//! 2. otherwise a declaration in a file the source file imports (inference),
//! 3. otherwise the only declaration with that name in the project (possibility).
//!
//! Names with several unrelated candidates stay unresolved rather than being
//! guessed. Imports resolved to an exact file path are facts; C# `using`
//! directives name namespaces, so their file links are inferences.

use crate::analysis::language::{ImportContext, LanguageRegistry};
use crate::intelligence::{ExtractedReference, ReferenceKind, SymbolKind};
use crate::issues::Certainty;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Imports,
    Calls,
    References,
    DependsOn,
    Inherits,
    Implements,
    GeneratedFrom,
}

impl RelationshipKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationshipKind::Imports => "imports",
            RelationshipKind::Calls => "calls",
            RelationshipKind::References => "references",
            RelationshipKind::DependsOn => "depends_on",
            RelationshipKind::Inherits => "inherits",
            RelationshipKind::Implements => "implements",
            RelationshipKind::GeneratedFrom => "generated_from",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            RelationshipKind::Imports,
            RelationshipKind::Calls,
            RelationshipKind::References,
            RelationshipKind::DependsOn,
            RelationshipKind::Inherits,
            RelationshipKind::Implements,
            RelationshipKind::GeneratedFrom,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// A file as seen by resolution.
#[derive(Debug, Clone)]
pub struct FileNode {
    pub id: i64,
    pub path: String,
    pub language: Option<String>,
}

/// A symbol as seen by resolution.
#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub id: i64,
    pub file_id: i64,
    pub identity: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    /// Declared `partial` (C#): one of several parts of the same type.
    pub is_partial: bool,
}

/// A stored raw reference.
#[derive(Debug, Clone)]
pub struct ReferenceNode {
    pub file_id: i64,
    pub reference: ExtractedReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedRelationship {
    pub kind: RelationshipKind,
    pub source_file_id: i64,
    pub source_symbol_id: Option<i64>,
    pub target_file_id: Option<i64>,
    pub target_symbol_id: Option<i64>,
    /// What the source wrote (module specifier or name).
    pub target_name: String,
    pub certainty: Certainty,
    /// Line of the reference in the source file (first one for aggregates).
    pub line: usize,
    pub evidence: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ResolutionStats {
    pub resolved: usize,
    /// Several candidate declarations and nothing to choose between them.
    pub ambiguous: usize,
    /// No declaration in the project (library, builtin, or dynamic).
    pub unresolved: usize,
}

/// A project-relative import that names no existing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokenImport {
    pub file_id: i64,
    pub path: String,
    /// The import as written (`./auth`, `.models`, `mod config`).
    pub target: String,
    pub line: usize,
    pub column: usize,
}

/// Project-relative imports (see `LanguageAnalyzer::is_relative_import`)
/// that resolve neither to an indexed file nor to a file on disk.
pub fn find_broken_imports(
    files: &[FileNode],
    references: &[ReferenceNode],
    registry: &LanguageRegistry,
    exists_on_disk: &dyn Fn(&str) -> bool,
) -> Vec<BrokenImport> {
    let files_by_id: HashMap<i64, &FileNode> = files.iter().map(|f| (f.id, f)).collect();
    let paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    let namespaces = HashMap::new();
    let ctx = ImportContext {
        paths: &paths,
        namespaces: &namespaces,
        on_disk: Some(exists_on_disk),
    };
    let mut out = Vec::new();
    for r in references {
        let Some(file) = files_by_id.get(&r.file_id) else {
            continue;
        };
        let Some(analyzer) = file.language.as_deref().and_then(|l| registry.get(l)) else {
            continue;
        };
        if analyzer.is_relative_import(&r.reference)
            && analyzer
                .resolve_import(&r.reference, &file.path, &ctx)
                .is_empty()
        {
            out.push(BrokenImport {
                file_id: file.id,
                path: file.path.clone(),
                target: r.reference.target.clone(),
                line: r.reference.range.start_line,
                column: r.reference.range.start_column,
            });
        }
    }
    out.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    out
}

pub fn resolve_relationships(
    files: &[FileNode],
    symbols: &[SymbolNode],
    references: &[ReferenceNode],
    registry: &LanguageRegistry,
) -> (Vec<ResolvedRelationship>, ResolutionStats) {
    let files_by_id: HashMap<i64, &FileNode> = files.iter().map(|f| (f.id, f)).collect();
    let file_id_by_path: HashMap<&str, i64> =
        files.iter().map(|f| (f.path.as_str(), f.id)).collect();
    let paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();

    let mut namespaces: HashMap<String, Vec<String>> = HashMap::new();
    for s in symbols.iter().filter(|s| s.kind == SymbolKind::Namespace) {
        if let Some(f) = files_by_id.get(&s.file_id) {
            let entry = namespaces.entry(s.qualified_name.clone()).or_default();
            if !entry.contains(&f.path) {
                entry.push(f.path.clone());
            }
        }
    }
    let ctx = ImportContext {
        paths: &paths,
        namespaces: &namespaces,
        on_disk: None,
    };

    let symbol_by_identity: HashMap<(i64, &str), &SymbolNode> = symbols
        .iter()
        .map(|s| ((s.file_id, s.identity.as_str()), s))
        .collect();
    let mut symbols_by_name: HashMap<&str, Vec<&SymbolNode>> = HashMap::new();
    for s in symbols {
        symbols_by_name.entry(s.name.as_str()).or_default().push(s);
    }

    let mut out = Vec::new();
    let mut stats = ResolutionStats::default();

    // Pass 1: imports, which also tell name resolution where to look.
    let mut imported_files: HashMap<i64, HashSet<i64>> = HashMap::new();
    // `use a::{b, c}` names one file twice on one line; link it once.
    let mut seen_imports: HashSet<(i64, i64, usize)> = HashSet::new();
    for r in references.iter().filter(|r| {
        matches!(
            r.reference.kind,
            ReferenceKind::Import | ReferenceKind::ModuleDecl
        )
    }) {
        let Some(file) = files_by_id.get(&r.file_id) else {
            continue;
        };
        let Some(analyzer) = file.language.as_deref().and_then(|l| registry.get(l)) else {
            continue;
        };
        let source_symbol = r
            .reference
            .from_symbol
            .as_deref()
            .and_then(|id| symbol_by_identity.get(&(r.file_id, id)))
            .map(|s| s.id);
        let targets = analyzer.resolve_import(&r.reference, &file.path, &ctx);
        let certainty = if file.language.as_deref() == Some("csharp") {
            Certainty::Inference
        } else {
            Certainty::Fact
        };
        if targets.is_empty() {
            stats.unresolved += 1;
            out.push(ResolvedRelationship {
                kind: RelationshipKind::Imports,
                source_file_id: r.file_id,
                source_symbol_id: source_symbol,
                target_file_id: None,
                target_symbol_id: None,
                target_name: r.reference.target.clone(),
                certainty: Certainty::Fact,
                line: r.reference.range.start_line,
                evidence: format!(
                    "`{}` does not name a file in the project (external package or module).",
                    r.reference.target
                ),
            });
            continue;
        }
        stats.resolved += 1;
        for target in targets {
            let Some(&target_id) = file_id_by_path.get(target.as_str()) else {
                continue;
            };
            if target_id == r.file_id
                || !seen_imports.insert((r.file_id, target_id, r.reference.range.start_line))
            {
                continue;
            }
            imported_files
                .entry(r.file_id)
                .or_default()
                .insert(target_id);
            out.push(ResolvedRelationship {
                kind: RelationshipKind::Imports,
                source_file_id: r.file_id,
                source_symbol_id: source_symbol,
                target_file_id: Some(target_id),
                target_symbol_id: None,
                target_name: r.reference.target.clone(),
                certainty,
                line: r.reference.range.start_line,
                evidence: if certainty == Certainty::Fact {
                    format!("`{}` resolves to {}.", r.reference.target, target)
                } else {
                    format!(
                        "`using {}` names a namespace declared in {}.",
                        r.reference.target, target
                    )
                },
            });
        }
    }

    // Pass 2: names (calls, type uses, inheritance).
    for r in references.iter().filter(|r| {
        !matches!(
            r.reference.kind,
            ReferenceKind::Import | ReferenceKind::ModuleDecl
        )
    }) {
        let Some(file) = files_by_id.get(&r.file_id) else {
            continue;
        };
        let reference = &r.reference;
        let source_symbol = reference
            .from_symbol
            .as_deref()
            .and_then(|id| symbol_by_identity.get(&(r.file_id, id)).copied());

        let receiver = CallReceiver::of(reference);
        let candidates: Vec<&SymbolNode> = symbols_by_name
            .get(reference.target.as_str())
            .map(|c| {
                c.iter()
                    .copied()
                    .filter(|s| kind_accepts(reference.kind, s.kind))
                    .filter(|s| receiver != CallReceiver::Object || s.kind == SymbolKind::Method)
                    .filter(|s| same_language_family(file, files_by_id.get(&s.file_id)))
                    // A declaration does not reference itself by its own name.
                    .filter(|s| Some(s.id) != source_symbol.map(|x| x.id))
                    .collect()
            })
            .unwrap_or_default();

        let imports = imported_files.get(&r.file_id);
        let candidates = merge_partial_parts(candidates, r.file_id, imports);
        // `x.name()` could be any type's method; without local or imported
        // evidence a same-named declaration elsewhere is not enough.
        let pick = choose(
            &candidates,
            r.file_id,
            imports,
            receiver == CallReceiver::None,
        );
        let (target, certainty, why) = match pick {
            Choice::One(t, c, why) => (t, c, why),
            Choice::Ambiguous => {
                stats.ambiguous += 1;
                continue;
            }
            Choice::None => {
                stats.unresolved += 1;
                continue;
            }
        };
        stats.resolved += 1;

        let kind = match reference.kind {
            ReferenceKind::Call => RelationshipKind::Calls,
            ReferenceKind::TypeReference => RelationshipKind::References,
            ReferenceKind::Implements => RelationshipKind::Implements,
            ReferenceKind::Inherits
                if matches!(target.kind, SymbolKind::Interface | SymbolKind::Trait) =>
            {
                RelationshipKind::Implements
            }
            ReferenceKind::Inherits => RelationshipKind::Inherits,
            ReferenceKind::Import | ReferenceKind::ModuleDecl => unreachable!(),
        };
        let target_path = files_by_id
            .get(&target.file_id)
            .map(|f| f.path.as_str())
            .unwrap_or("?");
        out.push(ResolvedRelationship {
            kind,
            source_file_id: r.file_id,
            source_symbol_id: source_symbol.map(|s| s.id),
            target_file_id: Some(target.file_id),
            target_symbol_id: Some(target.id),
            target_name: reference.target.clone(),
            certainty,
            line: reference.range.start_line,
            evidence: format!(
                "`{}` at line {} {} `{}` in {}.",
                reference.target,
                reference.range.start_line,
                why,
                target.qualified_name,
                target_path
            ),
        });
    }

    out.extend(file_dependencies(&out, &files_by_id));
    (out, stats)
}

/// Parts of one partial type (same qualified name and kind in several
/// files) are one declaration, not competing candidates. Keeps the part in
/// the referencing file, else in an imported file, else the first.
fn merge_partial_parts<'a>(
    candidates: Vec<&'a SymbolNode>,
    source_file: i64,
    imports: Option<&HashSet<i64>>,
) -> Vec<&'a SymbolNode> {
    let mut by_type: Vec<(&str, SymbolKind, &'a SymbolNode)> = Vec::new();
    for c in candidates {
        if !c.is_partial {
            by_type.push((c.qualified_name.as_str(), c.kind, c));
            continue;
        }
        let preferred = |s: &SymbolNode| {
            s.file_id == source_file || imports.is_some_and(|i| i.contains(&s.file_id))
        };
        match by_type
            .iter_mut()
            .find(|(q, k, s)| s.is_partial && *q == c.qualified_name && *k == c.kind)
        {
            Some(entry) if !preferred(entry.2) && preferred(c) => entry.2 = c,
            Some(_) => {}
            None => by_type.push((c.qualified_name.as_str(), c.kind, c)),
        }
    }
    by_type.into_iter().map(|(_, _, c)| c).collect()
}

/// What a call is made on, from the qualifier recorded at extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallReceiver {
    /// `name()`
    None,
    /// `value.name()`, `self.name()`: a method of some object.
    Object,
    /// `module::name()`, `ns.name()`, `Type.name()`: could be a function.
    Path,
}

impl CallReceiver {
    fn of(r: &ExtractedReference) -> Self {
        match r.detail.first().map(String::as_str) {
            _ if r.kind != ReferenceKind::Call => CallReceiver::None,
            None | Some("") => CallReceiver::None,
            Some("." | "self" | "this" | "base" | "super") => CallReceiver::Object,
            Some(q) if q.starts_with("this.") || q.starts_with("self.") => CallReceiver::Object,
            Some(_) => CallReceiver::Path,
        }
    }
}

enum Choice<'a> {
    One(&'a SymbolNode, Certainty, &'static str),
    Ambiguous,
    None,
}

fn choose<'a>(
    candidates: &[&'a SymbolNode],
    source_file: i64,
    imports: Option<&HashSet<i64>>,
    allow_project_wide: bool,
) -> Choice<'a> {
    let single = |subset: Vec<&'a SymbolNode>| -> Option<&'a SymbolNode> {
        if subset.len() == 1 {
            Some(subset[0])
        } else {
            None
        }
    };
    let local: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|s| s.file_id == source_file)
        .collect();
    if let Some(s) = single(local.clone()) {
        return Choice::One(s, Certainty::Inference, "matches the declaration of");
    }
    if !local.is_empty() {
        return Choice::Ambiguous;
    }
    if let Some(imports) = imports {
        let imported: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|s| imports.contains(&s.file_id))
            .collect();
        if let Some(s) = single(imported.clone()) {
            return Choice::One(s, Certainty::Inference, "matches the imported declaration");
        }
        if !imported.is_empty() {
            return Choice::Ambiguous;
        }
    }
    match candidates.len() {
        0 => Choice::None,
        _ if !allow_project_wide => Choice::None,
        1 => Choice::One(
            candidates[0],
            Certainty::Possibility,
            "matches the only project declaration named",
        ),
        _ => Choice::Ambiguous,
    }
}

fn kind_accepts(reference: ReferenceKind, target: SymbolKind) -> bool {
    use SymbolKind::*;
    match reference {
        ReferenceKind::Call => matches!(
            target,
            Function | Method | Constructor | Class | Struct | Macro
        ),
        ReferenceKind::TypeReference => {
            matches!(
                target,
                Class | Interface | Struct | Enum | Trait | TypeAlias
            )
        }
        ReferenceKind::Inherits | ReferenceKind::Implements => {
            matches!(target, Class | Interface | Struct | Trait)
        }
        ReferenceKind::Import | ReferenceKind::ModuleDecl => false,
    }
}

/// Code only calls into code of the same language (TypeScript and
/// JavaScript interoperate).
fn same_language_family(source: &FileNode, target: Option<&&FileNode>) -> bool {
    target.is_some_and(|t| {
        language_family(source.language.as_deref()) == language_family(t.language.as_deref())
    })
}

fn language_family(language: Option<&str>) -> Option<&str> {
    match language {
        Some("typescript") | Some("javascript") => Some("ecmascript"),
        other => other,
    }
}

/// One `depends_on` edge per ordered pair of distinct files, summarising
/// the relationships between them.
fn file_dependencies(
    relationships: &[ResolvedRelationship],
    files: &HashMap<i64, &FileNode>,
) -> Vec<ResolvedRelationship> {
    let mut grouped: BTreeMap<(i64, i64), Vec<&ResolvedRelationship>> = BTreeMap::new();
    for r in relationships {
        if let Some(target) = r.target_file_id {
            if target != r.source_file_id {
                grouped
                    .entry((r.source_file_id, target))
                    .or_default()
                    .push(r);
            }
        }
    }
    grouped
        .into_iter()
        .map(|((source, target), rels)| {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for r in &rels {
                *counts.entry(r.kind.as_str()).or_default() += 1;
            }
            let summary = counts
                .iter()
                .map(|(k, n)| format!("{n} {k}"))
                .collect::<Vec<_>>()
                .join(", ");
            let certainty = rels
                .iter()
                .map(|r| r.certainty)
                .min_by_key(|c| certainty_rank(*c))
                .unwrap_or(Certainty::Possibility);
            let target_path = files
                .get(&target)
                .map(|f| f.path.clone())
                .unwrap_or_default();
            ResolvedRelationship {
                kind: RelationshipKind::DependsOn,
                source_file_id: source,
                source_symbol_id: None,
                target_file_id: Some(target),
                target_symbol_id: None,
                target_name: target_path,
                certainty,
                line: rels.iter().map(|r| r.line).min().unwrap_or(0),
                evidence: summary,
            }
        })
        .collect()
}

/// Lower is stronger.
pub fn certainty_rank(c: Certainty) -> u8 {
    match c {
        Certainty::Fact => 0,
        Certainty::Inference => 1,
        Certainty::Possibility => 2,
    }
}
