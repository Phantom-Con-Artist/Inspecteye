//! Read models behind the `inspect`, `check` and `tree` command families.
//!
//! `inspect` and `tree` read the index only. `check` builds a uniform
//! report of findings; the CLI refreshes the analysis before running it.

use super::manager::{check_external_references, selects_any, to_filters, ProjectManager};
use crate::analysis::language::LanguageRegistry;
use crate::database::{
    StoredDiagnostic, StoredExternalReference, StoredRelationship, StoredSymbol, StoredSymbolChange,
};
use crate::error::{InspectyeError, Result};
use crate::git::GitProvider;
use crate::intelligence::{
    build_tree, find_broken_imports, import_cycles, Existence, RelationshipKind, TreeFile, TreeNode,
};
use crate::issues::{Certainty, DiagnosticCategory, Severity};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Languages where import cycles are a known hazard (partially initialized
/// modules at import time).
const CYCLE_LANGUAGES: &[&str] = &["python", "javascript", "typescript"];

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// One symbol with everything connected to it.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolDetail {
    pub symbol: StoredSymbol,
    /// Qualified name of the enclosing symbol.
    pub parent: Option<String>,
    /// `kind qualified_name` of directly nested symbols.
    pub children: Vec<String>,
    /// References made from inside the symbol (calls, type uses, imports).
    pub outgoing: Vec<StoredRelationship>,
    /// References to the symbol (or to its members) from outside it.
    pub incoming: Vec<StoredRelationship>,
    /// Other files that reference it.
    pub dependent_files: Vec<String>,
    pub diagnostics: Vec<StoredDiagnostic>,
    pub external_references: Vec<StoredExternalReference>,
    /// Changes recorded by Inspectye scans, oldest first.
    pub changes: Vec<StoredSymbolChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// What the targets depend on.
    Dependencies,
    /// What depends on the targets.
    Dependents,
}

/// A file on the other side of a dependency.
#[derive(Debug, Clone, Serialize)]
pub struct FileLink {
    pub path: String,
    /// Strongest certainty of the direct relationships.
    pub certainty: Certainty,
    pub evidence: String,
    /// The individual imports, calls and type uses behind the link.
    pub relationships: Vec<StoredRelationship>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyReport {
    pub targets: Vec<String>,
    pub direction: Direction,
    pub files: Vec<FileLink>,
    /// Imports that name no project file (packages, standard library).
    /// Only for `dependencies`.
    pub packages: Vec<String>,
    /// Relative imports that name no project file (see `check dependencies`).
    pub unresolved_relative: Vec<String>,
}

/// An imported package or module outside the project.
#[derive(Debug, Clone, Serialize)]
pub struct PackageUse {
    pub name: String,
    pub used_by: Vec<String>,
}

/// Resources the selected files reference.
#[derive(Debug, Clone, Serialize)]
pub struct ReferencesReport {
    /// Imports and source-map links resolved to project files.
    pub project_files: Vec<StoredRelationship>,
    pub packages: Vec<PackageUse>,
    /// Relative imports that name no project file.
    pub unresolved_relative: Vec<PackageUse>,
    /// Absolute filesystem paths, with their last check.
    pub external: Vec<StoredExternalReference>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsReport {
    pub diagnostics: Vec<StoredDiagnostic>,
    pub external_references: Vec<StoredExternalReference>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SeverityCounts {
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
}

/// One finding of a check, in a shape shared by every check.
#[derive(Debug, Clone, Serialize)]
pub struct CheckFinding {
    /// `syntax`, `logic`, `dependency` or `external`.
    pub category: String,
    pub rule: String,
    pub severity: Severity,
    pub certainty: Certainty,
    pub message: String,
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub symbol: Option<String>,
    pub evidence: Option<String>,
    /// Source lines with a marker, when available.
    pub excerpt: Option<String>,
    /// Other files involved: users of the enclosing symbol, or the other
    /// files of an import cycle.
    pub related_files: Vec<String>,
}

/// Result of a check. `passed` is false when there is an ERROR, or with
/// `strict` also a WARNING.
#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    /// `syntax`, `logic`, `dependencies` or `project`.
    pub scope: String,
    pub strict: bool,
    pub passed: bool,
    pub counts: SeverityCounts,
    pub findings: Vec<CheckFinding>,
}

impl CheckReport {
    fn new(scope: &str, strict: bool, mut findings: Vec<CheckFinding>) -> Self {
        findings.sort_by(|a, b| {
            (&a.path, a.line.unwrap_or(0), &a.rule).cmp(&(&b.path, b.line.unwrap_or(0), &b.rule))
        });
        let mut counts = SeverityCounts::default();
        for f in &findings {
            match f.severity {
                Severity::Error => counts.errors += 1,
                Severity::Warning => counts.warnings += 1,
                Severity::Info => counts.info += 1,
            }
        }
        let passed = counts.errors == 0 && (!strict || counts.warnings == 0);
        Self {
            scope: scope.to_string(),
            strict,
            passed,
            counts,
            findings,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TreeOptions {
    pub max_depth: Option<usize>,
    pub tracked_only: bool,
    /// Show only ignored files (by default they are left out).
    pub ignored_only: bool,
    pub directories_only: bool,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

impl<G: GitProvider> ProjectManager<G> {
    /// Symbols matching `query` (identity, qualified name with `.` or `::`,
    /// a qualified-name suffix, or a simple name), optionally only in the
    /// selected files, with their connections.
    pub fn symbol_details(
        &self,
        start_dir: &Path,
        query: &str,
        paths: &[String],
    ) -> Result<Vec<SymbolDetail>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        let all = repo.get_project_symbols(project.id)?;
        let best = all
            .iter()
            .filter(|s| selects_any(&filters, &s.path))
            .map(|s| symbol_match_rank(s, query))
            .max()
            .unwrap_or(0);
        if best == 0 {
            return Err(InspectyeError::NotFound(format!(
                "no symbol matches '{query}'. Run 'inspecteye deep-scan' if the code is new."
            )));
        }
        let matches: Vec<&StoredSymbol> = all
            .iter()
            .filter(|s| selects_any(&filters, &s.path) && symbol_match_rank(s, query) == best)
            .collect();

        let by_id: HashMap<i64, &StoredSymbol> = all.iter().map(|s| (s.id, s)).collect();
        let mut children_of: HashMap<i64, Vec<i64>> = HashMap::new();
        for s in &all {
            if let Some(p) = s.parent_id {
                children_of.entry(p).or_default().push(s.id);
            }
        }
        let relationships = repo.get_relationships(project.id)?;
        let diagnostics = repo.get_diagnostics(project.id, None)?;
        let externals = repo.get_external_references(project.id)?;
        let changes = repo.get_symbol_changes(project.id)?;

        let mut out = Vec::new();
        for s in matches {
            // The symbol and everything nested in it.
            let mut subtree: HashSet<i64> = HashSet::from([s.id]);
            let mut stack = vec![s.id];
            while let Some(id) = stack.pop() {
                for c in children_of.get(&id).into_iter().flatten() {
                    if subtree.insert(*c) {
                        stack.push(*c);
                    }
                }
            }
            let inside = |id: Option<i64>| id.is_some_and(|i| subtree.contains(&i));
            let (mut outgoing, mut incoming) = (Vec::new(), Vec::new());
            for r in &relationships {
                let rel = &r.relationship;
                if rel.kind == RelationshipKind::DependsOn {
                    continue;
                }
                let from_inside = inside(rel.source_symbol_id);
                let to_inside = inside(rel.target_symbol_id);
                if from_inside && !to_inside {
                    outgoing.push(r.clone());
                } else if to_inside && !from_inside {
                    incoming.push(r.clone());
                }
            }
            let mut dependent_files: Vec<String> = incoming
                .iter()
                .filter(|r| r.source_path != s.path)
                .map(|r| r.source_path.clone())
                .collect();
            dependent_files.sort();
            dependent_files.dedup();
            let range = &s.symbol.range;
            let within = |start: usize| start >= range.start_byte && start < range.end_byte;
            out.push(SymbolDetail {
                parent: s
                    .parent_id
                    .and_then(|p| by_id.get(&p))
                    .map(|p| p.symbol.qualified_name.clone()),
                children: children_of
                    .get(&s.id)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| by_id.get(c))
                    .map(|c| format!("{} {}", c.symbol.kind.as_str(), c.symbol.qualified_name))
                    .collect(),
                outgoing,
                incoming,
                dependent_files,
                diagnostics: diagnostics
                    .iter()
                    .filter(|d| d.file_id == s.file_id && within(d.diagnostic.range.start_byte))
                    .cloned()
                    .collect(),
                external_references: externals
                    .iter()
                    .filter(|e| e.file_id == s.file_id && within(e.reference.range.start_byte))
                    .cloned()
                    .collect(),
                changes: changes
                    .iter()
                    .filter(|c| c.file_id == s.file_id && c.identity == s.symbol.identity)
                    .cloned()
                    .collect(),
                symbol: s.clone(),
            });
        }
        Ok(out)
    }

    /// Files the selection depends on, or files that depend on it.
    pub fn dependency_report(
        &self,
        start_dir: &Path,
        paths: &[String],
        direction: Direction,
    ) -> Result<DependencyReport> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        let relationships = repo.get_relationships(project.id)?;
        let selected = |p: &str| selects_any(&filters, p);

        // The file on the other side, for relationships crossing the selection.
        let other_side = |r: &StoredRelationship| -> Option<String> {
            let target = r.target_path.as_deref()?;
            let (src, tgt) = (selected(&r.source_path), selected(target));
            match direction {
                Direction::Dependencies if src && !tgt => Some(target.to_string()),
                Direction::Dependents if tgt && !src => Some(r.source_path.clone()),
                _ => None,
            }
        };

        let mut links: BTreeMap<String, FileLink> = BTreeMap::new();
        for r in relationships
            .iter()
            .filter(|r| r.relationship.kind == RelationshipKind::DependsOn)
        {
            if let Some(other) = other_side(r) {
                links.insert(
                    other.clone(),
                    FileLink {
                        path: other,
                        certainty: r.relationship.certainty,
                        evidence: r.relationship.evidence.clone(),
                        relationships: Vec::new(),
                    },
                );
            }
        }
        for r in relationships
            .iter()
            .filter(|r| r.relationship.kind != RelationshipKind::DependsOn)
        {
            if let Some(link) = other_side(r).and_then(|o| links.get_mut(&o)) {
                link.relationships.push(r.clone());
            }
        }

        let unresolved: Vec<String> = match direction {
            Direction::Dependents => Vec::new(),
            Direction::Dependencies => relationships
                .iter()
                .filter(|r| {
                    r.relationship.kind == RelationshipKind::Imports
                        && r.target_path.is_none()
                        && selected(&r.source_path)
                })
                .map(|r| r.relationship.target_name.clone())
                .collect(),
        };
        let (mut unresolved_relative, mut packages): (Vec<String>, Vec<String>) = unresolved
            .into_iter()
            .partition(|t| is_relative_specifier(t));
        for list in [&mut packages, &mut unresolved_relative] {
            list.sort();
            list.dedup();
        }

        Ok(DependencyReport {
            targets: filters,
            direction,
            files: links.into_values().collect(),
            packages,
            unresolved_relative,
        })
    }

    /// Resources referenced by the selected files.
    pub fn references_report(
        &self,
        start_dir: &Path,
        paths: &[String],
    ) -> Result<ReferencesReport> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        let mut project_files = Vec::new();
        let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut relative: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in repo.get_relationships(project.id)? {
            let rel = &r.relationship;
            if !selects_any(&filters, &r.source_path)
                || !matches!(
                    rel.kind,
                    RelationshipKind::Imports | RelationshipKind::GeneratedFrom
                )
            {
                continue;
            }
            match &r.target_path {
                Some(t) if *t != r.source_path => project_files.push(r),
                Some(_) => {}
                None => {
                    let map = if is_relative_specifier(&rel.target_name) {
                        &mut relative
                    } else {
                        &mut packages
                    };
                    let users = map.entry(rel.target_name.clone()).or_default();
                    if !users.contains(&r.source_path) {
                        users.push(r.source_path.clone());
                    }
                }
            }
        }
        let external = repo
            .get_external_references(project.id)?
            .into_iter()
            .filter(|e| selects_any(&filters, &e.file_path))
            .collect();
        let uses = |m: BTreeMap<String, Vec<String>>| {
            m.into_iter()
                .map(|(name, used_by)| PackageUse { name, used_by })
                .collect()
        };
        Ok(ReferencesReport {
            project_files,
            packages: uses(packages),
            unresolved_relative: uses(relative),
            external,
        })
    }

    /// Stored diagnostics and external-reference findings of the selection.
    /// `category` is `syntax`, `logic` or `external` (all when `None`).
    pub fn diagnostics_report(
        &self,
        start_dir: &Path,
        paths: &[String],
        category: Option<&str>,
    ) -> Result<DiagnosticsReport> {
        let diagnostic_category = match category {
            None | Some("external") => None,
            Some(c) => Some(DiagnosticCategory::parse(c).ok_or_else(|| {
                InspectyeError::InvalidArgument(format!(
                    "unknown category '{c}'; expected syntax, logic or external"
                ))
            })?),
        };
        let diagnostics = if category == Some("external") {
            Vec::new()
        } else {
            self.diagnostics(start_dir, diagnostic_category, paths)?
        };
        let external_references = if category.is_none() || category == Some("external") {
            self.external_references(start_dir, paths)?
        } else {
            Vec::new()
        };
        Ok(DiagnosticsReport {
            diagnostics,
            external_references,
        })
    }

    /// Syntax findings of the selection (stored; refresh first).
    pub fn check_syntax(
        &self,
        start_dir: &Path,
        paths: &[String],
        strict: bool,
    ) -> Result<CheckReport> {
        let findings = self
            .diagnostics(start_dir, Some(DiagnosticCategory::Syntax), paths)?
            .iter()
            .map(|d| diagnostic_finding(d, Vec::new()))
            .collect();
        Ok(CheckReport::new("syntax", strict, findings))
    }

    /// Logic findings of the selection, with the files that use the
    /// enclosing symbol (stored; refresh first).
    pub fn check_logic(
        &self,
        start_dir: &Path,
        paths: &[String],
        strict: bool,
    ) -> Result<CheckReport> {
        let dependents = self.symbol_dependents(start_dir)?;
        let findings = self
            .diagnostics(start_dir, Some(DiagnosticCategory::Logic), paths)?
            .iter()
            .map(|d| {
                let users = d
                    .diagnostic
                    .symbol
                    .as_ref()
                    .and_then(|s| dependents.get(&(d.path.clone(), s.clone())))
                    .cloned()
                    .unwrap_or_default();
                diagnostic_finding(d, users)
            })
            .collect();
        Ok(CheckReport::new("logic", strict, findings))
    }

    /// Broken project-relative imports, import cycles, and external
    /// references (re-checked now).
    pub fn check_dependencies(
        &self,
        start_dir: &Path,
        paths: &[String],
        strict: bool,
    ) -> Result<CheckReport> {
        Ok(CheckReport::new(
            "dependencies",
            strict,
            self.dependency_findings(start_dir, paths)?,
        ))
    }

    /// Every check over the whole project.
    pub fn check_project(&self, start_dir: &Path, strict: bool) -> Result<CheckReport> {
        let mut findings = self.check_syntax(start_dir, &[], strict)?.findings;
        findings.extend(self.check_logic(start_dir, &[], strict)?.findings);
        findings.extend(self.dependency_findings(start_dir, &[])?);
        Ok(CheckReport::new("project", strict, findings))
    }

    fn dependency_findings(&self, start_dir: &Path, paths: &[String]) -> Result<Vec<CheckFinding>> {
        let (root, mut repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        let mut findings = Vec::new();

        let files = repo.get_file_nodes(project.id)?;
        let references: Vec<_> = repo
            .get_reference_nodes(project.id)?
            .into_iter()
            .filter(|r| {
                r.reference.kind.as_str() == "import" || r.reference.kind.as_str() == "module_decl"
            })
            .collect();
        let exists = |rel: &str| root.join(rel).is_file();
        for b in find_broken_imports(&files, &references, &LanguageRegistry::default(), &exists) {
            if !selects_any(&filters, &b.path) {
                continue;
            }
            findings.push(CheckFinding {
                category: "dependency".into(),
                rule: "dependency.broken_import".into(),
                severity: Severity::Error,
                certainty: Certainty::Fact,
                message: format!("`{}` names a project file that does not exist.", b.target),
                path: b.path,
                line: Some(b.line),
                column: Some(b.column),
                symbol: None,
                evidence: Some(
                    "No file on disk matches any location the language resolves this import to."
                        .into(),
                ),
                excerpt: None,
                related_files: Vec::new(),
            });
        }

        let language: HashMap<i64, Option<&str>> = files
            .iter()
            .map(|f| (f.id, f.language.as_deref()))
            .collect();
        let path_of: HashMap<i64, &str> = files.iter().map(|f| (f.id, f.path.as_str())).collect();
        let edges: Vec<(i64, i64)> = repo
            .get_relationships(project.id)?
            .into_iter()
            .filter(|r| r.relationship.kind == RelationshipKind::Imports)
            .filter_map(|r| {
                let (s, t) = (
                    r.relationship.source_file_id,
                    r.relationship.target_file_id?,
                );
                let lang = language.get(&s).copied().flatten()?;
                CYCLE_LANGUAGES.contains(&lang).then_some((s, t))
            })
            .collect();
        for cycle in import_cycles(&edges) {
            let members: Vec<String> = cycle
                .iter()
                .filter_map(|id| path_of.get(id).map(|p| p.to_string()))
                .collect();
            if !members.iter().any(|p| selects_any(&filters, p)) {
                continue;
            }
            findings.push(CheckFinding {
                category: "dependency".into(),
                rule: "dependency.import_cycle".into(),
                severity: Severity::Info,
                certainty: Certainty::Fact,
                message: format!(
                    "Import cycle among {} files; modules in a cycle can see each other partially initialized.",
                    members.len()
                ),
                path: members[0].clone(),
                line: None,
                column: None,
                symbol: None,
                evidence: Some(format!("Each file imports another: {}", members.join(" -> "))),
                excerpt: None,
                related_files: members[1..].to_vec(),
            });
        }

        repo.begin()?;
        let externals = match check_external_references(&mut repo, project.id, &root) {
            Ok((refs, _)) => refs,
            Err(e) => {
                let _ = repo.rollback();
                return Err(e);
            }
        };
        repo.commit()?;
        findings.extend(
            externals
                .iter()
                .filter(|e| selects_any(&filters, &e.file_path))
                .filter_map(external_finding),
        );
        Ok(findings)
    }

    /// The indexed directory hierarchy under `target` (the project root
    /// when `None`).
    pub fn tree(
        &self,
        start_dir: &Path,
        target: Option<&str>,
        opts: TreeOptions,
    ) -> Result<TreeNode> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let base = match target {
            Some(t) => to_filters(&root, &[t.to_string()])?.remove(0),
            None => String::new(),
        };
        let entries: Vec<(String, TreeFile)> = repo
            .get_all_files(project.id)?
            .into_iter()
            .filter(|f| f.exists_on_disk)
            .filter(|f| f.is_ignored == opts.ignored_only)
            .filter(|f| !opts.tracked_only || f.git_tracked)
            .map(|f| {
                (
                    f.path,
                    TreeFile {
                        kind: f.kind,
                        language: f.language,
                        lines: f.line_count,
                        size_bytes: f.size_bytes,
                        git_tracked: f.git_tracked,
                        ignored: f.is_ignored,
                    },
                )
            })
            .collect();

        // A file target shows that one file under its directory.
        if !base.is_empty() && entries.iter().any(|(p, _)| *p == base) {
            let parent = base.rsplit_once('/').map_or("", |(d, _)| d).to_string();
            let only: Vec<_> = entries.into_iter().filter(|(p, _)| *p == base).collect();
            let name = if parent.is_empty() {
                project.name.clone()
            } else {
                parent.clone()
            };
            return Ok(build_tree(&name, &parent, only, opts.max_depth, false));
        }
        let name = if base.is_empty() {
            project.name.clone()
        } else {
            base.clone()
        };
        let tree = build_tree(&name, &base, entries, opts.max_depth, opts.directories_only);
        if tree.file_count == 0 && !base.is_empty() {
            return Err(InspectyeError::NotFound(format!(
                "nothing indexed under '{base}'. Run 'inspecteye scan' if it is new."
            )));
        }
        Ok(tree)
    }
}

/// `./x`, `../x` (JS/TS) or `.x` (Python): names a project location.
fn is_relative_specifier(target: &str) -> bool {
    target.starts_with('.')
}

/// 3: identity, 2: qualified name, 1: qualified-name suffix or simple name.
fn symbol_match_rank(s: &StoredSymbol, query: &str) -> u8 {
    let norm = |x: &str| x.replace("::", ".");
    let (q, qualified) = (norm(query), norm(&s.symbol.qualified_name));
    if s.symbol.identity == query {
        3
    } else if qualified == q {
        2
    } else if s.symbol.name == query || qualified.ends_with(&format!(".{q}")) {
        1
    } else {
        0
    }
}

fn diagnostic_finding(d: &StoredDiagnostic, related_files: Vec<String>) -> CheckFinding {
    let diag = &d.diagnostic;
    CheckFinding {
        category: diag.category.as_str().to_string(),
        rule: diag.rule.clone(),
        severity: diag.severity,
        certainty: diag.certainty,
        message: diag.message.clone(),
        path: d.path.clone(),
        line: Some(diag.range.start_line),
        column: Some(diag.range.start_column),
        symbol: diag.symbol.clone(),
        evidence: diag.evidence.clone(),
        excerpt: Some(diag.excerpt.clone()).filter(|e| !e.is_empty()),
        related_files,
    }
}

fn external_finding(e: &StoredExternalReference) -> Option<CheckFinding> {
    let check = e.check.as_ref()?;
    let mut evidence = vec![match &check.existence {
        Existence::Exists { is_directory } => {
            format!("exists{}", if *is_directory { " (directory)" } else { "" })
        }
        Existence::Missing { parent_exists } => format!(
            "missing; parent directory {}",
            if *parent_exists {
                "exists"
            } else {
                "missing too"
            }
        ),
        Existence::Unchecked { reason } => format!("not checked: {reason}"),
    }];
    evidence.push(format!(
        "inside repository: {}",
        if check.inside_repository { "yes" } else { "no" }
    ));
    if let Some(m) = &check.machine_specific {
        evidence.push(format!("machine-specific: {m}"));
    }
    Some(CheckFinding {
        category: "external".into(),
        rule: check.rule.clone(),
        severity: check.severity,
        certainty: check.certainty,
        message: check.message.clone(),
        path: e.file_path.clone(),
        line: Some(e.reference.range.start_line),
        column: Some(e.reference.range.start_column),
        symbol: e.reference.symbol.clone(),
        evidence: Some(evidence.join("; ")),
        excerpt: None,
        related_files: Vec::new(),
    })
}
