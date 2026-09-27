use crate::database::models::StoredDiagnostic;
use crate::database::Repository;
use crate::error::Result;
use crate::issues::{Certainty, Diagnostic, DiagnosticCategory, Severity, SourceRange};
use chrono::Utc;
use rusqlite::{params, Row};

impl Repository {
    /// Replaces a file's diagnostics of one category with a fresh set.
    pub fn replace_diagnostics(
        &mut self,
        project_id: i64,
        file_id: i64,
        content_hash: &str,
        category: DiagnosticCategory,
        diagnostics: &[Diagnostic],
    ) -> Result<()> {
        let conn = self.connection_mut();
        conn.execute(
            "DELETE FROM diagnostics WHERE file_id = ?1 AND category = ?2",
            params![file_id, category.as_str()],
        )?;
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare_cached(
            r#"
            INSERT INTO diagnostics (
                project_id, file_id, category, rule, severity, certainty, message,
                start_line, start_column, end_line, end_column, start_byte, end_byte,
                excerpt, expected, evidence, symbol, content_hash, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
        )?;
        for d in diagnostics {
            let expected = if d.expected.is_empty() {
                None
            } else {
                Some(d.expected.join("\n"))
            };
            stmt.execute(params![
                project_id,
                file_id,
                d.category.as_str(),
                d.rule,
                d.severity.as_str(),
                d.certainty.as_str(),
                d.message,
                d.range.start_line as i64,
                d.range.start_column as i64,
                d.range.end_line as i64,
                d.range.end_column as i64,
                d.range.start_byte as i64,
                d.range.end_byte as i64,
                d.excerpt,
                expected,
                d.evidence,
                d.symbol,
                content_hash,
                now,
            ])?;
        }
        Ok(())
    }

    /// Removes every diagnostic of a file (e.g. it was deleted or became ignored).
    pub fn delete_file_diagnostics(&mut self, file_id: i64) -> Result<()> {
        self.connection_mut().execute(
            "DELETE FROM diagnostics WHERE file_id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    /// Removes everything derived from analysing a file's content.
    pub fn delete_file_analysis_output(&mut self, file_id: i64) -> Result<()> {
        self.delete_file_diagnostics(file_id)?;
        self.delete_file_references(file_id)?;
        self.delete_file_external_references(file_id)?;
        self.delete_file_symbols(file_id)
    }

    /// Diagnostics of files currently on disk, ordered by path and position.
    pub fn get_diagnostics(
        &self,
        project_id: i64,
        category: Option<DiagnosticCategory>,
    ) -> Result<Vec<StoredDiagnostic>> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT d.id, d.file_id, f.path, d.content_hash, d.category, d.rule, d.severity,
                   d.certainty, d.message, d.start_line, d.start_column, d.end_line,
                   d.end_column, d.start_byte, d.end_byte, d.excerpt, d.expected,
                   d.evidence, d.symbol
            FROM diagnostics d
            JOIN files f ON f.id = d.file_id
            WHERE d.project_id = ?1 AND f.exists_on_disk = 1
              AND (?2 IS NULL OR d.category = ?2)
            ORDER BY f.path, d.start_byte, d.id
            "#,
        )?;
        let rows = stmt.query_map(params![project_id, category.map(|c| c.as_str())], map_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

fn map_row(row: &Row) -> rusqlite::Result<StoredDiagnostic> {
    let category: String = row.get(4)?;
    let severity: String = row.get(6)?;
    let certainty: String = row.get(7)?;
    let expected: Option<String> = row.get(16)?;
    let usize_at = |i: usize| -> rusqlite::Result<usize> { Ok(row.get::<_, i64>(i)? as usize) };
    Ok(StoredDiagnostic {
        id: row.get(0)?,
        file_id: row.get(1)?,
        path: row.get(2)?,
        content_hash: row.get(3)?,
        diagnostic: Diagnostic {
            category: DiagnosticCategory::parse(&category).unwrap_or(DiagnosticCategory::Syntax),
            rule: row.get(5)?,
            severity: Severity::parse(&severity).unwrap_or(Severity::Info),
            certainty: Certainty::parse(&certainty).unwrap_or(Certainty::Possibility),
            message: row.get(8)?,
            range: SourceRange {
                start_line: usize_at(9)?,
                start_column: usize_at(10)?,
                end_line: usize_at(11)?,
                end_column: usize_at(12)?,
                start_byte: usize_at(13)?,
                end_byte: usize_at(14)?,
            },
            excerpt: row.get(15)?,
            expected: expected
                .map(|e| e.lines().map(str::to_string).collect())
                .unwrap_or_default(),
            evidence: row.get(17)?,
            symbol: row.get(18)?,
        },
    })
}
