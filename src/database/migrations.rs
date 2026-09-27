use crate::error::{InspectyeError, Result};
use chrono::Utc;
use rusqlite::Connection;

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
        CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root_path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            path TEXT NOT NULL,
            kind TEXT NOT NULL,
            language TEXT,
            size_bytes INTEGER NOT NULL,
            line_count INTEGER,
            content_hash TEXT NOT NULL,
            git_tracked INTEGER NOT NULL DEFAULT 0,
            exists_on_disk INTEGER NOT NULL DEFAULT 1,
            last_seen_at TEXT NOT NULL,
            last_analyzed_at TEXT,
            UNIQUE(project_id, path)
        );

        CREATE INDEX IF NOT EXISTS idx_files_project_path ON files(project_id, path);
        CREATE INDEX IF NOT EXISTS idx_files_content_hash ON files(content_hash);
        CREATE INDEX IF NOT EXISTS idx_files_exists ON files(project_id, exists_on_disk);

        CREATE TABLE IF NOT EXISTS analysis_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT NOT NULL,
            trigger TEXT NOT NULL,
            files_scanned INTEGER NOT NULL DEFAULT 0,
            files_changed INTEGER NOT NULL DEFAULT 0,
            files_failed INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_analysis_runs_project ON analysis_runs(project_id, started_at DESC);

        CREATE TABLE IF NOT EXISTS file_analysis (
            file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
            content_hash TEXT NOT NULL,
            parser_version TEXT,
            analyzer_version TEXT,
            analyzed_at TEXT NOT NULL,
            status TEXT NOT NULL,
            error TEXT
        );
        "#,
    },
    Migration {
        version: 2,
        name: "add_scan_breakdown_to_analysis_runs",
        sql: r#"
        ALTER TABLE analysis_runs ADD COLUMN files_new INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE analysis_runs ADD COLUMN files_unchanged INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE analysis_runs ADD COLUMN files_removed INTEGER NOT NULL DEFAULT 0;
        "#,
    },
    Migration {
        version: 3,
        name: "add_restored_breakdown_to_analysis_runs",
        sql: r#"
        ALTER TABLE analysis_runs ADD COLUMN files_restored INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE analysis_runs ADD COLUMN files_restored_changed INTEGER NOT NULL DEFAULT 0;
        "#,
    },
    Migration {
        version: 4,
        name: "add_is_ignored_to_files",
        sql: r#"
        ALTER TABLE files ADD COLUMN is_ignored INTEGER NOT NULL DEFAULT 0;
        "#,
    },
    Migration {
        version: 5,
        name: "add_file_states_and_transitions",
        sql: r#"
        CREATE TABLE IF NOT EXISTS file_states (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            content_hash TEXT NOT NULL,
            recorded_at TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'working_tree',
            git_commit TEXT,
            line_count INTEGER,
            size_bytes INTEGER NOT NULL,
            content TEXT,
            exists_on_disk INTEGER NOT NULL DEFAULT 1
        );

        CREATE INDEX IF NOT EXISTS idx_file_states_file ON file_states(file_id, recorded_at);
        CREATE INDEX IF NOT EXISTS idx_file_states_hash ON file_states(file_id, content_hash);

        CREATE TABLE IF NOT EXISTS state_transitions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            analysis_run_id INTEGER REFERENCES analysis_runs(id) ON DELETE SET NULL,
            from_state_id INTEGER REFERENCES file_states(id) ON DELETE SET NULL,
            to_state_id INTEGER NOT NULL REFERENCES file_states(id) ON DELETE CASCADE,
            transition_type TEXT NOT NULL,
            restored_to_state_id INTEGER REFERENCES file_states(id) ON DELETE SET NULL,
            added_lines INTEGER NOT NULL DEFAULT 0,
            removed_lines INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_state_transitions_file ON state_transitions(file_id, created_at);
        "#,
    },
];

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        "#,
    )?;

    for migration in MIGRATIONS {
        let applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get(0),
            )
            .map_err(|e| {
                InspectyeError::Migration(format!("Failed to query migration status: {e}"))
            })?;

        if !applied {
            let tx = conn.transaction().map_err(|e| {
                InspectyeError::Migration(format!("Failed to start transaction: {e}"))
            })?;

            tx.execute_batch(migration.sql).map_err(|e| {
                InspectyeError::Migration(format!(
                    "Failed executing migration {} ({}): {e}",
                    migration.version, migration.name
                ))
            })?;

            let now = Utc::now().to_rfc3339();
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![migration.version, migration.name, now],
            )
            .map_err(|e| {
                InspectyeError::Migration(format!(
                    "Failed recording migration {} ({}): {e}",
                    migration.version, migration.name
                ))
            })?;

            tx.commit().map_err(|e| {
                InspectyeError::Migration(format!("Failed committing migration: {e}"))
            })?;
        }
    }

    Ok(())
}
