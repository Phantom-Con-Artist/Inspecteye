use crate::analysis::{
    compute_file_hash, DeepAnalyzer, FileState, StateTransition, TransitionType,
};
use crate::database::{
    open_database, AnalysisRun, FileAnalysisRecord, FileRecord, FileStateRecord, FileStats,
    NewFileRecord, Repository, StateTransitionRecord,
};
use crate::error::{InspectyeError, Result};
use crate::git::{GitCli, GitProvider};
use crate::scanner::{
    classify_resource, discover_files, inspect_file_metadata, InspectyeIgnore, PathFilter,
    IGNORE_FILE_NAME,
};
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const INSPECTYE_DIR: &str = ".inspectye";
pub const DB_NAME: &str = "inspectye.db";

pub fn normalize_path(path: &Path) -> PathBuf {
    let p_str = path.to_string_lossy();
    if let Some(stripped) = p_str.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

#[derive(Debug, Clone)]
pub struct InitResult {
    pub project_name: String,
    pub root_path: PathBuf,
    pub db_path: PathBuf,
    pub already_initialized: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct DeepScanSummary {
    pub project_name: String,
    pub project_root: PathBuf,
    pub total_eligible: usize,
    pub analyzed: usize,
    pub skipped: usize,
    pub ineligible: usize,
    pub failed: usize,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
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

        // Git tracked files
        let is_git = self.git.is_git_repository(&root);
        let tracked_files = if is_git {
            self.git.get_tracked_files(&root).unwrap_or_default()
        } else {
            Default::default()
        };

        // File discovery and ignore rules
        let filter = PathFilter::default();
        let ignore = InspectyeIgnore::from_file(&root.join(IGNORE_FILE_NAME));
        let discovered = discover_files(&root, &filter)?;
        let discovered_count = discovered.len();

        // Existing DB records
        let existing_files = repo.get_all_files(project.id)?;
        let existing_map: HashMap<String, FileRecord> = existing_files
            .into_iter()
            .map(|f| (f.path.clone(), f))
            .collect();

        let mut resources = ResourceBreakdown::default();
        let mut new_files = 0;
        let mut changed_files = 0;
        let mut unchanged_files = 0;
        let mut restored_files = 0;
        let mut restored_changed_files = 0;
        let mut failed_files = 0;
        let mut discovered_rel_paths = Vec::with_capacity(discovered_count);
        let now = Utc::now().to_rfc3339();

        for file in discovered {
            discovered_rel_paths.push(file.relative_path.clone());

            let hash = match compute_file_hash(&file.absolute_path) {
                Ok(h) => h,
                Err(_) => {
                    failed_files += 1;
                    continue;
                }
            };

            let meta = match inspect_file_metadata(&file.absolute_path) {
                Ok(m) => m,
                Err(_) => {
                    failed_files += 1;
                    continue;
                }
            };

            let is_tracked = tracked_files.contains(&file.relative_path);
            let is_ignored = ignore.is_ignored(&file.relative_path);
            let (kind, lang) =
                classify_resource(Path::new(&file.relative_path), &file.absolute_path);

            match kind.as_str() {
                "source" => resources.source += 1,
                "config" => resources.config += 1,
                "documentation" => resources.documentation += 1,
                "asset" => resources.asset += 1,
                "dependency" => resources.dependency += 1,
                "generated" => resources.generated += 1,
                "binary" => resources.binary += 1,
                _ => resources.unknown += 1,
            }

            let change_kind = if let Some(existing) = existing_map.get(&file.relative_path) {
                if existing.exists_on_disk {
                    if existing.content_hash == hash {
                        unchanged_files += 1;
                        "unchanged"
                    } else {
                        changed_files += 1;
                        "changed"
                    }
                } else {
                    if existing.content_hash == hash {
                        restored_files += 1;
                        "restored"
                    } else {
                        restored_changed_files += 1;
                        "restored_changed"
                    }
                }
            } else {
                new_files += 1;
                "new"
            };

            let new_record = NewFileRecord {
                project_id: project.id,
                path: file.relative_path.clone(),
                kind,
                language: lang,
                size_bytes: meta.size_bytes,
                line_count: meta.line_count,
                content_hash: hash.clone(),
                git_tracked: is_tracked,
                exists_on_disk: true,
                is_ignored,
                last_seen_at: now.clone(),
            };

            let upserted_file = match repo.upsert_file(&new_record) {
                Ok(f) => f,
                Err(_) => {
                    failed_files += 1;
                    continue;
                }
            };

            // Maintain file states and transitions
            let head_commit = self.git.get_head_commit(&root);
            match change_kind {
                "new" => {
                    let content = read_text_snapshot(&file.absolute_path, meta.size_bytes);
                    if let Ok(state_id) = repo.insert_file_state(&FileStateRecord {
                        id: 0,
                        project_id: project.id,
                        file_id: upserted_file.id,
                        content_hash: hash.clone(),
                        recorded_at: now.clone(),
                        source: "working_tree".to_string(),
                        git_commit: head_commit.clone(),
                        line_count: meta.line_count,
                        size_bytes: meta.size_bytes,
                        content,
                        exists_on_disk: true,
                    }) {
                        let _ = repo.insert_state_transition(&StateTransitionRecord {
                            id: 0,
                            project_id: project.id,
                            file_id: upserted_file.id,
                            analysis_run_id: Some(run_id),
                            from_state_id: None,
                            to_state_id: state_id,
                            transition_type: "new".to_string(),
                            restored_to_state_id: None,
                            added_lines: meta.line_count.unwrap_or(0),
                            removed_lines: 0,
                            created_at: now.clone(),
                        });
                    }
                }
                "unchanged" => {
                    if let Ok(None) = repo.get_latest_file_state(upserted_file.id) {
                        let content = read_text_snapshot(&file.absolute_path, meta.size_bytes);
                        let _ = repo.insert_file_state(&FileStateRecord {
                            id: 0,
                            project_id: project.id,
                            file_id: upserted_file.id,
                            content_hash: hash.clone(),
                            recorded_at: now.clone(),
                            source: "working_tree".to_string(),
                            git_commit: head_commit.clone(),
                            line_count: meta.line_count,
                            size_bytes: meta.size_bytes,
                            content,
                            exists_on_disk: true,
                        });
                    }
                }
                "changed" => {
                    let from_state = repo.get_latest_file_state(upserted_file.id).ok().flatten();
                    let prior_matching = repo
                        .find_file_state_by_hash(upserted_file.id, &hash)
                        .ok()
                        .flatten();
                    let (transition_type, restored_to_id) = if let Some(prior) = prior_matching {
                        ("restored", Some(prior.id))
                    } else {
                        ("modified", None)
                    };

                    let content = read_text_snapshot(&file.absolute_path, meta.size_bytes);
                    if let Ok(to_state_id) = repo.insert_file_state(&FileStateRecord {
                        id: 0,
                        project_id: project.id,
                        file_id: upserted_file.id,
                        content_hash: hash.clone(),
                        recorded_at: now.clone(),
                        source: "working_tree".to_string(),
                        git_commit: head_commit.clone(),
                        line_count: meta.line_count,
                        size_bytes: meta.size_bytes,
                        content,
                        exists_on_disk: true,
                    }) {
                        let _ = repo.insert_state_transition(&StateTransitionRecord {
                            id: 0,
                            project_id: project.id,
                            file_id: upserted_file.id,
                            analysis_run_id: Some(run_id),
                            from_state_id: from_state.map(|s| s.id),
                            to_state_id,
                            transition_type: transition_type.to_string(),
                            restored_to_state_id: restored_to_id,
                            added_lines: 0,
                            removed_lines: 0,
                            created_at: now.clone(),
                        });
                    }
                }
                "restored" => {
                    let from_state = repo.get_latest_file_state(upserted_file.id).ok().flatten();
                    let prior_matching = repo
                        .find_file_state_by_hash(upserted_file.id, &hash)
                        .ok()
                        .flatten();
                    let content = read_text_snapshot(&file.absolute_path, meta.size_bytes);
                    if let Ok(to_state_id) = repo.insert_file_state(&FileStateRecord {
                        id: 0,
                        project_id: project.id,
                        file_id: upserted_file.id,
                        content_hash: hash.clone(),
                        recorded_at: now.clone(),
                        source: "working_tree".to_string(),
                        git_commit: head_commit.clone(),
                        line_count: meta.line_count,
                        size_bytes: meta.size_bytes,
                        content,
                        exists_on_disk: true,
                    }) {
                        let _ = repo.insert_state_transition(&StateTransitionRecord {
                            id: 0,
                            project_id: project.id,
                            file_id: upserted_file.id,
                            analysis_run_id: Some(run_id),
                            from_state_id: from_state.map(|s| s.id),
                            to_state_id,
                            transition_type: "restored".to_string(),
                            restored_to_state_id: prior_matching.map(|s| s.id),
                            added_lines: 0,
                            removed_lines: 0,
                            created_at: now.clone(),
                        });
                    }
                }
                "restored_changed" => {
                    let from_state = repo.get_latest_file_state(upserted_file.id).ok().flatten();
                    let content = read_text_snapshot(&file.absolute_path, meta.size_bytes);
                    if let Ok(to_state_id) = repo.insert_file_state(&FileStateRecord {
                        id: 0,
                        project_id: project.id,
                        file_id: upserted_file.id,
                        content_hash: hash.clone(),
                        recorded_at: now.clone(),
                        source: "working_tree".to_string(),
                        git_commit: head_commit.clone(),
                        line_count: meta.line_count,
                        size_bytes: meta.size_bytes,
                        content,
                        exists_on_disk: true,
                    }) {
                        let _ = repo.insert_state_transition(&StateTransitionRecord {
                            id: 0,
                            project_id: project.id,
                            file_id: upserted_file.id,
                            analysis_run_id: Some(run_id),
                            from_state_id: from_state.map(|s| s.id),
                            to_state_id,
                            transition_type: "restored_modified".to_string(),
                            restored_to_state_id: None,
                            added_lines: 0,
                            removed_lines: 0,
                            created_at: now.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        let newly_removed_files =
            repo.mark_missing_files_as_removed(project.id, &discovered_rel_paths)?;
        let removed_files = newly_removed_files.len();
        let head_commit = self.git.get_head_commit(&root);

        for removed in newly_removed_files {
            let from_state = repo.get_latest_file_state(removed.id).ok().flatten();
            if let Ok(to_state_id) = repo.insert_file_state(&FileStateRecord {
                id: 0,
                project_id: project.id,
                file_id: removed.id,
                content_hash: removed.content_hash,
                recorded_at: now.clone(),
                source: "working_tree".to_string(),
                git_commit: head_commit.clone(),
                line_count: None,
                size_bytes: 0,
                content: None,
                exists_on_disk: false,
            }) {
                let _ = repo.insert_state_transition(&StateTransitionRecord {
                    id: 0,
                    project_id: project.id,
                    file_id: removed.id,
                    analysis_run_id: Some(run_id),
                    from_state_id: from_state.map(|s| s.id),
                    to_state_id,
                    transition_type: "removed".to_string(),
                    restored_to_state_id: None,
                    added_lines: 0,
                    removed_lines: removed.line_count.unwrap_or(0),
                    created_at: now.clone(),
                });
            }
        }

        let status = if failed_files > 0 && discovered_count == failed_files {
            "failed"
        } else {
            "completed"
        };

        repo.finish_analysis_run(
            run_id,
            status,
            discovered_count as i64,
            new_files as i64,
            changed_files as i64,
            unchanged_files as i64,
            restored_files as i64,
            restored_changed_files as i64,
            removed_files as i64,
            failed_files as i64,
        )?;

        let duration = start_time.elapsed();

        Ok(ScanSummary {
            project_name: project.name,
            project_root: root,
            files_discovered: discovered_count,
            resources,
            new_files,
            changed_files,
            unchanged_files,
            restored_files,
            restored_changed_files,
            removed_files,
            failed_files,
            duration,
        })
    }

    pub fn deep_scan<A: DeepAnalyzer>(
        &self,
        start_dir: &Path,
        analyzer: &A,
    ) -> Result<DeepScanSummary> {
        let start_time = Instant::now();
        let root = self.find_project_root(start_dir)?;
        let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);

        let conn = open_database(&db_path)?;
        let mut repo = Repository::new(conn);

        let project = repo
            .get_project_by_root(&root)?
            .or_else(|| repo.get_first_project().ok().flatten())
            .ok_or_else(|| InspectyeError::NotInitialized(root.clone()))?;

        let all_files = repo.get_all_files(project.id)?;

        let mut eligible_files = Vec::new();
        let mut ineligible_count = 0;

        for file in all_files {
            if analyzer.is_eligible(&file) {
                eligible_files.push(file);
            } else {
                ineligible_count += 1;
            }
        }

        let total_eligible = eligible_files.len();
        let mut analyzed = 0;
        let mut skipped = 0;
        let mut failed = 0;

        let run_id = repo.create_analysis_run(project.id, "deep_scan")?;

        for file in eligible_files {
            let existing_analysis = repo.get_file_analysis(file.id)?;

            let needs_analysis = match existing_analysis {
                Some(ref prev) => {
                    prev.content_hash != file.content_hash
                        || prev.status != "completed"
                        || prev.analyzer_version.as_deref() != Some(analyzer.analyzer_version())
                }
                None => true,
            };

            if !needs_analysis {
                skipped += 1;
                continue;
            }

            match analyzer.analyze(&root, &file) {
                Ok(result) => {
                    let now = Utc::now().to_rfc3339();
                    let record = FileAnalysisRecord {
                        file_id: result.file_id,
                        content_hash: result.content_hash,
                        parser_version: Some(result.parser_version),
                        analyzer_version: Some(result.analyzer_version),
                        analyzed_at: now.clone(),
                        status: result.status.clone(),
                        error: result.error,
                    };
                    repo.record_file_analysis(&record)?;
                    if result.status == "completed" {
                        repo.update_file_last_analyzed_at(file.id, &now)?;
                        analyzed += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(e) => {
                    let now = Utc::now().to_rfc3339();
                    let record = FileAnalysisRecord {
                        file_id: file.id,
                        content_hash: file.content_hash.clone(),
                        parser_version: Some(analyzer.parser_version().to_string()),
                        analyzer_version: Some(analyzer.analyzer_version().to_string()),
                        analyzed_at: now,
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                    };
                    repo.record_file_analysis(&record)?;
                    failed += 1;
                }
            }
        }

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

        let duration = start_time.elapsed();

        Ok(DeepScanSummary {
            project_name: project.name,
            project_root: root,
            total_eligible,
            analyzed,
            skipped,
            ineligible: ineligible_count,
            failed,
            duration,
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
        let last_scan = repo
            .get_latest_analysis_run_by_trigger(project.id, "cli")?
            .or_else(|| {
                repo.get_latest_analysis_run_by_trigger(project.id, "scan")
                    .ok()
                    .flatten()
            })
            .or_else(|| repo.get_latest_analysis_run(project.id).ok().flatten());

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

    pub fn changes(&self, start_dir: &Path, file_path_str: &str) -> Result<StateTransition> {
        let root = self.find_project_root(start_dir)?;
        let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);

        let conn = open_database(&db_path)?;
        let repo = Repository::new(conn);

        let project = repo
            .get_project_by_root(&root)?
            .or_else(|| repo.get_first_project().ok().flatten())
            .ok_or_else(|| InspectyeError::NotInitialized(root.clone()))?;

        let head_commit = self.git.get_head_commit(&root);

        // Normalize requested path to project-relative path
        let req_path = Path::new(file_path_str);
        let rel_path = if req_path.is_absolute() {
            req_path.strip_prefix(&root).map_err(|_| {
                InspectyeError::FileNotFound(format!(
                    "Path '{}' is outside the project root '{}'",
                    file_path_str,
                    root.display()
                ))
            })?
        } else {
            req_path
        };

        let mut rel_str = rel_path.to_string_lossy().replace('\\', "/");
        if rel_str.starts_with("./") {
            rel_str = rel_str[2..].to_string();
        }

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
                return Ok(StateTransition::removed(from_state, to_state));
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

        let latest_scan = repo
            .get_latest_analysis_run_by_trigger(project.id, "cli")?
            .or_else(|| {
                repo.get_latest_analysis_run_by_trigger(project.id, "scan")
                    .ok()
                    .flatten()
            })
            .or_else(|| repo.get_latest_analysis_run(project.id).ok().flatten());

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
                ));
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
                            let from_state = if let Some(from_id) = trans.from_state_id {
                                repo.get_file_state_by_id(from_id)?
                                    .map(|s| FileState::from_record(&s, rel_str.clone()))
                            } else {
                                None
                            };
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
            return Ok(StateTransition::new_file(to_state));
        }
    }
}

fn read_text_snapshot(path: &Path, size_bytes: i64) -> Option<String> {
    if size_bytes > 2 * 1024 * 1024 {
        None
    } else {
        std::fs::read_to_string(path).ok()
    }
}
