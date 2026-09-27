#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub root_path: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub id: i64,
    pub project_id: i64,
    pub path: String,
    pub kind: String,
    pub language: Option<String>,
    pub size_bytes: i64,
    pub line_count: Option<i64>,
    pub content_hash: String,
    pub git_tracked: bool,
    pub exists_on_disk: bool,
    pub is_ignored: bool,
    pub last_seen_at: String,
    pub last_analyzed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewFileRecord {
    pub project_id: i64,
    pub path: String,
    pub kind: String,
    pub language: Option<String>,
    pub size_bytes: i64,
    pub line_count: Option<i64>,
    pub content_hash: String,
    pub git_tracked: bool,
    pub exists_on_disk: bool,
    pub is_ignored: bool,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisRun {
    pub id: i64,
    pub project_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub trigger: String,
    pub files_scanned: i64,
    pub files_new: i64,
    pub files_changed: i64,
    pub files_unchanged: i64,
    pub files_restored: i64,
    pub files_restored_changed: i64,
    pub files_removed: i64,
    pub files_failed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAnalysisRecord {
    pub file_id: i64,
    pub content_hash: String,
    pub parser_version: Option<String>,
    pub analyzer_version: Option<String>,
    pub analyzed_at: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileStats {
    pub total_known_files: usize,
    pub discovered_files: usize,
    pub removed_files: usize,
    pub git_tracked_files: usize,
    pub untracked_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStateRecord {
    pub id: i64,
    pub project_id: i64,
    pub file_id: i64,
    pub content_hash: String,
    pub recorded_at: String,
    pub source: String,
    pub git_commit: Option<String>,
    pub line_count: Option<i64>,
    pub size_bytes: i64,
    pub content: Option<String>,
    pub exists_on_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransitionRecord {
    pub id: i64,
    pub project_id: i64,
    pub file_id: i64,
    pub analysis_run_id: Option<i64>,
    pub from_state_id: Option<i64>,
    pub to_state_id: i64,
    pub transition_type: String,
    pub restored_to_state_id: Option<i64>,
    pub added_lines: i64,
    pub removed_lines: i64,
    pub created_at: String,
}
