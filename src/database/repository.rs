use crate::database::models::{
    AnalysisRun, FileAnalysisRecord, FileRecord, FileStateRecord, FileStats, NewFileRecord,
    Project, StateTransitionRecord,
};
use crate::error::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Repository {
    conn: Connection,
}

impl Repository {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn get_or_create_project(&mut self, root_path: &Path, name: &str) -> Result<Project> {
        let root_str = root_path.to_string_lossy().to_string();
        let now = Utc::now().to_rfc3339();

        let existing: Option<Project> = self
            .conn
            .query_row(
                "SELECT id, root_path, name, created_at, updated_at FROM projects WHERE root_path = ?1",
                params![root_str],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        root_path: row.get(1)?,
                        name: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?;

        if let Some(mut project) = existing {
            self.conn.execute(
                "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, project.id],
            )?;
            project.name = name.to_string();
            project.updated_at = now;
            Ok(project)
        } else {
            self.conn.execute(
                "INSERT INTO projects (root_path, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![root_str, name, now, now],
            )?;
            let id = self.conn.last_insert_rowid();
            Ok(Project {
                id,
                root_path: root_str,
                name: name.to_string(),
                created_at: now.clone(),
                updated_at: now,
            })
        }
    }

    pub fn get_project_by_root(&self, root_path: &Path) -> Result<Option<Project>> {
        let root_str = root_path.to_string_lossy().to_string();
        let project = self
            .conn
            .query_row(
                "SELECT id, root_path, name, created_at, updated_at FROM projects WHERE root_path = ?1 COLLATE NOCASE",
                params![root_str],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        root_path: row.get(1)?,
                        name: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?;

        Ok(project)
    }

    pub fn get_first_project(&self) -> Result<Option<Project>> {
        let project = self
            .conn
            .query_row(
                "SELECT id, root_path, name, created_at, updated_at FROM projects ORDER BY id ASC LIMIT 1",
                [],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        root_path: row.get(1)?,
                        name: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?;

        Ok(project)
    }

    pub fn get_all_files(&self, project_id: i64) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, project_id, path, kind, language, size_bytes, line_count,
                   content_hash, git_tracked, exists_on_disk, is_ignored, last_seen_at, last_analyzed_at
            FROM files
            WHERE project_id = ?1
            ORDER BY path ASC
            "#,
        )?;

        let rows = stmt.query_map(params![project_id], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                path: row.get(2)?,
                kind: row.get(3)?,
                language: row.get(4)?,
                size_bytes: row.get(5)?,
                line_count: row.get(6)?,
                content_hash: row.get(7)?,
                git_tracked: row.get::<_, i64>(8)? != 0,
                exists_on_disk: row.get::<_, i64>(9)? != 0,
                is_ignored: row.get::<_, i64>(10)? != 0,
                last_seen_at: row.get(11)?,
                last_analyzed_at: row.get(12)?,
            })
        })?;

        let mut files = Vec::new();
        for file in rows {
            files.push(file?);
        }
        Ok(files)
    }

    pub fn get_file_by_path(&self, project_id: i64, path: &str) -> Result<Option<FileRecord>> {
        let forward = path.replace('\\', "/");
        let back = path.replace('/', "\\");
        let file = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, path, kind, language, size_bytes, line_count,
                       content_hash, git_tracked, exists_on_disk, is_ignored, last_seen_at, last_analyzed_at
                FROM files
                WHERE project_id = ?1 AND (path = ?2 OR path = ?3)
                "#,
                params![project_id, forward, back],
                |row| {
                    Ok(FileRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        path: row.get(2)?,
                        kind: row.get(3)?,
                        language: row.get(4)?,
                        size_bytes: row.get(5)?,
                        line_count: row.get(6)?,
                        content_hash: row.get(7)?,
                        git_tracked: row.get::<_, i64>(8)? != 0,
                        exists_on_disk: row.get::<_, i64>(9)? != 0,
                        is_ignored: row.get::<_, i64>(10)? != 0,
                        last_seen_at: row.get(11)?,
                        last_analyzed_at: row.get(12)?,
                    })
                },
            )
            .optional()?;

        Ok(file)
    }

    pub fn upsert_file(&mut self, record: &NewFileRecord) -> Result<FileRecord> {
        self.conn.execute(
            r#"
            INSERT INTO files (
                project_id, path, kind, language, size_bytes, line_count,
                content_hash, git_tracked, exists_on_disk, is_ignored, last_seen_at, last_analyzed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)
            ON CONFLICT(project_id, path) DO UPDATE SET
                kind = excluded.kind,
                language = excluded.language,
                size_bytes = excluded.size_bytes,
                line_count = excluded.line_count,
                content_hash = excluded.content_hash,
                git_tracked = excluded.git_tracked,
                exists_on_disk = excluded.exists_on_disk,
                is_ignored = excluded.is_ignored,
                last_seen_at = excluded.last_seen_at
            "#,
            params![
                record.project_id,
                record.path,
                record.kind,
                record.language,
                record.size_bytes,
                record.line_count,
                record.content_hash,
                if record.git_tracked { 1 } else { 0 },
                if record.exists_on_disk { 1 } else { 0 },
                if record.is_ignored { 1 } else { 0 },
                record.last_seen_at,
            ],
        )?;

        let file = self
            .get_file_by_path(record.project_id, &record.path)?
            .expect("file must exist after upsert");

        Ok(file)
    }

    pub fn mark_missing_files_as_removed(
        &mut self,
        project_id: i64,
        discovered_paths: &[String],
    ) -> Result<Vec<FileRecord>> {
        let all_files = self.get_all_files(project_id)?;
        let mut removed = Vec::new();

        let active_set: std::collections::HashSet<&str> =
            discovered_paths.iter().map(|s| s.as_str()).collect();

        for file in all_files {
            if file.exists_on_disk && !active_set.contains(file.path.as_str()) {
                self.conn.execute(
                    "UPDATE files SET exists_on_disk = 0 WHERE id = ?1",
                    params![file.id],
                )?;
                removed.push(file);
            }
        }

        Ok(removed)
    }

    pub fn create_analysis_run(&mut self, project_id: i64, trigger: &str) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"
            INSERT INTO analysis_runs (
                project_id, started_at, finished_at, status, trigger,
                files_scanned, files_changed, files_failed
            ) VALUES (?1, ?2, NULL, 'running', ?3, 0, 0, 0)
            "#,
            params![project_id, now, trigger],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_analysis_run(
        &mut self,
        run_id: i64,
        status: &str,
        files_scanned: i64,
        files_new: i64,
        files_changed: i64,
        files_unchanged: i64,
        files_restored: i64,
        files_restored_changed: i64,
        files_removed: i64,
        files_failed: i64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"
            UPDATE analysis_runs SET
                finished_at = ?1,
                status = ?2,
                files_scanned = ?3,
                files_new = ?4,
                files_changed = ?5,
                files_unchanged = ?6,
                files_restored = ?7,
                files_restored_changed = ?8,
                files_removed = ?9,
                files_failed = ?10
            WHERE id = ?11
            "#,
            params![
                now,
                status,
                files_scanned,
                files_new,
                files_changed,
                files_unchanged,
                files_restored,
                files_restored_changed,
                files_removed,
                files_failed,
                run_id
            ],
        )?;
        Ok(())
    }

    pub fn get_latest_analysis_run(&self, project_id: i64) -> Result<Option<AnalysisRun>> {
        let run = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, started_at, finished_at, status, trigger,
                       files_scanned, files_new, files_changed, files_unchanged,
                       files_restored, files_restored_changed, files_removed, files_failed
                FROM analysis_runs
                WHERE project_id = ?1
                ORDER BY id DESC
                LIMIT 1
                "#,
                params![project_id],
                |row| {
                    Ok(AnalysisRun {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        started_at: row.get(2)?,
                        finished_at: row.get(3)?,
                        status: row.get(4)?,
                        trigger: row.get(5)?,
                        files_scanned: row.get(6)?,
                        files_new: row.get(7)?,
                        files_changed: row.get(8)?,
                        files_unchanged: row.get(9)?,
                        files_restored: row.get(10)?,
                        files_restored_changed: row.get(11)?,
                        files_removed: row.get(12)?,
                        files_failed: row.get(13)?,
                    })
                },
            )
            .optional()?;

        Ok(run)
    }

    pub fn get_latest_analysis_run_by_trigger(
        &self,
        project_id: i64,
        trigger_pattern: &str,
    ) -> Result<Option<AnalysisRun>> {
        let run = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, started_at, finished_at, status, trigger,
                       files_scanned, files_new, files_changed, files_unchanged,
                       files_restored, files_restored_changed, files_removed, files_failed
                FROM analysis_runs
                WHERE project_id = ?1 AND trigger LIKE ?2
                ORDER BY id DESC
                LIMIT 1
                "#,
                params![project_id, trigger_pattern],
                |row| {
                    Ok(AnalysisRun {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        started_at: row.get(2)?,
                        finished_at: row.get(3)?,
                        status: row.get(4)?,
                        trigger: row.get(5)?,
                        files_scanned: row.get(6)?,
                        files_new: row.get(7)?,
                        files_changed: row.get(8)?,
                        files_unchanged: row.get(9)?,
                        files_restored: row.get(10)?,
                        files_restored_changed: row.get(11)?,
                        files_removed: row.get(12)?,
                        files_failed: row.get(13)?,
                    })
                },
            )
            .optional()?;

        Ok(run)
    }

    pub fn get_file_stats(&self, project_id: i64) -> Result<FileStats> {
        let total_known: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let existing: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE project_id = ?1 AND exists_on_disk = 1",
            params![project_id],
            |row| row.get(0),
        )?;

        let removed = total_known.saturating_sub(existing);

        let git_tracked: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE project_id = ?1 AND exists_on_disk = 1 AND git_tracked = 1",
            params![project_id],
            |row| row.get(0),
        )?;

        let untracked = existing.saturating_sub(git_tracked);

        Ok(FileStats {
            total_known_files: total_known,
            discovered_files: existing,
            removed_files: removed,
            git_tracked_files: git_tracked,
            untracked_files: untracked,
        })
    }

    pub fn record_file_analysis(&mut self, record: &FileAnalysisRecord) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO file_analysis (
                file_id, content_hash, parser_version, analyzer_version, analyzed_at, status, error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(file_id) DO UPDATE SET
                content_hash = excluded.content_hash,
                parser_version = excluded.parser_version,
                analyzer_version = excluded.analyzer_version,
                analyzed_at = excluded.analyzed_at,
                status = excluded.status,
                error = excluded.error
            "#,
            params![
                record.file_id,
                record.content_hash,
                record.parser_version,
                record.analyzer_version,
                record.analyzed_at,
                record.status,
                record.error,
            ],
        )?;
        Ok(())
    }

    pub fn get_file_analysis(&self, file_id: i64) -> Result<Option<FileAnalysisRecord>> {
        let record = self
            .conn
            .query_row(
                r#"
                SELECT file_id, content_hash, parser_version, analyzer_version, analyzed_at, status, error
                FROM file_analysis
                WHERE file_id = ?1
                "#,
                params![file_id],
                |row| {
                    Ok(FileAnalysisRecord {
                        file_id: row.get(0)?,
                        content_hash: row.get(1)?,
                        parser_version: row.get(2)?,
                        analyzer_version: row.get(3)?,
                        analyzed_at: row.get(4)?,
                        status: row.get(5)?,
                        error: row.get(6)?,
                    })
                },
            )
            .optional()?;

        Ok(record)
    }

    pub fn update_file_last_analyzed_at(&mut self, file_id: i64, timestamp: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET last_analyzed_at = ?1 WHERE id = ?2",
            params![timestamp, file_id],
        )?;
        Ok(())
    }

    pub fn insert_file_state(&mut self, state: &FileStateRecord) -> Result<i64> {
        self.conn.execute(
            r#"
            INSERT INTO file_states (
                project_id, file_id, content_hash, recorded_at, source,
                git_commit, line_count, size_bytes, content, exists_on_disk
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                state.project_id,
                state.file_id,
                state.content_hash,
                state.recorded_at,
                state.source,
                state.git_commit,
                state.line_count,
                state.size_bytes,
                state.content,
                if state.exists_on_disk { 1 } else { 0 },
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_latest_file_state(&self, file_id: i64) -> Result<Option<FileStateRecord>> {
        let state = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, file_id, content_hash, recorded_at, source,
                       git_commit, line_count, size_bytes, content, exists_on_disk
                FROM file_states
                WHERE file_id = ?1
                ORDER BY id DESC
                LIMIT 1
                "#,
                params![file_id],
                |row| {
                    Ok(FileStateRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        file_id: row.get(2)?,
                        content_hash: row.get(3)?,
                        recorded_at: row.get(4)?,
                        source: row.get(5)?,
                        git_commit: row.get(6)?,
                        line_count: row.get(7)?,
                        size_bytes: row.get(8)?,
                        content: row.get(9)?,
                        exists_on_disk: row.get::<_, i64>(10)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(state)
    }

    pub fn get_file_state_by_id(&self, state_id: i64) -> Result<Option<FileStateRecord>> {
        let state = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, file_id, content_hash, recorded_at, source,
                       git_commit, line_count, size_bytes, content, exists_on_disk
                FROM file_states
                WHERE id = ?1
                "#,
                params![state_id],
                |row| {
                    Ok(FileStateRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        file_id: row.get(2)?,
                        content_hash: row.get(3)?,
                        recorded_at: row.get(4)?,
                        source: row.get(5)?,
                        git_commit: row.get(6)?,
                        line_count: row.get(7)?,
                        size_bytes: row.get(8)?,
                        content: row.get(9)?,
                        exists_on_disk: row.get::<_, i64>(10)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(state)
    }

    pub fn find_file_state_by_hash(
        &self,
        file_id: i64,
        content_hash: &str,
    ) -> Result<Option<FileStateRecord>> {
        let state = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, file_id, content_hash, recorded_at, source,
                       git_commit, line_count, size_bytes, content, exists_on_disk
                FROM file_states
                WHERE file_id = ?1 AND content_hash = ?2 AND exists_on_disk = 1
                ORDER BY id ASC
                LIMIT 1
                "#,
                params![file_id, content_hash],
                |row| {
                    Ok(FileStateRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        file_id: row.get(2)?,
                        content_hash: row.get(3)?,
                        recorded_at: row.get(4)?,
                        source: row.get(5)?,
                        git_commit: row.get(6)?,
                        line_count: row.get(7)?,
                        size_bytes: row.get(8)?,
                        content: row.get(9)?,
                        exists_on_disk: row.get::<_, i64>(10)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(state)
    }

    pub fn insert_state_transition(&mut self, transition: &StateTransitionRecord) -> Result<i64> {
        self.conn.execute(
            r#"
            INSERT INTO state_transitions (
                project_id, file_id, analysis_run_id, from_state_id, to_state_id, transition_type,
                restored_to_state_id, added_lines, removed_lines, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                transition.project_id,
                transition.file_id,
                transition.analysis_run_id,
                transition.from_state_id,
                transition.to_state_id,
                transition.transition_type,
                transition.restored_to_state_id,
                transition.added_lines,
                transition.removed_lines,
                transition.created_at,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_latest_state_transition(
        &self,
        file_id: i64,
    ) -> Result<Option<StateTransitionRecord>> {
        let record = self
            .conn
            .query_row(
                r#"
                SELECT id, project_id, file_id, analysis_run_id, from_state_id, to_state_id, transition_type,
                       restored_to_state_id, added_lines, removed_lines, created_at
                FROM state_transitions
                WHERE file_id = ?1
                ORDER BY id DESC
                LIMIT 1
                "#,
                params![file_id],
                |row| {
                    Ok(StateTransitionRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        file_id: row.get(2)?,
                        analysis_run_id: row.get(3)?,
                        from_state_id: row.get(4)?,
                        to_state_id: row.get(5)?,
                        transition_type: row.get(6)?,
                        restored_to_state_id: row.get(7)?,
                        added_lines: row.get(8)?,
                        removed_lines: row.get(9)?,
                        created_at: row.get(10)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    pub fn get_all_state_transitions(&self, file_id: i64) -> Result<Vec<StateTransitionRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, project_id, file_id, analysis_run_id, from_state_id, to_state_id, transition_type,
                   restored_to_state_id, added_lines, removed_lines, created_at
            FROM state_transitions
            WHERE file_id = ?1
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok(StateTransitionRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                file_id: row.get(2)?,
                analysis_run_id: row.get(3)?,
                from_state_id: row.get(4)?,
                to_state_id: row.get(5)?,
                transition_type: row.get(6)?,
                restored_to_state_id: row.get(7)?,
                added_lines: row.get(8)?,
                removed_lines: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }
}
