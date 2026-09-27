use crate::database::models::StoredRelationship;
use crate::database::Repository;
use crate::error::Result;
use crate::intelligence::{
    ExtractedReference, FileNode, ReferenceKind, ReferenceNode, RelationshipKind,
    ResolvedRelationship, SymbolKind, SymbolNode,
};
use crate::issues::{Certainty, SourceRange};
use rusqlite::params;

impl Repository {
    /// Replaces the raw references extracted from one file.
    pub fn replace_file_references(
        &mut self,
        project_id: i64,
        file_id: i64,
        references: &[ExtractedReference],
    ) -> Result<()> {
        let conn = self.connection_mut();
        conn.execute(
            "DELETE FROM code_references WHERE file_id = ?1",
            params![file_id],
        )?;
        let mut stmt = conn.prepare_cached(
            r#"
            INSERT INTO code_references (
                project_id, file_id, kind, target, detail, from_symbol,
                start_line, start_column, end_line, end_column, start_byte, end_byte
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
        )?;
        for r in references {
            stmt.execute(params![
                project_id,
                file_id,
                r.kind.as_str(),
                r.target,
                r.detail.join("\n"),
                r.from_symbol,
                r.range.start_line as i64,
                r.range.start_column as i64,
                r.range.end_line as i64,
                r.range.end_column as i64,
                r.range.start_byte as i64,
                r.range.end_byte as i64,
            ])?;
        }
        Ok(())
    }

    pub fn delete_file_references(&mut self, file_id: i64) -> Result<()> {
        self.connection_mut().execute(
            "DELETE FROM code_references WHERE file_id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    /// Files currently on disk, for resolution.
    pub fn get_file_nodes(&self, project_id: i64) -> Result<Vec<FileNode>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, path, language FROM files WHERE project_id = ?1 AND exists_on_disk = 1",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(FileNode {
                id: row.get(0)?,
                path: row.get(1)?,
                language: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// (id, path) of generated files currently on disk.
    pub fn get_generated_files(&self, project_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, path FROM files WHERE project_id = ?1 AND exists_on_disk = 1 AND kind = 'generated'",
        )?;
        let rows = stmt.query_map(params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_symbol_nodes(&self, project_id: i64) -> Result<Vec<SymbolNode>> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT s.id, s.file_id, s.identity, s.name, s.qualified_name, s.kind, s.modifiers
            FROM symbols s JOIN files f ON f.id = s.file_id
            WHERE s.project_id = ?1 AND f.exists_on_disk = 1
            "#,
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            let kind: String = row.get(5)?;
            let modifiers: String = row.get(6)?;
            Ok(SymbolNode {
                is_partial: modifiers.split_whitespace().any(|m| m == "partial"),
                id: row.get(0)?,
                file_id: row.get(1)?,
                identity: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                kind: SymbolKind::parse(&kind).unwrap_or(SymbolKind::Function),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_reference_nodes(&self, project_id: i64) -> Result<Vec<ReferenceNode>> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT r.file_id, r.kind, r.target, r.detail, r.from_symbol, r.start_line,
                   r.start_column, r.end_line, r.end_column, r.start_byte, r.end_byte
            FROM code_references r JOIN files f ON f.id = r.file_id
            WHERE r.project_id = ?1 AND f.exists_on_disk = 1
            ORDER BY r.file_id, r.start_byte
            "#,
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            let kind: String = row.get(1)?;
            let detail: String = row.get(3)?;
            let at = |i: usize| -> rusqlite::Result<usize> { Ok(row.get::<_, i64>(i)? as usize) };
            Ok(ReferenceNode {
                file_id: row.get(0)?,
                reference: ExtractedReference {
                    kind: ReferenceKind::parse(&kind).unwrap_or(ReferenceKind::TypeReference),
                    target: row.get(2)?,
                    detail: detail
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(str::to_string)
                        .collect(),
                    from_symbol: row.get(4)?,
                    range: SourceRange {
                        start_line: at(5)?,
                        start_column: at(6)?,
                        end_line: at(7)?,
                        end_column: at(8)?,
                        start_byte: at(9)?,
                        end_byte: at(10)?,
                    },
                },
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Replaces all derived relationships of a project.
    pub fn replace_relationships(
        &mut self,
        project_id: i64,
        relationships: &[ResolvedRelationship],
    ) -> Result<()> {
        let conn = self.connection_mut();
        conn.execute(
            "DELETE FROM relationships WHERE project_id = ?1",
            params![project_id],
        )?;
        let mut stmt = conn.prepare_cached(
            r#"
            INSERT INTO relationships (
                project_id, kind, source_file_id, source_symbol_id, target_file_id,
                target_symbol_id, target_name, certainty, line, evidence
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )?;
        for r in relationships {
            stmt.execute(params![
                project_id,
                r.kind.as_str(),
                r.source_file_id,
                r.source_symbol_id,
                r.target_file_id,
                r.target_symbol_id,
                r.target_name,
                r.certainty.as_str(),
                r.line as i64,
                r.evidence,
            ])?;
        }
        Ok(())
    }

    pub fn get_relationships(&self, project_id: i64) -> Result<Vec<StoredRelationship>> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT r.kind, r.source_file_id, r.source_symbol_id, r.target_file_id,
                   r.target_symbol_id, r.target_name, r.certainty, r.line, r.evidence,
                   sf.path, ss.qualified_name, tf.path, ts.qualified_name
            FROM relationships r
            JOIN files sf ON sf.id = r.source_file_id
            LEFT JOIN symbols ss ON ss.id = r.source_symbol_id
            LEFT JOIN files tf ON tf.id = r.target_file_id
            LEFT JOIN symbols ts ON ts.id = r.target_symbol_id
            WHERE r.project_id = ?1
            ORDER BY sf.path, r.line, r.id
            "#,
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            let kind: String = row.get(0)?;
            let certainty: String = row.get(6)?;
            Ok(StoredRelationship {
                relationship: ResolvedRelationship {
                    kind: RelationshipKind::parse(&kind).unwrap_or(RelationshipKind::References),
                    source_file_id: row.get(1)?,
                    source_symbol_id: row.get(2)?,
                    target_file_id: row.get(3)?,
                    target_symbol_id: row.get(4)?,
                    target_name: row.get(5)?,
                    certainty: Certainty::parse(&certainty).unwrap_or(Certainty::Possibility),
                    line: row.get::<_, i64>(7)? as usize,
                    evidence: row.get(8)?,
                },
                source_path: row.get(9)?,
                source_symbol: row.get(10)?,
                target_path: row.get(11)?,
                target_symbol: row.get(12)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}
