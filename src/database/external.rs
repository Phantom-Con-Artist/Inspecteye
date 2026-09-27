use crate::database::models::StoredExternalReference;
use crate::database::Repository;
use crate::error::Result;
use crate::intelligence::{Existence, ExternalCheck, ExternalReference, PathStyle};
use crate::issues::{Certainty, Severity, SourceRange};
use chrono::Utc;
use rusqlite::{params, Row};

impl Repository {
    /// Replaces a file's extracted external references. Their evaluation is
    /// stored separately by [`Repository::update_external_check`].
    pub fn replace_external_references(
        &mut self,
        project_id: i64,
        file_id: i64,
        content_hash: &str,
        refs: &[ExternalReference],
    ) -> Result<()> {
        let conn = self.connection_mut();
        conn.execute(
            "DELETE FROM external_references WHERE file_id = ?1",
            params![file_id],
        )?;
        let mut stmt = conn.prepare_cached(
            r#"
            INSERT INTO external_references (
                project_id, file_id, content_hash, raw_value, resolved_path, reference_type,
                context, start_line, start_column, end_line, end_column, start_byte, end_byte,
                symbol, in_test_code
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            "#,
        )?;
        for r in refs {
            stmt.execute(params![
                project_id,
                file_id,
                content_hash,
                r.raw,
                r.path,
                r.style.as_str(),
                r.context,
                r.range.start_line as i64,
                r.range.start_column as i64,
                r.range.end_line as i64,
                r.range.end_column as i64,
                r.range.start_byte as i64,
                r.range.end_byte as i64,
                r.symbol,
                r.in_test_code,
            ])?;
        }
        Ok(())
    }

    pub fn delete_file_external_references(&mut self, file_id: i64) -> Result<()> {
        self.connection_mut().execute(
            "DELETE FROM external_references WHERE file_id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    /// Records the result of checking one reference on this machine.
    pub fn update_external_check(&mut self, id: i64, check: &ExternalCheck) -> Result<()> {
        let (existence, is_directory, parent_exists, reason) = match &check.existence {
            Existence::Exists { is_directory } => ("exists", Some(*is_directory), None, None),
            Existence::Missing { parent_exists } => ("missing", None, Some(*parent_exists), None),
            Existence::Unchecked { reason } => ("unchecked", None, None, Some(reason.as_str())),
        };
        self.connection_mut()
            .prepare_cached(
                r#"
                UPDATE external_references SET
                    existence = ?2, is_directory = ?3, parent_exists = ?4, unchecked_reason = ?5,
                    inside_project = ?6, machine_specific = ?7, rule = ?8, severity = ?9,
                    certainty = ?10, message = ?11, last_checked_at = ?12
                WHERE id = ?1
                "#,
            )?
            .execute(params![
                id,
                existence,
                is_directory,
                parent_exists,
                reason,
                check.inside_repository,
                check.machine_specific,
                check.rule,
                check.severity.as_str(),
                check.certainty.as_str(),
                check.message,
                Utc::now().to_rfc3339(),
            ])?;
        Ok(())
    }

    /// External references of files currently on disk, by path and position.
    pub fn get_external_references(&self, project_id: i64) -> Result<Vec<StoredExternalReference>> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT e.id, e.file_id, f.path, e.raw_value, e.resolved_path, e.reference_type,
                   e.context, e.start_line, e.start_column, e.end_line, e.end_column,
                   e.start_byte, e.end_byte, e.symbol, e.existence, e.is_directory,
                   e.parent_exists, e.unchecked_reason, e.inside_project, e.machine_specific,
                   e.rule, e.severity, e.certainty, e.message, e.last_checked_at, e.in_test_code
            FROM external_references e
            JOIN files f ON f.id = e.file_id
            WHERE e.project_id = ?1 AND f.exists_on_disk = 1
            ORDER BY f.path, e.start_byte, e.id
            "#,
        )?;
        let rows = stmt.query_map(params![project_id], map_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

fn map_row(row: &Row) -> rusqlite::Result<StoredExternalReference> {
    let usize_at = |i: usize| -> rusqlite::Result<usize> { Ok(row.get::<_, i64>(i)? as usize) };
    let style: String = row.get(5)?;
    let reference = ExternalReference {
        raw: row.get(3)?,
        path: row.get(4)?,
        style: PathStyle::parse(&style).unwrap_or(PathStyle::Unix),
        context: row.get(6)?,
        range: SourceRange {
            start_line: usize_at(7)?,
            start_column: usize_at(8)?,
            end_line: usize_at(9)?,
            end_column: usize_at(10)?,
            start_byte: usize_at(11)?,
            end_byte: usize_at(12)?,
        },
        symbol: row.get(13)?,
        in_test_code: row.get(25)?,
    };
    let existence: Option<String> = row.get(14)?;
    let check = match existence.as_deref() {
        None => None,
        Some(state) => {
            let existence = match state {
                "exists" => Existence::Exists {
                    is_directory: row.get::<_, Option<bool>>(15)?.unwrap_or(false),
                },
                "missing" => Existence::Missing {
                    parent_exists: row.get::<_, Option<bool>>(16)?.unwrap_or(false),
                },
                _ => Existence::Unchecked {
                    reason: row.get::<_, Option<String>>(17)?.unwrap_or_default(),
                },
            };
            let severity: String = row.get(21)?;
            let certainty: String = row.get(22)?;
            Some(ExternalCheck {
                existence,
                inside_repository: row.get::<_, Option<bool>>(18)?.unwrap_or(false),
                machine_specific: row.get(19)?,
                rule: row.get::<_, Option<String>>(20)?.unwrap_or_default(),
                severity: Severity::parse(&severity).unwrap_or(Severity::Warning),
                certainty: Certainty::parse(&certainty).unwrap_or(Certainty::Possibility),
                message: row.get::<_, Option<String>>(23)?.unwrap_or_default(),
            })
        }
    };
    Ok(StoredExternalReference {
        id: row.get(0)?,
        file_id: row.get(1)?,
        file_path: row.get(2)?,
        reference,
        check,
        checked_at: row.get(24)?,
    })
}
