use crate::analysis::language::LanguageRegistry;
use crate::analysis::source_map::source_map_links;
use crate::analysis::{
    compute_file_hash, compute_line_diff, ChangeTag, DeepAnalysisResult, DeepAnalyzer, DiffHunk,
    FileAnalysis, FileState, StateTransition, TransitionType,
};
use crate::database::{
    open_database, AnalysisRun, FileAnalysisRecord, FileRecord, FileStateRecord, FileStats,
    NewFileRecord, Project, Repository, StateTransitionRecord, StoredDiagnostic,
    StoredExternalReference, StoredRelationship, StoredSymbol, StoredSymbolChange,
};
use crate::error::{InspectyeError, Result};
use crate::git::{CommitInfo, GitCli, GitProvider};
use crate::intelligence::{
    compute_impact, diff_symbols, evaluate_external, extract_symbols, file_degrees, find_symbol,
    partial_type_groups, resolve_relationships, row, symbols_of, trace_symbol, AffectedFile,
    ExtractedSymbol, FileDegree, ImpactSeed, PartialDiagnosticInput, PartialSymbolInput,
    PartialTypeGroup, Ranking, ResolutionStats, ResolvedRelationship, SymbolChange,
    SymbolChangeKind, SymbolKind, SymbolRevision,
};
use crate::issues::{DiagnosticCategory, Severity};
use crate::scanner::{
    classify_path, classify_resource, discover_files, inspect_file_metadata, FileMetadata,
    InspectyeIgnore, PathFilter, IGNORE_FILE_NAME,
};
use chrono::Utc;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const INSPECTYE_DIR: &str = ".inspectye";
pub const DB_NAME: &str = "inspectye.db";
/// Commits read for Git-based change rankings.
const GIT_HISTORY_WINDOW: usize = 500;

pub fn normalize_path(path: &Path) -> PathBuf {
    let p_str = path.to_string_lossy();
    if let Some(stripped) = p_str.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InitResult {
    pub project_name: String,
    pub root_path: PathBuf,
    pub db_path: PathBuf,
    pub already_initialized: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ResourceBreakdown {
    pub source: usize,
    pub config: usize,
    pub documentation: usize,
    pub asset: usize,
    pub dependency: usize,
    pub generated: usize,
    pub binary: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub project_name: String,
    pub project_root: PathBuf,
    pub files_discovered: usize,
    pub resources: ResourceBreakdown,
    pub new_files: usize,
    pub changed_files: usize,
    pub unchanged_files: usize,
    pub restored_files: usize,
    pub restored_changed_files: usize,
    pub removed_files: usize,
    pub failed_files: usize,
    #[serde(rename = "duration_ms", serialize_with = "duration_ms")]
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeepScanSummary {
    pub project_name: String,
    pub project_root: PathBuf,
    pub total_eligible: usize,
    pub analyzed: usize,
    pub skipped: usize,
    pub ineligible: usize,
    pub failed: usize,
    pub resolution: ResolutionStats,
    pub external_references: ExternalReferenceCounts,
    #[serde(rename = "duration_ms", serialize_with = "duration_ms")]
    pub duration: std::time::Duration,
}

/// External references by outcome after the latest check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExternalReferenceCounts {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
}

fn duration_ms<S: serde::Serializer>(
    d: &std::time::Duration,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_millis() as u64)
}

/// Why a file is a starting point of an impact analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImpactOrigin {
    pub path: String,
    /// `pending` (working tree, not yet scanned), `last scan`, or `requested`.
    pub source: String,
    pub change: String,
    /// Qualified names of modified symbols; empty with `whole_file`.
    pub modified_symbols: Vec<String>,
    pub removed_symbols: Vec<String>,
    pub whole_file: bool,
}

#[derive(Debug, Clone)]
pub struct ImpactReport {
    pub origins: Vec<ImpactOrigin>,
    pub affected: Vec<AffectedFile>,
    pub file_paths: HashMap<i64, String>,
    pub symbol_names: HashMap<i64, String>,
    pub max_depth: usize,
}

/// Serialized with paths and symbol names in place of database ids, so the
/// JSON output is self-contained.
impl Serialize for ImpactReport {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Step<'a> {
            kind: crate::intelligence::RelationshipKind,
            dependent: &'a str,
            dependency: &'a str,
            symbol: Option<&'a str>,
            line: usize,
            certainty: crate::issues::Certainty,
        }
        #[derive(Serialize)]
        struct Affected<'a> {
            path: &'a str,
            depth: usize,
            certainty: crate::issues::Certainty,
            references_removed_symbol: bool,
            via: Vec<Step<'a>>,
        }
        #[derive(Serialize)]
        struct View<'a> {
            origins: &'a [ImpactOrigin],
            max_depth: usize,
            affected: Vec<Affected<'a>>,
        }
        let path = |id: i64| self.file_paths.get(&id).map(String::as_str).unwrap_or("?");
        View {
            origins: &self.origins,
            max_depth: self.max_depth,
            affected: self
                .affected
                .iter()
                .map(|a| Affected {
                    path: path(a.file_id),
                    depth: a.depth,
                    certainty: a.certainty,
                    references_removed_symbol: a.references_removed_symbol,
                    via: a
                        .via
                        .iter()
                        .map(|v| Step {
                            kind: v.kind,
                            dependent: path(v.dependent_file_id),
                            dependency: path(v.dependency_file_id),
                            symbol: v
                                .target_symbol_id
                                .and_then(|id| self.symbol_names.get(&id))
                                .map(String::as_str),
                            line: v.line,
                            certainty: v.certainty,
                        })
                        .collect(),
                })
                .collect(),
        }
        .serialize(s)
    }
}

/// Everything Inspectye knows about one file, in one read. Meant for AI
/// agents that should not have to read the file or the whole project.
#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub file: FileRecord,
    /// Stored analysis state; `None` when the file was never deep-scanned.
    pub analysis: Option<FileAnalysisRecord>,
    /// False when the file changed after its last analysis: symbols,
    /// diagnostics and relationships below describe an older version.
    pub analysis_current: bool,
    pub symbols: Vec<StoredSymbol>,
    pub diagnostics: Vec<StoredDiagnostic>,
    /// Relationships from this file to other files or packages.
    pub dependencies: Vec<StoredRelationship>,
    /// Relationships from other files to this one.
    pub dependents: Vec<StoredRelationship>,
    /// Absolute filesystem paths written in the file, with their last check.
    pub external_references: Vec<StoredExternalReference>,
    /// Most recent recorded working-tree changes, newest first.
    pub recent_changes: Vec<RecordedChange>,
    /// Most recent recorded symbol changes, newest first.
    pub recent_symbol_changes: Vec<StoredSymbolChange>,
}

/// One entry of a file's history.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    /// A Git commit, or `None` for the working tree.
    pub commit: Option<CommitInfo>,
    /// With a symbol query: how the symbol changed in this version.
    pub symbol_change: Option<SymbolRevision>,
}

/// A working-tree state change recorded by Inspectye scans.
#[derive(Debug, Clone, Serialize)]
pub struct RecordedChange {
    pub recorded_at: String,
    pub transition: String,
    pub added_lines: i64,
    pub removed_lines: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryReport {
    pub path: String,
    pub symbol: Option<String>,
    pub git_available: bool,
    /// Newest first.
    pub entries: Vec<HistoryEntry>,
    /// Oldest first.
    pub recorded: Vec<RecordedChange>,
    /// Symbol changes recorded by Inspectye analyses of the file, oldest first.
    pub symbol_changes: Vec<StoredSymbolChange>,
}

/// Two versions of a file (or of one symbol in it) compared.
#[derive(Debug, Clone, Serialize)]
pub struct FileComparison {
    pub path: String,
    pub from_label: String,
    pub to_label: String,
    pub symbol: Option<String>,
    /// Line offset of the compared text in each version (symbol mode).
    pub from_line_offset: usize,
    pub to_line_offset: usize,
    pub hunks: Vec<DiffHunk>,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub symbol_changes: Vec<SymbolChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub project_name: String,
    pub root_path: PathBuf,
    pub db_path: PathBuf,
    pub stats: FileStats,
    pub last_run: Option<AnalysisRun>,
    pub last_deep_scan: Option<AnalysisRun>,
    pub git_active: bool,
}

pub struct ProjectManager<G: GitProvider = GitCli> {
    git: G,
}

impl Default for ProjectManager<GitCli> {
    fn default() -> Self {
        Self::new(GitCli::new())
    }
}

impl<G: GitProvider> ProjectManager<G> {
    pub fn new(git: G) -> Self {
        Self { git }
    }

    pub fn find_project_root(&self, start_dir: &Path) -> Result<PathBuf> {
        let mut curr = start_dir
            .canonicalize()
            .map(|p| normalize_path(&p))
            .unwrap_or_else(|_| normalize_path(start_dir));
        loop {
            let inspectye_dir = curr.join(INSPECTYE_DIR);
            let db_file = inspectye_dir.join(DB_NAME);
            if inspectye_dir.is_dir() && db_file.is_file() {
                return Ok(curr);
            }
            if !curr.pop() {
                break;
            }
        }
        Err(InspectyeError::NotInitialized(start_dir.to_path_buf()))
    }

    pub fn init(&self, path: &Path) -> Result<InitResult> {
        if !path.exists() {
            return Err(InspectyeError::InvalidProjectLocation(format!(
                "Directory does not exist: {}",
                path.display()
            )));
        }

        let canonical_root = path
            .canonicalize()
            .map(|p| normalize_path(&p))
            .map_err(|e| {
                InspectyeError::InvalidProjectLocation(format!("{}: {e}", path.display()))
            })?;

        if !canonical_root.is_dir() {
            return Err(InspectyeError::InvalidProjectLocation(format!(
                "Path is not a directory: {}",
                canonical_root.display()
            )));
        }

        let project_name = canonical_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let inspectye_dir = canonical_root.join(INSPECTYE_DIR);
        let db_path = inspectye_dir.join(DB_NAME);
        let already_initialized = inspectye_dir.exists() && db_path.exists();

        std::fs::create_dir_all(&inspectye_dir)?;

        let conn = open_database(&db_path)?;
        let mut repo = Repository::new(conn);
        repo.get_or_create_project(&canonical_root, &project_name)?;

        Ok(InitResult {
            project_name,
            root_path: canonical_root,
            db_path,
            already_initialized,
        })
    }

    pub fn scan(&self, start_dir: &Path, trigger: &str) -> Result<ScanSummary> {
        let start_time = Instant::now();
        let root = self.find_project_root(start_dir)?;
        let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);

        let conn = open_database(&db_path)?;
        let mut repo = Repository::new(conn);

        let project = repo
            .get_project_by_root(&root)?
            .or_else(|| repo.get_first_project().ok().flatten())
            .ok_or_else(|| InspectyeError::NotInitialized(root.clone()))?;

        let run_id = repo.create_analysis_run(project.id, trigger)?;

        // Git state is resolved once per scan, not per file.
        let is_git = self.git.is_git_repository(&root);
        let tracked_files = if is_git {
            self.git.get_tracked_files(&root).unwrap_or_default()
        } else {
            Default::default()
        };
        let head_commit = if is_git {
            self.git.get_head_commit(&root)
        } else {
            None
        };

        let filter = PathFilter::default();
        let ignore = InspectyeIgnore::from_file(&root.join(IGNORE_FILE_NAME));
        let discovered = discover_files(&root, &filter)?;
        let discovered_count = discovered.len();

        let existing_map: HashMap<String, FileRecord> = repo
            .get_all_files(project.id)?
            .into_iter()
            .map(|f| (f.path.clone(), f))
            .collect();

        let mut summary = ScanSummary {
            project_name: project.name.clone(),
            project_root: root.clone(),
            files_discovered: discovered_count,
            resources: ResourceBreakdown::default(),
            new_files: 0,
            changed_files: 0,
            unchanged_files: 0,
            restored_files: 0,
            restored_changed_files: 0,
            removed_files: 0,
            failed_files: 0,
            duration: Default::default(),
        };
        let mut discovered_rel_paths = Vec::with_capacity(discovered_count);
        let now = Utc::now().to_rfc3339();
        let ctx = ScanContext {
            project_id: project.id,
            run_id,
            head_commit: head_commit.as_deref(),
            now: &now,
        };

        repo.begin()?;
        let result = (|| -> Result<()> {
            for file in discovered {
                discovered_rel_paths.push(file.relative_path.clone());

                let (hash, meta) = match compute_file_hash(&file.absolute_path)
                    .and_then(|h| inspect_file_metadata(&file.absolute_path).map(|m| (h, m)))
                {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("warning: could not read '{}': {e}", file.relative_path);
                        summary.failed_files += 1;
                        continue;
                    }
                };

                let (kind, lang) =
                    classify_resource(Path::new(&file.relative_path), &file.absolute_path);
                summary.resources.count(&kind);

                let upserted = repo.upsert_file(&NewFileRecord {
                    project_id: project.id,
                    path: file.relative_path.clone(),
                    kind,
                    language: lang,
                    size_bytes: meta.size_bytes,
                    line_count: meta.line_count,
                    content_hash: hash.clone(),
                    git_tracked: tracked_files.contains(&file.relative_path),
                    exists_on_disk: true,
                    is_ignored: ignore.is_ignored(&file.relative_path),
                    modified_at: meta.modified_at.clone(),
                    last_seen_at: now.clone(),
                })?;

                let previous = existing_map.get(&file.relative_path);
                let transition = record_present_file(
                    &mut repo,
                    &ctx,
                    &upserted,
                    previous,
                    &hash,
                    &file.absolute_path,
                    &meta,
                )?;

                match transition {
                    TransitionType::New => summary.new_files += 1,
                    TransitionType::Modified => summary.changed_files += 1,
                    TransitionType::Unchanged => summary.unchanged_files += 1,
                    TransitionType::Restored => summary.restored_files += 1,
                    TransitionType::RestoredModified => summary.restored_changed_files += 1,
                    TransitionType::Removed => {}
                }
            }

            let newly_removed =
                repo.mark_missing_files_as_removed(project.id, &discovered_rel_paths)?;
            summary.removed_files = newly_removed.len();
            for removed in newly_removed {
                record_removed_file(&mut repo, &ctx, &removed)?;
            }
            Ok(())
        })();

        match result {
            Ok(()) => repo.commit()?,
            Err(e) => {
                let _ = repo.rollback();
                let _ = repo.finish_analysis_run(run_id, "failed", 0, 0, 0, 0, 0, 0, 0, 0);
                return Err(e);
            }
        }

        let status = if summary.failed_files > 0 && discovered_count == summary.failed_files {
            "failed"
        } else {
            "completed"
        };

        repo.finish_analysis_run(
            run_id,
            status,
            discovered_count as i64,
            summary.new_files as i64,
            summary.changed_files as i64,
            summary.unchanged_files as i64,
            summary.restored_files as i64,
            summary.restored_changed_files as i64,
            summary.removed_files as i64,
            summary.failed_files as i64,
        )?;

        summary.duration = start_time.elapsed();
        Ok(summary)
    }

    pub fn deep_scan<A: DeepAnalyzer>(
        &self,
        start_dir: &Path,
        analyzer: &A,
    ) -> Result<DeepScanSummary> {
        let start_time = Instant::now();
        let (root, mut repo, project) = self.open_project(start_dir)?;

        let mut eligible_files = Vec::new();
        let mut ineligible_files = Vec::new();
        for file in repo.get_all_files(project.id)? {
            if analyzer.is_eligible(&file) {
                eligible_files.push(file);
            } else {
                ineligible_files.push(file);
            }
        }

        let total_eligible = eligible_files.len();
        let ineligible_count = ineligible_files.len();
        let mut analyzed = 0;
        let mut skipped = 0;
        let mut failed = 0;

        let run_id = repo.create_analysis_run(project.id, "deep_scan")?;
        let mut resolution = ResolutionStats::default();
        let mut external_references = ExternalReferenceCounts::default();

        repo.begin()?;
        let result = (|| -> Result<()> {
            // Findings for removed or newly ignored files no longer describe the project.
            for file in &ineligible_files {
                repo.delete_file_analysis_output(file.id)?;
            }

            for file in eligible_files {
                let previous = repo.get_file_analysis(file.id)?;
                // Symbol changes are only meaningful against a comparable
                // earlier analysis (same analyzer, completed).
                let comparable = previous.as_ref().is_some_and(|prev| {
                    prev.status == "completed"
                        && prev.analyzer_version.as_deref() == Some(analyzer.analyzer_version())
                });
                let needs_analysis = !comparable
                    || previous
                        .as_ref()
                        .is_some_and(|prev| prev.content_hash != file.content_hash);
                if !needs_analysis {
                    skipped += 1;
                    continue;
                }

                let now = Utc::now().to_rfc3339();
                let result =
                    analyzer
                        .analyze(&root, &file)
                        .unwrap_or_else(|e| DeepAnalysisResult {
                            file_id: file.id,
                            content_hash: file.content_hash.clone(),
                            status: "failed".to_string(),
                            parser_version: analyzer.parser_version().to_string(),
                            analyzer_version: analyzer.analyzer_version().to_string(),
                            error: Some(e.to_string()),
                            analysis: None,
                        });

                match &result.analysis {
                    Some(analysis) if result.status == "completed" => {
                        store_analysis_output(
                            &mut repo,
                            project.id,
                            file.id,
                            &result.content_hash,
                            analysis,
                            Some(run_id),
                            comparable,
                        )?;
                        repo.update_file_last_analyzed_at(file.id, &now)?;
                        analyzed += 1;
                    }
                    _ => {
                        repo.delete_file_analysis_output(file.id)?;
                        failed += 1;
                    }
                }

                repo.record_file_analysis(&FileAnalysisRecord {
                    file_id: file.id,
                    content_hash: result.content_hash,
                    parser_version: Some(result.parser_version),
                    analyzer_version: Some(result.analyzer_version),
                    analyzed_at: now,
                    status: result.status,
                    error: result.error,
                })?;
            }
            resolution = resolve_project(&mut repo, project.id, &root)?;
            // Outside files change without the project changing, so every
            // reference is re-checked, not only those of re-analyzed files.
            external_references = check_external_references(&mut repo, project.id, &root)?.1;
            Ok(())
        })();

        if let Err(e) = result {
            let _ = repo.rollback();
            let _ = repo.finish_analysis_run(run_id, "failed", 0, 0, 0, 0, 0, 0, 0, 0);
            return Err(e);
        }
        repo.commit()?;

        let status = if failed > 0 && analyzed == 0 && total_eligible > 0 {
            "failed"
        } else {
            "completed"
        };

        repo.finish_analysis_run(
            run_id,
            status,
            total_eligible as i64,
            analyzed as i64,
            0,
            skipped as i64,
            0,
            0,
            0,
            failed as i64,
        )?;

        Ok(DeepScanSummary {
            project_name: project.name,
            project_root: root,
            total_eligible,
            analyzed,
            skipped,
            ineligible: ineligible_count,
            failed,
            resolution,
            external_references,
            duration: start_time.elapsed(),
        })
    }

    /// Brings the index and analysis up to date: a scan of the working tree
    /// followed by deep analysis of only the files whose content changed.
    pub fn refresh<A: DeepAnalyzer>(
        &self,
        start_dir: &Path,
        analyzer: &A,
    ) -> Result<(ScanSummary, DeepScanSummary)> {
        let scan = self.scan(start_dir, "refresh")?;
        let deep = self.deep_scan(start_dir, analyzer)?;
        Ok((scan, deep))
    }

    /// Stored diagnostics, optionally limited to a category and to paths.
    /// A path selects a file or, as a prefix, a directory. Paths may be
    /// absolute or relative to the project root.
    pub fn diagnostics(
        &self,
        start_dir: &Path,
        category: Option<DiagnosticCategory>,
        paths: &[String],
    ) -> Result<Vec<StoredDiagnostic>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        Ok(repo
            .get_diagnostics(project.id, category)?
            .into_iter()
            .filter(|d| selects_any(&filters, &d.path))
            .collect())
    }

    /// Stored external references of the selected files, with the result
    /// of their last check (made by the last deep scan).
    pub fn external_references(
        &self,
        start_dir: &Path,
        paths: &[String],
    ) -> Result<Vec<StoredExternalReference>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        Ok(repo
            .get_external_references(project.id)?
            .into_iter()
            .filter(|e| selects_any(&filters, &e.file_path))
            .collect())
    }

    /// Re-checks every external reference now and returns those of the
    /// selected files.
    pub fn check_external_references(
        &self,
        start_dir: &Path,
        paths: &[String],
    ) -> Result<Vec<StoredExternalReference>> {
        let (root, mut repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        repo.begin()?;
        let refs = match check_external_references(&mut repo, project.id, &root) {
            Ok((refs, _)) => refs,
            Err(e) => {
                let _ = repo.rollback();
                return Err(e);
            }
        };
        repo.commit()?;
        Ok(refs
            .into_iter()
            .filter(|e| selects_any(&filters, &e.file_path))
            .collect())
    }

    /// Stored symbols of the selected files (all files when `paths` is empty).
    pub fn symbols(&self, start_dir: &Path, paths: &[String]) -> Result<Vec<StoredSymbol>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        Ok(repo
            .get_project_symbols(project.id)?
            .into_iter()
            .filter(|s| selects_any(&filters, &s.path))
            .collect())
    }

    /// Recorded symbol changes of the selected files, oldest first.
    pub fn symbol_history(
        &self,
        start_dir: &Path,
        paths: &[String],
    ) -> Result<Vec<StoredSymbolChange>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        Ok(repo
            .get_symbol_changes(project.id)?
            .into_iter()
            .filter(|c| selects_any(&filters, &c.path))
            .collect())
    }

    /// Relationships whose source or target file is selected (all when
    /// `paths` is empty).
    pub fn relationships(
        &self,
        start_dir: &Path,
        paths: &[String],
    ) -> Result<Vec<StoredRelationship>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let filters = to_filters(&root, paths)?;
        Ok(repo
            .get_relationships(project.id)?
            .into_iter()
            .filter(|r| {
                selects_any(&filters, &r.source_path)
                    || r.target_path
                        .as_deref()
                        .is_some_and(|t| selects_any(&filters, t))
            })
            .collect())
    }

    /// Dependency degree of every file that has a dependency edge, with its path.
    pub fn dependency_metrics(&self, start_dir: &Path) -> Result<Vec<(String, FileDegree)>> {
        let (_, repo, project) = self.open_project(start_dir)?;
        let relationships = repo.get_relationships(project.id)?;
        let paths: HashMap<i64, String> = relationships
            .iter()
            .flat_map(|r| {
                let mut v = vec![(r.relationship.source_file_id, r.source_path.clone())];
                if let (Some(id), Some(p)) = (r.relationship.target_file_id, &r.target_path) {
                    v.push((id, p.clone()));
                }
                v
            })
            .collect();
        let rels: Vec<_> = relationships.into_iter().map(|r| r.relationship).collect();
        Ok(file_degrees(&rels)
            .into_iter()
            .map(|d| (paths.get(&d.file_id).cloned().unwrap_or_default(), d))
            .collect())
    }

    /// Files (other than its own) that reference each symbol, keyed by
    /// (path, qualified name). Used to show who may be affected by a finding.
    pub fn symbol_dependents(
        &self,
        start_dir: &Path,
    ) -> Result<HashMap<(String, String), Vec<String>>> {
        let (_, repo, project) = self.open_project(start_dir)?;
        let mut out: HashMap<(String, String), Vec<String>> = HashMap::new();
        for r in repo.get_relationships(project.id)? {
            let (Some(path), Some(symbol)) = (r.target_path, r.target_symbol) else {
                continue;
            };
            if path == r.source_path {
                continue;
            }
            let users = out.entry((path, symbol)).or_default();
            if !users.contains(&r.source_path) {
                users.push(r.source_path);
            }
        }
        Ok(out)
    }

    /// Files that may be affected by changing the given files, or only the
    /// named symbols in them. Uses current relationships.
    pub fn impact(
        &self,
        start_dir: &Path,
        paths: &[String],
        symbol_names: &[String],
        max_depth: usize,
    ) -> Result<ImpactReport> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let mut seeds = Vec::new();
        let mut origins = Vec::new();
        for p in paths {
            let rel = to_project_relative(&root, p)?;
            let files: Vec<FileRecord> = repo
                .get_all_files(project.id)?
                .into_iter()
                .filter(|f| f.exists_on_disk && path_selects(&rel, &f.path))
                .collect();
            if files.is_empty() {
                return Err(InspectyeError::FileNotFound(format!(
                    "'{rel}' is not indexed. Run 'inspecteye scan' first."
                )));
            }
            for file in files {
                let symbols = repo.get_file_symbols(file.id)?;
                let (symbol_ids, names) = if symbol_names.is_empty() {
                    (None, Vec::new())
                } else {
                    let picked: Vec<_> = symbols
                        .iter()
                        .filter(|s| {
                            symbol_names
                                .iter()
                                .any(|n| *n == s.symbol.name || *n == s.symbol.qualified_name)
                        })
                        .collect();
                    if picked.is_empty() {
                        continue;
                    }
                    (
                        Some(picked.iter().map(|s| s.id).collect()),
                        picked
                            .iter()
                            .map(|s| s.symbol.qualified_name.clone())
                            .collect(),
                    )
                };
                origins.push(ImpactOrigin {
                    path: file.path.clone(),
                    source: "requested".to_string(),
                    change: "any change".to_string(),
                    whole_file: symbol_ids.is_none(),
                    modified_symbols: names,
                    removed_symbols: Vec::new(),
                });
                seeds.push(ImpactSeed {
                    file_id: file.id,
                    symbol_ids,
                    removed_symbol_ids: Vec::new(),
                });
            }
        }
        if !symbol_names.is_empty() && seeds.is_empty() {
            return Err(InspectyeError::FileNotFound(format!(
                "No symbol named {} in the selected files.",
                symbol_names.join(", ")
            )));
        }
        self.build_impact_report(&repo, project.id, seeds, origins, max_depth)
    }

    /// Impact of the working tree's changes since the last scan, or, when
    /// there are none, of the changes recorded by the last scan.
    ///
    /// Uses the relationships recorded before the change, so references to
    /// removed declarations are still known. Run it before re-scanning.
    pub fn impact_of_changes(&self, start_dir: &Path, max_depth: usize) -> Result<ImpactReport> {
        let (root, repo, project) = self.open_project(start_dir)?;

        let mut changed: Vec<(String, &'static str)> = Vec::new();
        for f in repo
            .get_all_files(project.id)?
            .into_iter()
            .filter(|f| f.exists_on_disk)
        {
            let abs = root.join(&f.path);
            if !abs.is_file() {
                changed.push((f.path, "pending"));
            } else if compute_file_hash(&abs).map_or(false, |h| h != f.content_hash) {
                changed.push((f.path, "pending"));
            }
        }
        if changed.is_empty() {
            if let Some(run) = repo.get_latest_scan_run(project.id)? {
                changed = repo
                    .get_run_transitions(run.id)?
                    .into_iter()
                    .filter(|(_, t)| t != "new" && t != "unchanged")
                    .map(|(p, _)| (p, "last scan"))
                    .collect();
            }
        }

        let mut seeds = Vec::new();
        let mut origins = Vec::new();
        for (path, source) in changed {
            let Some(file) = repo.get_file_by_path(project.id, &path)? else {
                continue;
            };
            let transition = self.changes(start_dir, &path)?;
            let stored = repo.get_file_symbols(file.id)?;
            let id_of = |identity: &str| {
                stored
                    .iter()
                    .find(|s| s.symbol.identity == identity)
                    .map(|s| (s.id, s.symbol.qualified_name.clone()))
            };
            let mut origin = ImpactOrigin {
                path: path.clone(),
                source: source.to_string(),
                change: transition.transition_type.as_str().to_string(),
                modified_symbols: Vec::new(),
                removed_symbols: Vec::new(),
                whole_file: false,
            };
            let seed = match transition.transition_type {
                TransitionType::Removed => {
                    origin.whole_file = true;
                    origin.removed_symbols = stored
                        .iter()
                        .map(|s| s.symbol.qualified_name.clone())
                        .collect();
                    ImpactSeed {
                        file_id: file.id,
                        symbol_ids: None,
                        removed_symbol_ids: stored.iter().map(|s| s.id).collect(),
                    }
                }
                _ if transition.symbol_changes.is_empty() => {
                    origin.whole_file = true;
                    ImpactSeed {
                        file_id: file.id,
                        symbol_ids: None,
                        removed_symbol_ids: Vec::new(),
                    }
                }
                _ => {
                    let mut modified = Vec::new();
                    let mut removed = Vec::new();
                    for c in &transition.symbol_changes {
                        let Some((id, name)) = id_of(&c.identity) else {
                            continue;
                        };
                        match c.change {
                            SymbolChangeKind::Modified => {
                                modified.push(id);
                                origin.modified_symbols.push(name);
                            }
                            SymbolChangeKind::Removed => {
                                removed.push(id);
                                origin.removed_symbols.push(name);
                            }
                            SymbolChangeKind::Added => {}
                        }
                    }
                    ImpactSeed {
                        file_id: file.id,
                        symbol_ids: Some(modified),
                        removed_symbol_ids: removed,
                    }
                }
            };
            origins.push(origin);
            seeds.push(seed);
        }
        self.build_impact_report(&repo, project.id, seeds, origins, max_depth)
    }

    fn build_impact_report(
        &self,
        repo: &Repository,
        project_id: i64,
        seeds: Vec<ImpactSeed>,
        origins: Vec<ImpactOrigin>,
        max_depth: usize,
    ) -> Result<ImpactReport> {
        let stored = repo.get_relationships(project_id)?;
        let mut file_paths = HashMap::new();
        let mut symbol_names = HashMap::new();
        for r in &stored {
            file_paths.insert(r.relationship.source_file_id, r.source_path.clone());
            if let (Some(id), Some(p)) = (r.relationship.target_file_id, &r.target_path) {
                file_paths.insert(id, p.clone());
            }
            if let (Some(id), Some(n)) = (r.relationship.target_symbol_id, &r.target_symbol) {
                symbol_names.insert(id, n.clone());
            }
        }
        let relationships: Vec<ResolvedRelationship> =
            stored.into_iter().map(|r| r.relationship).collect();
        Ok(ImpactReport {
            affected: compute_impact(&seeds, &relationships, max_depth),
            origins,
            file_paths,
            symbol_names,
            max_depth,
        })
    }

    /// C# partial types grouped across files, with aggregated metrics.
    pub fn partial_types(&self, start_dir: &Path) -> Result<Vec<PartialTypeGroup>> {
        let (_, repo, project) = self.open_project(start_dir)?;
        let symbols: Vec<PartialSymbolInput> = repo
            .get_project_symbols(project.id)?
            .into_iter()
            .map(|s| PartialSymbolInput {
                id: s.id,
                file_id: s.file_id,
                is_partial: s.symbol.has_modifier("partial"),
                path: s.path,
                kind: s.symbol.kind,
                qualified_name: s.symbol.qualified_name,
                start_line: s.symbol.range.start_line,
                end_line: s.symbol.range.end_line,
            })
            .collect();
        let diagnostics: Vec<PartialDiagnosticInput> = repo
            .get_diagnostics(project.id, None)?
            .into_iter()
            .map(|d| PartialDiagnosticInput {
                path: d.path,
                symbol: d.diagnostic.symbol,
                severity: d.diagnostic.severity,
            })
            .collect();
        let changes: Vec<(String, String)> = repo
            .get_symbol_changes(project.id)?
            .into_iter()
            .map(|c| (c.path, c.qualified_name))
            .collect();
        let relationships: Vec<ResolvedRelationship> = repo
            .get_relationships(project.id)?
            .into_iter()
            .map(|r| r.relationship)
            .collect();
        Ok(partial_type_groups(
            &symbols,
            &diagnostics,
            &changes,
            &relationships,
        ))
    }

    /// Commits touching a file (newest first) and the working-tree changes
    /// Inspectye recorded. With `symbol`, only versions where that symbol's
    /// code changed are listed.
    pub fn history(
        &self,
        start_dir: &Path,
        path: &str,
        symbol: Option<&str>,
        limit: usize,
    ) -> Result<HistoryReport> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let rel = to_project_relative(&root, path)?;
        let git_available = self.git.is_git_repository(&root);
        let commits = if git_available {
            self.git.file_history(&root, &rel, limit)?
        } else {
            Vec::new()
        };

        let symbol_changes: Vec<StoredSymbolChange> = repo
            .get_symbol_changes(project.id)?
            .into_iter()
            .filter(|c| c.path == rel)
            .collect();
        let recorded = match repo.get_file_by_path(project.id, &rel)? {
            Some(f) => repo
                .get_all_state_transitions(f.id)?
                .into_iter()
                .map(|t| RecordedChange {
                    recorded_at: t.created_at,
                    transition: t.transition_type,
                    added_lines: t.added_lines,
                    removed_lines: t.removed_lines,
                })
                .collect(),
            None => Vec::new(),
        };
        if commits.is_empty() && recorded.is_empty() && !root.join(&rel).is_file() {
            return Err(InspectyeError::FileNotFound(format!(
                "'{rel}' has no Git history and is not indexed."
            )));
        }

        let entries = match symbol {
            None => commits
                .into_iter()
                .map(|c| HistoryEntry {
                    commit: Some(c),
                    symbol_change: None,
                })
                .collect(),
            Some(query) => {
                let language = classify_path(Path::new(&rel)).1.ok_or_else(|| {
                    InspectyeError::Analysis(format!("'{rel}' is not a supported source file"))
                })?;
                // Oldest commit first, then the working tree.
                let mut versions: Vec<(String, Option<String>)> = Vec::new();
                for c in commits.iter().rev() {
                    versions.push((c.path.clone(), self.git.file_at(&root, &c.hash, &c.path)?));
                }
                versions.push((rel.clone(), std::fs::read_to_string(root.join(&rel)).ok()));
                let registry = LanguageRegistry::default();
                let oldest_first: Vec<Option<CommitInfo>> = commits
                    .iter()
                    .rev()
                    .cloned()
                    .map(Some)
                    .chain(std::iter::once(None))
                    .collect();
                let mut entries: Vec<HistoryEntry> =
                    trace_symbol(&registry, language, &versions, query)
                        .into_iter()
                        .map(|r| HistoryEntry {
                            commit: oldest_first[r.version].clone(),
                            symbol_change: Some(r),
                        })
                        .collect();
                entries.reverse();
                entries
            }
        };

        Ok(HistoryReport {
            path: rel,
            symbol: symbol.map(str::to_string),
            git_available,
            entries,
            recorded,
            symbol_changes,
        })
    }

    /// Compares a file between two revisions. A revision is a Git revision
    /// (`HEAD~3`, a hash, a tag) or `working` for the working tree; `to`
    /// defaults to the working tree. With `symbol`, only that symbol's code
    /// is compared, found by identity in each version.
    pub fn compare(
        &self,
        start_dir: &Path,
        path: &str,
        from: &str,
        to: Option<&str>,
        symbol: Option<&str>,
    ) -> Result<FileComparison> {
        let (root, _, _) = self.open_project(start_dir)?;
        let rel = to_project_relative(&root, path)?;
        let to = to.unwrap_or("working");
        let (from_label, old) = self.content_at(&root, &rel, from)?;
        let (to_label, new) = self.content_at(&root, &rel, to)?;

        let language = classify_path(Path::new(&rel)).1;
        let registry = LanguageRegistry::default();
        let old_symbols = language.and_then(|l| symbols_of(&registry, l, &rel, old.as_deref()));
        let new_symbols = language.and_then(|l| symbols_of(&registry, l, &rel, new.as_deref()));

        let (old_text, new_text, from_offset, to_offset) = match symbol {
            None => (
                old.clone().unwrap_or_default(),
                new.clone().unwrap_or_default(),
                0,
                0,
            ),
            Some(query) => {
                let pick = |symbols: &Option<Vec<ExtractedSymbol>>, content: &Option<String>| {
                    let s = symbols.as_deref().and_then(|s| find_symbol(s, query))?;
                    let text = content
                        .as_deref()?
                        .get(s.range.start_byte..s.range.end_byte)?;
                    Some((text.to_string(), s.range.start_line - 1))
                };
                let (o, of) = pick(&old_symbols, &old).unwrap_or_default();
                let (n, nf) = pick(&new_symbols, &new).unwrap_or_default();
                if o.is_empty() && n.is_empty() {
                    return Err(InspectyeError::FileNotFound(format!(
                        "Symbol '{query}' not found in either version of '{rel}'."
                    )));
                }
                (o, n, of, nf)
            }
        };

        let hunks = compute_line_diff(&old_text, &new_text);
        let (added, removed) = count_line_changes(&old_text, &new_text);
        let symbol_changes = match (&old_symbols, &new_symbols, symbol) {
            (Some(o), Some(n), None) => diff_symbols(o, n),
            _ => Vec::new(),
        };
        Ok(FileComparison {
            path: rel,
            from_label,
            to_label,
            symbol: symbol.map(str::to_string),
            from_line_offset: from_offset,
            to_line_offset: to_offset,
            hunks,
            added_lines: added as usize,
            removed_lines: removed as usize,
            symbol_changes,
        })
    }

    /// (label, content) of a file at a revision; content is `None` when the
    /// file does not exist there.
    fn content_at(
        &self,
        root: &Path,
        rel: &str,
        revision: &str,
    ) -> Result<(String, Option<String>)> {
        if matches!(revision, "working" | "worktree" | "WORKING") {
            return Ok((
                "working tree".to_string(),
                std::fs::read_to_string(root.join(rel)).ok(),
            ));
        }
        let hash = self.git.resolve_revision(root, revision).ok_or_else(|| {
            InspectyeError::Git(format!("'{revision}' is not a commit in this repository"))
        })?;
        // Follow renames: use the path the file had in that commit if known.
        let path_then = self
            .git
            .file_history(root, rel, 1000)?
            .into_iter()
            .find(|c| c.hash == hash)
            .map(|c| c.path)
            .unwrap_or_else(|| rel.to_string());
        let label = format!("{} ({})", revision, &hash[..hash.len().min(10)]);
        Ok((label, self.git.file_at(root, &hash, &path_then)?))
    }

    /// Ranked lists for the selected categories (all when empty).
    /// Rankings of the given categories (all when empty). With `paths`,
    /// only rows about files in the selection are kept.
    pub fn insights(
        &self,
        start_dir: &Path,
        categories: &[String],
        limit: usize,
        paths: &[String],
    ) -> Result<Vec<Ranking>> {
        if paths.is_empty() {
            return self.rankings(start_dir, categories, limit);
        }
        let root = self.find_project_root(start_dir)?;
        let filters = to_filters(&root, paths)?;
        let mut rankings = self.rankings(start_dir, categories, usize::MAX)?;
        for r in &mut rankings {
            // Rows without a file (partial-class groups) are kept as-is.
            r.rows.retain(|row| {
                row.path
                    .as_deref()
                    .map_or(true, |p| selects_any(&filters, p))
            });
            r.rows.truncate(limit);
        }
        Ok(rankings)
    }

    fn rankings(
        &self,
        start_dir: &Path,
        categories: &[String],
        limit: usize,
    ) -> Result<Vec<Ranking>> {
        let (root, repo, project) = self.open_project(start_dir)?;
        let wants = |c: &str| categories.is_empty() || categories.iter().any(|x| x == c);
        let mut out = Vec::new();

        if wants("changed") {
            let churn = if self.git.is_git_repository(&root) {
                self.git.churn(&root, GIT_HISTORY_WINDOW)?
            } else {
                Vec::new()
            };
            let existing: HashSet<String> = repo
                .get_all_files(project.id)?
                .into_iter()
                .filter(|f| f.exists_on_disk)
                .map(|f| f.path)
                .collect();
            let churn: Vec<_> = churn
                .into_iter()
                .filter(|c| existing.contains(&c.path))
                .collect();
            out.push(Ranking::new(
                "changed",
                "Most changed files (Git)",
                format!("lines added + deleted in the last {GIT_HISTORY_WINDOW} commits"),
                churn
                    .iter()
                    .map(|c| {
                        row(
                            &c.path,
                            c.added + c.deleted,
                            format!("+{} -{} in {} commit(s)", c.added, c.deleted, c.commits),
                        )
                    })
                    .collect(),
                limit,
            ));
            out.push(Ranking::new(
                "changed",
                "Most frequently changed files (Git)",
                format!("commits touching the file, of the last {GIT_HISTORY_WINDOW}"),
                churn.iter().map(|c| row(&c.path, c.commits, "")).collect(),
                limit,
            ));
            out.push(Ranking::new(
                "changed",
                "Most frequently changed files (working tree)",
                "content changes recorded by Inspectye scans",
                repo.recorded_change_counts(project.id)?
                    .into_iter()
                    .map(|(p, n)| row(p, n, ""))
                    .collect(),
                limit,
            ));
        }

        let diagnostics = if wants("diagnostics") || wants("symbols") {
            repo.get_diagnostics(project.id, None)?
        } else {
            Vec::new()
        };
        if wants("diagnostics") {
            let count_by_file = |pred: &dyn Fn(&StoredDiagnostic) -> bool| {
                let mut m: BTreeMap<&str, usize> = BTreeMap::new();
                for d in diagnostics.iter().filter(|d| pred(d)) {
                    *m.entry(d.path.as_str()).or_default() += 1;
                }
                m.into_iter()
                    .map(|(p, n)| row(p, n, ""))
                    .collect::<Vec<_>>()
            };
            out.push(Ranking::new(
                "diagnostics",
                "Files with most syntax errors",
                "syntax diagnostics of severity ERROR",
                count_by_file(&|d| {
                    d.diagnostic.category == DiagnosticCategory::Syntax
                        && d.diagnostic.severity == Severity::Error
                }),
                limit,
            ));
            out.push(Ranking::new(
                "diagnostics",
                "Files with most logic warnings",
                "logic findings of severity WARNING",
                count_by_file(&|d| {
                    d.diagnostic.category == DiagnosticCategory::Logic
                        && d.diagnostic.severity == Severity::Warning
                }),
                limit,
            ));
            out.push(Ranking::new(
                "diagnostics",
                "Files with most diagnostics",
                "all syntax and logic diagnostics",
                count_by_file(&|_| true),
                limit,
            ));
        }

        if wants("dependencies") {
            let metrics = self.dependency_metrics(start_dir)?;
            let rank = |title: &str, metric: &str, f: &dyn Fn(&FileDegree) -> usize| {
                Ranking::new(
                    "dependencies",
                    title,
                    metric,
                    metrics
                        .iter()
                        .map(|(p, d)| {
                            row(
                                p,
                                f(d),
                                format!(
                                    "{} dependents, {} dependencies",
                                    d.dependents, d.dependencies
                                ),
                            )
                        })
                        .collect(),
                    limit,
                )
            };
            out.push(rank(
                "Files with most dependents",
                "distinct project files depending on the file",
                &|d| d.dependents,
            ));
            out.push(rank(
                "Files with most dependencies",
                "distinct project files the file depends on",
                &|d| d.dependencies,
            ));
            out.push(rank("Dependency hubs", "dependents + dependencies", &|d| {
                d.total()
            }));
        }

        let symbols = if wants("symbols") || wants("size") || wants("complexity") {
            repo.get_project_symbols(project.id)?
        } else {
            Vec::new()
        };
        if wants("symbols") {
            let mut changes: BTreeMap<(String, String), usize> = BTreeMap::new();
            for c in repo
                .get_symbol_changes(project.id)?
                .into_iter()
                .filter(|c| c.change == "modified")
            {
                *changes.entry((c.path, c.qualified_name)).or_default() += 1;
            }
            out.push(Ranking::new(
                "symbols",
                "Most changed symbols",
                "recorded modifications of the symbol's code",
                changes
                    .into_iter()
                    .map(|((p, s), n)| row(format!("{s}  ({p})"), n, ""))
                    .collect(),
                limit,
            ));
            let mut by_symbol: BTreeMap<(String, String), usize> = BTreeMap::new();
            for d in &diagnostics {
                if let Some(s) = &d.diagnostic.symbol {
                    *by_symbol.entry((d.path.clone(), s.clone())).or_default() += 1;
                }
            }
            out.push(Ranking::new(
                "symbols",
                "Symbols with most diagnostics",
                "syntax and logic diagnostics inside the symbol",
                by_symbol
                    .into_iter()
                    .map(|((p, s), n)| row(format!("{s}  ({p})"), n, ""))
                    .collect(),
                limit,
            ));
        }

        if wants("size") {
            out.push(Ranking::new(
                "size",
                "Largest source files",
                "lines",
                repo.get_all_files(project.id)?
                    .into_iter()
                    .filter(|f| f.exists_on_disk && f.kind == "source")
                    .map(|f| row(f.path, f.line_count.unwrap_or(0) as usize, ""))
                    .collect(),
                limit,
            ));
            out.push(Ranking::new(
                "size",
                "Largest functions and methods",
                "lines of the declaration",
                symbols
                    .iter()
                    .filter(|s| is_callable(s.symbol.kind))
                    .map(|s| {
                        row(
                            format!("{}  ({})", s.symbol.qualified_name, s.path),
                            symbol_lines(s),
                            "",
                        )
                    })
                    .collect(),
                limit,
            ));
        }

        if wants("complexity") {
            out.push(Ranking::new(
                "complexity",
                "Most complex functions and methods",
                "cyclomatic complexity (1 + branches, loops, handlers, && / ||, conditional expressions, cases)",
                symbols
                    .iter()
                    .filter_map(|s| {
                        s.symbol.complexity.map(|c| {
                            row(
                                format!("{}  ({}:{})", s.symbol.qualified_name, s.path, s.symbol.range.start_line),
                                c,
                                format!("{} lines", symbol_lines(s)),
                            )
                        })
                    })
                    .collect(),
                limit,
            ));
        }

        if wants("partial-classes") {
            let groups = self.partial_types(start_dir)?;
            out.push(Ranking::new(
                "partial-classes",
                "Largest partial types",
                "lines across all parts",
                groups
                    .iter()
                    .map(|g| {
                        row(
                            &g.qualified_name,
                            g.lines,
                            format!("{} part(s), {} members", g.parts.len(), g.members),
                        )
                    })
                    .collect(),
                limit,
            ));
            out.push(Ranking::new(
                "partial-classes",
                "Partial types with most diagnostics",
                "errors + warnings in any part",
                groups
                    .iter()
                    .map(|g| {
                        row(
                            &g.qualified_name,
                            g.errors + g.warnings,
                            format!("{} error(s), {} warning(s)", g.errors, g.warnings),
                        )
                    })
                    .collect(),
                limit,
            ));
        }
        Ok(out)
    }

    /// Opens the project database that contains `start_dir`.
    pub(super) fn open_project(&self, start_dir: &Path) -> Result<(PathBuf, Repository, Project)> {
        let root = self.find_project_root(start_dir)?;
        let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);
        let repo = Repository::new(open_database(&db_path)?);
        let project = repo
            .get_project_by_root(&root)?
            .or_else(|| repo.get_first_project().ok().flatten())
            .ok_or_else(|| InspectyeError::NotInitialized(root.clone()))?;
        Ok((root, repo, project))
    }

    /// Stored intelligence about one file. Reads the index only.
    pub fn file_summary(&self, start_dir: &Path, path: &str) -> Result<FileSummary> {
        const RECENT: usize = 10;
        let (root, repo, project) = self.open_project(start_dir)?;
        let rel = to_project_relative(&root, path)?;
        let file = repo.get_file_by_path(project.id, &rel)?.ok_or_else(|| {
            InspectyeError::FileNotFound(format!(
                "'{rel}' is not indexed. Run 'inspecteye scan' first."
            ))
        })?;
        let analysis = repo.get_file_analysis(file.id)?;
        let analysis_current = analysis
            .as_ref()
            .is_some_and(|a| a.content_hash == file.content_hash);

        let symbols = repo.get_file_symbols(file.id)?;
        let diagnostics = repo
            .get_diagnostics(project.id, None)?
            .into_iter()
            .filter(|d| d.file_id == file.id)
            .collect();
        let (mut dependencies, mut dependents) = (Vec::new(), Vec::new());
        for r in repo.get_relationships(project.id)? {
            if r.relationship.source_file_id == file.id {
                dependencies.push(r);
            } else if r.relationship.target_file_id == Some(file.id) {
                dependents.push(r);
            }
        }
        let external_references = repo
            .get_external_references(project.id)?
            .into_iter()
            .filter(|e| e.file_id == file.id)
            .collect();
        let recent_changes = repo
            .get_all_state_transitions(file.id)?
            .into_iter()
            .rev()
            .take(RECENT)
            .map(|t| RecordedChange {
                recorded_at: t.created_at,
                transition: t.transition_type,
                added_lines: t.added_lines,
                removed_lines: t.removed_lines,
            })
            .collect();
        let recent_symbol_changes = repo
            .get_symbol_changes(project.id)?
            .into_iter()
            .filter(|c| c.file_id == file.id)
            .rev()
            .take(RECENT)
            .collect();

        Ok(FileSummary {
            file,
            analysis,
            analysis_current,
            symbols,
            diagnostics,
            dependencies,
            dependents,
            external_references,
            recent_changes,
            recent_symbol_changes,
        })
    }

    pub fn status(&self, start_dir: &Path) -> Result<StatusReport> {
        let root = self.find_project_root(start_dir)?;
        let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);

        let conn = open_database(&db_path)?;
        let repo = Repository::new(conn);

        let project = repo
            .get_project_by_root(&root)?
            .or_else(|| repo.get_first_project().ok().flatten())
            .ok_or_else(|| InspectyeError::NotInitialized(root.clone()))?;

        let stats = repo.get_file_stats(project.id)?;
        let last_scan = repo.get_latest_scan_run(project.id)?;

        let last_deep_scan = repo
            .get_latest_analysis_run_by_trigger(project.id, "deep_scan")
            .ok()
            .flatten();

        let git_active = self.git.is_git_repository(&root);

        Ok(StatusReport {
            project_name: project.name,
            root_path: root,
            db_path,
            stats,
            last_run: last_scan,
            last_deep_scan,
            git_active,
        })
    }

    /// The change of one file: pending working-tree edits if any, otherwise
    /// the last recorded transition. Includes the symbols the change touches.
    pub fn changes(&self, start_dir: &Path, file_path_str: &str) -> Result<StateTransition> {
        let transition = self.file_transition(start_dir, file_path_str)?;
        Ok(with_symbol_changes(transition))
    }

    fn file_transition(&self, start_dir: &Path, file_path_str: &str) -> Result<StateTransition> {
        let root = self.find_project_root(start_dir)?;
        let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);

        let conn = open_database(&db_path)?;
        let repo = Repository::new(conn);

        let project = repo
            .get_project_by_root(&root)?
            .or_else(|| repo.get_first_project().ok().flatten())
            .ok_or_else(|| InspectyeError::NotInitialized(root.clone()))?;

        let head_commit = self.git.get_head_commit(&root);

        let rel_str = to_project_relative(&root, file_path_str)?;

        let file_rec = repo
            .get_file_by_path(project.id, &rel_str)?
            .ok_or_else(|| {
                InspectyeError::FileNotFound(format!(
                    "File '{}' is not tracked by Inspectye. Run 'inspecteye scan' first.",
                    rel_str
                ))
            })?;

        let abs_path = root.join(&rel_str);
        let exists = abs_path.exists() && abs_path.is_file();

        if !exists {
            // File does not exist on disk
            let latest_state_rec = repo.get_latest_file_state(file_rec.id)?;
            if let Some(latest) = latest_state_rec {
                let pending = latest.exists_on_disk;
                let from_state = if latest.exists_on_disk {
                    FileState::from_record(&latest, rel_str.clone())
                } else {
                    let prev_active =
                        repo.find_file_state_by_hash(file_rec.id, &file_rec.content_hash)?;
                    prev_active
                        .map(|s| FileState::from_record(&s, rel_str.clone()))
                        .unwrap_or_else(|| FileState::from_record(&latest, rel_str.clone()))
                };
                let to_state = FileState {
                    id: None,
                    project_id: project.id,
                    file_id: file_rec.id,
                    path: rel_str,
                    content_hash: file_rec.content_hash,
                    recorded_at: Utc::now().to_rfc3339(),
                    source: "working_tree".to_string(),
                    git_commit: head_commit,
                    line_count: None,
                    size_bytes: 0,
                    content: None,
                    exists_on_disk: false,
                };
                let transition = StateTransition::removed(from_state, to_state);
                return Ok(if pending {
                    transition.into_pending()
                } else {
                    transition
                });
            } else {
                return Err(InspectyeError::FileNotFound(format!(
                    "File '{}' no longer exists and has no recorded state.",
                    rel_str
                )));
            }
        }

        // File exists on disk
        let current_hash = compute_file_hash(&abs_path)?;
        let meta = std::fs::metadata(&abs_path)?;
        let current_content = read_text_snapshot(&abs_path, meta.len() as i64);
        let latest_state_rec = repo.get_latest_file_state(file_rec.id)?;

        let latest_scan = repo.get_latest_scan_run(project.id)?;

        if let Some(latest) = latest_state_rec {
            if current_hash != latest.content_hash {
                // Working tree has uncommitted modifications!
                let prior_matching = repo.find_file_state_by_hash(file_rec.id, &current_hash)?;
                let restored_to =
                    prior_matching.map(|s| FileState::from_record(&s, rel_str.clone()));
                let transition_type = if restored_to.is_some() {
                    TransitionType::Restored
                } else {
                    TransitionType::Modified
                };

                let to_state = FileState {
                    id: None,
                    project_id: project.id,
                    file_id: file_rec.id,
                    path: rel_str.clone(),
                    content_hash: current_hash,
                    recorded_at: Utc::now().to_rfc3339(),
                    source: "working_tree".to_string(),
                    git_commit: head_commit,
                    line_count: current_content.as_ref().map(|c| c.lines().count()),
                    size_bytes: meta.len(),
                    content: current_content,
                    exists_on_disk: true,
                };

                return Ok(StateTransition::compute_transition(
                    FileState::from_record(&latest, rel_str),
                    to_state,
                    transition_type,
                    restored_to,
                )
                .into_pending());
            } else {
                // Disk matches latest recorded state in SQLite
                let latest_trans = repo.get_latest_state_transition(file_rec.id)?;
                if let Some(trans) = latest_trans {
                    let is_from_latest_run = latest_scan
                        .as_ref()
                        .map(|r| Some(r.id) == trans.analysis_run_id)
                        .unwrap_or(true);

                    match trans.transition_type.as_str() {
                        "new" => {
                            if is_from_latest_run {
                                return Ok(StateTransition::new_file(FileState::from_record(
                                    &latest, rel_str,
                                )));
                            } else {
                                return Ok(StateTransition::unchanged(FileState::from_record(
                                    &latest, rel_str,
                                )));
                            }
                        }
                        "unchanged" => {
                            return Ok(StateTransition::unchanged(FileState::from_record(
                                &latest, rel_str,
                            )));
                        }
                        "removed" => {
                            return Ok(StateTransition::removed(
                                FileState::from_record(&latest, rel_str.clone()),
                                FileState::from_record(&latest, rel_str),
                            ));
                        }
                        "modified" | "restored" | "restored_modified" => {
                            // A removal marker has no content; diff against the last
                            // state in which the file existed instead.
                            let from_state = match trans.from_state_id {
                                Some(from_id) => match repo.get_file_state_by_id(from_id)? {
                                    Some(s) if !s.exists_on_disk => repo
                                        .get_latest_existing_state_before(file_rec.id, s.id)?
                                        .or(Some(s)),
                                    other => other,
                                },
                                None => None,
                            }
                            .map(|s| FileState::from_record(&s, rel_str.clone()));
                            let restored_to = if let Some(r_id) = trans.restored_to_state_id {
                                repo.get_file_state_by_id(r_id)?
                                    .map(|s| FileState::from_record(&s, rel_str.clone()))
                            } else {
                                None
                            };
                            let t_type = TransitionType::from_str(&trans.transition_type);
                            if let Some(from) = from_state {
                                return Ok(StateTransition::compute_transition(
                                    from,
                                    FileState::from_record(&latest, rel_str),
                                    t_type,
                                    restored_to,
                                ));
                            } else {
                                return Ok(StateTransition::unchanged(FileState::from_record(
                                    &latest, rel_str,
                                )));
                            }
                        }
                        _ => {
                            return Ok(StateTransition::unchanged(FileState::from_record(
                                &latest, rel_str,
                            )));
                        }
                    }
                } else {
                    return Ok(StateTransition::unchanged(FileState::from_record(
                        &latest, rel_str,
                    )));
                }
            }
        } else {
            // No state recorded yet for this file
            let to_state = FileState {
                id: None,
                project_id: project.id,
                file_id: file_rec.id,
                path: rel_str,
                content_hash: current_hash,
                recorded_at: Utc::now().to_rfc3339(),
                source: "working_tree".to_string(),
                git_commit: head_commit,
                line_count: current_content.as_ref().map(|c| c.lines().count()),
                size_bytes: meta.len(),
                content: current_content,
                exists_on_disk: true,
            };
            return Ok(StateTransition::new_file(to_state).into_pending());
        }
    }
}

impl ResourceBreakdown {
    fn count(&mut self, kind: &str) {
        match kind {
            "source" => self.source += 1,
            "config" => self.config += 1,
            "documentation" => self.documentation += 1,
            "asset" => self.asset += 1,
            "dependency" => self.dependency += 1,
            "generated" => self.generated += 1,
            "binary" => self.binary += 1,
            _ => self.unknown += 1,
        }
    }
}

struct ScanContext<'a> {
    project_id: i64,
    run_id: i64,
    head_commit: Option<&'a str>,
    now: &'a str,
}

/// Records the state of a file present on disk and the transition from its
/// previous state. Returns the transition type; nothing is written for Unchanged.
fn record_present_file(
    repo: &mut Repository,
    ctx: &ScanContext,
    file: &FileRecord,
    previous: Option<&FileRecord>,
    hash: &str,
    abs_path: &Path,
    meta: &FileMetadata,
) -> Result<TransitionType> {
    let latest = repo.get_latest_file_state(file.id)?;
    let earlier_match = repo.find_file_state_by_hash(file.id, hash)?;

    let transition = match previous {
        None => TransitionType::New,
        Some(prev) if prev.exists_on_disk && prev.content_hash == hash => {
            // Backfill a baseline snapshot for files indexed before states existed.
            if latest.is_none() {
                insert_state(repo, ctx, file.id, hash, abs_path, meta)?;
            }
            return Ok(TransitionType::Unchanged);
        }
        Some(prev) => {
            let matches_earlier =
                earlier_match.is_some() || (!prev.exists_on_disk && prev.content_hash == hash);
            match (prev.exists_on_disk, matches_earlier) {
                (true, false) => TransitionType::Modified,
                (_, true) => TransitionType::Restored,
                (false, false) => TransitionType::RestoredModified,
            }
        }
    };

    let restored_to = if transition == TransitionType::Restored {
        earlier_match.map(|s| s.id)
    } else {
        None
    };
    let diff_base = match &latest {
        Some(s) if s.exists_on_disk => Some(s.clone()),
        Some(s) => repo.get_latest_existing_state_before(file.id, s.id)?,
        None => None,
    };

    let content = read_text_snapshot(abs_path, meta.size_bytes);
    let (added, removed) = match (
        diff_base.as_ref().and_then(|s| s.content.as_deref()),
        content.as_deref(),
    ) {
        (Some(old), Some(new)) => count_line_changes(old, new),
        _ if transition == TransitionType::New => (meta.line_count.unwrap_or(0), 0),
        _ => (0, 0),
    };
    let to_state_id = insert_state_with_content(repo, ctx, file.id, hash, meta, content)?;

    repo.insert_state_transition(&StateTransitionRecord {
        id: 0,
        project_id: ctx.project_id,
        file_id: file.id,
        analysis_run_id: Some(ctx.run_id),
        from_state_id: latest.map(|s| s.id),
        to_state_id,
        transition_type: transition.as_str().to_string(),
        restored_to_state_id: restored_to,
        added_lines: added,
        removed_lines: removed,
        created_at: ctx.now.to_string(),
    })?;
    Ok(transition)
}

fn record_removed_file(
    repo: &mut Repository,
    ctx: &ScanContext,
    removed: &FileRecord,
) -> Result<()> {
    let from_state = repo.get_latest_file_state(removed.id)?;
    let to_state_id = repo.insert_file_state(&FileStateRecord {
        id: 0,
        project_id: ctx.project_id,
        file_id: removed.id,
        content_hash: removed.content_hash.clone(),
        recorded_at: ctx.now.to_string(),
        source: "working_tree".to_string(),
        git_commit: ctx.head_commit.map(str::to_string),
        line_count: None,
        size_bytes: 0,
        content: None,
        exists_on_disk: false,
    })?;
    repo.insert_state_transition(&StateTransitionRecord {
        id: 0,
        project_id: ctx.project_id,
        file_id: removed.id,
        analysis_run_id: Some(ctx.run_id),
        from_state_id: from_state.map(|s| s.id),
        to_state_id,
        transition_type: TransitionType::Removed.as_str().to_string(),
        restored_to_state_id: None,
        added_lines: 0,
        removed_lines: removed.line_count.unwrap_or(0),
        created_at: ctx.now.to_string(),
    })?;
    Ok(())
}

fn insert_state(
    repo: &mut Repository,
    ctx: &ScanContext,
    file_id: i64,
    hash: &str,
    abs_path: &Path,
    meta: &FileMetadata,
) -> Result<i64> {
    let content = read_text_snapshot(abs_path, meta.size_bytes);
    insert_state_with_content(repo, ctx, file_id, hash, meta, content)
}

fn insert_state_with_content(
    repo: &mut Repository,
    ctx: &ScanContext,
    file_id: i64,
    hash: &str,
    meta: &FileMetadata,
    content: Option<String>,
) -> Result<i64> {
    repo.insert_file_state(&FileStateRecord {
        id: 0,
        project_id: ctx.project_id,
        file_id,
        content_hash: hash.to_string(),
        recorded_at: ctx.now.to_string(),
        source: "working_tree".to_string(),
        git_commit: ctx.head_commit.map(str::to_string),
        line_count: meta.line_count,
        size_bytes: meta.size_bytes,
        content,
        exists_on_disk: true,
    })
}

fn count_line_changes(old: &str, new: &str) -> (i64, i64) {
    let mut added = 0;
    let mut removed = 0;
    for hunk in compute_line_diff(old, new) {
        for line in hunk.lines {
            match line.tag {
                ChangeTag::Insert => added += 1,
                ChangeTag::Delete => removed += 1,
                ChangeTag::Equal => {}
            }
        }
    }
    (added, removed)
}

/// Adds the symbol-level diff between a transition's two states.
fn with_symbol_changes(mut t: StateTransition) -> StateTransition {
    let compares_versions = matches!(
        t.transition_type,
        TransitionType::Modified | TransitionType::Restored | TransitionType::RestoredModified
    );
    let (Some(old), Some(new)) = (
        t.from_state.as_ref().and_then(|s| s.content.clone()),
        t.to_state.content.clone(),
    ) else {
        return t;
    };
    let Some(language) = classify_path(Path::new(&t.path)).1 else {
        return t;
    };
    if !compares_versions {
        return t;
    }
    let registry = LanguageRegistry::default();
    let Some(lang) = registry.get(language) else {
        return t;
    };
    let path = Path::new(&t.path);
    if let (Ok(old_p), Ok(new_p)) = (
        registry.parse(language, path, old),
        registry.parse(language, path, new),
    ) {
        t.symbol_changes = diff_symbols(
            &extract_symbols(&old_p, lang),
            &extract_symbols(&new_p, lang),
        );
    }
    t
}

/// Persists everything derived from one file's analysis, replacing what was
/// stored for its previous content.
fn store_analysis_output(
    repo: &mut Repository,
    project_id: i64,
    file_id: i64,
    content_hash: &str,
    analysis: &FileAnalysis,
    run_id: Option<i64>,
    record_changes: bool,
) -> Result<()> {
    repo.replace_diagnostics(
        project_id,
        file_id,
        content_hash,
        DiagnosticCategory::Syntax,
        &analysis.syntax_diagnostics,
    )?;
    repo.replace_diagnostics(
        project_id,
        file_id,
        content_hash,
        DiagnosticCategory::Logic,
        &analysis.logic_diagnostics,
    )?;
    repo.sync_file_symbols(
        project_id,
        file_id,
        content_hash,
        run_id,
        &analysis.symbols,
        record_changes,
    )?;
    repo.replace_file_references(project_id, file_id, &analysis.references)?;
    repo.replace_external_references(
        project_id,
        file_id,
        content_hash,
        &analysis.external_references,
    )?;
    Ok(())
}

/// Evaluates every stored external reference on this machine and records
/// the result. Touches only the referenced paths (and their parents).
pub(super) fn check_external_references(
    repo: &mut Repository,
    project_id: i64,
    root: &Path,
) -> Result<(Vec<StoredExternalReference>, ExternalReferenceCounts)> {
    let mut refs = repo.get_external_references(project_id)?;
    let mut counts = ExternalReferenceCounts::default();
    let now = Utc::now().to_rfc3339();
    for r in &mut refs {
        let check = evaluate_external(
            &r.reference.path,
            r.reference.style,
            root,
            r.reference.in_test_code,
        );
        repo.update_external_check(r.id, &check)?;
        counts.total += 1;
        match check.severity {
            Severity::Error => counts.errors += 1,
            Severity::Warning => counts.warnings += 1,
            Severity::Info => counts.info += 1,
        }
        r.check = Some(check);
        r.checked_at = Some(now.clone());
    }
    Ok((refs, counts))
}

/// Rebuilds the project's relationships from stored references and symbols.
/// Runs over the whole project because a change in one file can alter what
/// other files' references resolve to; it reads only the database.
fn resolve_project(repo: &mut Repository, project_id: i64, root: &Path) -> Result<ResolutionStats> {
    let files = repo.get_file_nodes(project_id)?;
    let symbols = repo.get_symbol_nodes(project_id)?;
    let references = repo.get_reference_nodes(project_id)?;
    let (mut relationships, stats) =
        resolve_relationships(&files, &symbols, &references, &LanguageRegistry::default());
    // Build-output links are not code dependencies, so they are added after
    // `depends_on` aggregation.
    let path_ids: HashMap<String, i64> = files.iter().map(|f| (f.path.clone(), f.id)).collect();
    relationships.extend(source_map_links(
        root,
        &repo.get_generated_files(project_id)?,
        &path_ids,
    ));
    repo.replace_relationships(project_id, &relationships)?;
    Ok(stats)
}

/// Converts a user path (absolute, or relative to the project root) to the
/// project-relative, forward-slash form used in the index.
pub fn to_project_relative(root: &Path, path: &str) -> Result<String> {
    let p = Path::new(path);
    let rel = if p.is_absolute() {
        strip_root(&normalize_path(p), root).ok_or_else(|| {
            InspectyeError::FileNotFound(format!(
                "Path '{}' is outside the project root '{}'",
                path,
                root.display()
            ))
        })?
    } else {
        p.to_path_buf()
    };
    let mut s = rel.to_string_lossy().replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    Ok(s.trim_start_matches('/').trim_end_matches('/').to_string())
}

fn strip_root(path: &Path, root: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    // Windows paths compare case-insensitively; drive letters often differ in case.
    if cfg!(windows) {
        let (p, r) = (path.to_string_lossy(), root.to_string_lossy());
        if p.get(..r.len())?.eq_ignore_ascii_case(&r) {
            let rest = &p[r.len()..];
            // Require a separator so root `C:\foo` does not match `C:\foobar`.
            if rest.is_empty() || rest.starts_with(['\\', '/']) {
                return Some(PathBuf::from(rest.trim_start_matches(['\\', '/'])));
            }
        }
    }
    None
}

fn is_callable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

fn symbol_lines(s: &StoredSymbol) -> usize {
    s.symbol.range.end_line + 1 - s.symbol.range.start_line
}

pub(super) fn to_filters(root: &Path, paths: &[String]) -> Result<Vec<String>> {
    paths.iter().map(|p| to_project_relative(root, p)).collect()
}

/// True when no filters are given or any filter selects `path`.
pub(super) fn selects_any(filters: &[String], path: &str) -> bool {
    filters.is_empty() || filters.iter().any(|f| path_selects(f, path))
}

/// True when `filter` names `path` itself or a directory containing it.
fn path_selects(filter: &str, path: &str) -> bool {
    filter.is_empty()
        || filter == "."
        || path == filter
        || path
            .strip_prefix(filter)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn read_text_snapshot(path: &Path, size_bytes: i64) -> Option<String> {
    if size_bytes > 2 * 1024 * 1024 {
        None
    } else {
        std::fs::read_to_string(path).ok()
    }
}
