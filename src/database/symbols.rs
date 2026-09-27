use crate::database::models::{StoredSymbol, StoredSymbolChange};
use crate::database::Repository;
use crate::error::Result;
use crate::intelligence::{diff_symbols, ExtractedSymbol, SymbolChange, SymbolKind};
use crate::issues::SourceRange;
use chrono::Utc;
use rusqlite::{params, Row};
use std::collections::HashMap;

const SYMBOL_COLUMNS: &str = "s.id, s.file_id, f.path, s.parent_id, s.identity, s.kind, s.name, \
     s.qualified_name, s.signature, s.modifiers, s.start_line, s.start_column, s.end_line, \
     s.end_column, s.start_byte, s.end_byte, s.body_hash, s.complexity";

impl Repository {
    /// Brings a file's stored symbols in line with a fresh extraction.
    ///
    /// Symbols are matched by identity, so a symbol keeps its row id while it
    /// exists. Changes are recorded against `run_id` only when `record_changes`
    /// is set: a first extraction is a baseline, and an analyzer upgrade is not
    /// a code change.
    pub fn sync_file_symbols(
        &mut self,
        project_id: i64,
        file_id: i64,
        content_hash: &str,
        run_id: Option<i64>,
        symbols: &[ExtractedSymbol],
        record_changes: bool,
    ) -> Result<Vec<SymbolChange>> {
        let previous = self.get_file_symbols(file_id)?;
        let old: Vec<ExtractedSymbol> = previous.iter().map(|s| s.symbol.clone()).collect();
        let changes = diff_symbols(&old, symbols);
        let existing_ids: HashMap<&str, i64> = previous
            .iter()
            .map(|s| (s.symbol.identity.as_str(), s.id))
            .collect();

        let conn = self.connection_mut();
        let mut ids: Vec<i64> = Vec::with_capacity(symbols.len());
        for (ordinal, s) in symbols.iter().enumerate() {
            let parent_id = s.parent.map(|p| ids[p]);
            let modifiers = s.modifiers.join(" ");
            let r = &s.range;
            let id = match existing_ids.get(s.identity.as_str()) {
                Some(&id) => {
                    conn.prepare_cached(
                        r#"
                        UPDATE symbols SET parent_id = ?1, kind = ?2, name = ?3, qualified_name = ?4,
                            signature = ?5, modifiers = ?6, start_line = ?7, start_column = ?8,
                            end_line = ?9, end_column = ?10, start_byte = ?11, end_byte = ?12,
                            body_hash = ?13, ordinal = ?14, complexity = ?16
                        WHERE id = ?15
                        "#,
                    )?
                    .execute(params![
                        parent_id,
                        s.kind.as_str(),
                        s.name,
                        s.qualified_name,
                        s.signature,
                        modifiers,
                        r.start_line as i64,
                        r.start_column as i64,
                        r.end_line as i64,
                        r.end_column as i64,
                        r.start_byte as i64,
                        r.end_byte as i64,
                        s.body_hash,
                        ordinal as i64,
                        id,
                        s.complexity.map(|c| c as i64)
                    ])?;
                    id
                }
                None => {
                    conn.prepare_cached(
                        r#"
                        INSERT INTO symbols (
                            project_id, file_id, parent_id, identity, kind, name, qualified_name,
                            signature, modifiers, start_line, start_column, end_line, end_column,
                            start_byte, end_byte, body_hash, ordinal, complexity
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                        "#,
                    )?
                    .execute(params![
                        project_id,
                        file_id,
                        parent_id,
                        s.identity,
                        s.kind.as_str(),
                        s.name,
                        s.qualified_name,
                        s.signature,
                        modifiers,
                        r.start_line as i64,
                        r.start_column as i64,
                        r.end_line as i64,
                        r.end_column as i64,
                        r.start_byte as i64,
                        r.end_byte as i64,
                        s.body_hash,
                        ordinal as i64,
                        s.complexity.map(|c| c as i64)
                    ])?;
                    conn.last_insert_rowid()
                }
            };
            ids.push(id);
        }

        for stale in previous
            .iter()
            .filter(|p| !symbols.iter().any(|s| s.identity == p.symbol.identity))
        {
            conn.execute("DELETE FROM symbols WHERE id = ?1", params![stale.id])?;
        }

        if record_changes {
            let now = Utc::now().to_rfc3339();
            for c in &changes {
                conn.prepare_cached(
                    r#"
                    INSERT INTO symbol_changes (
                        project_id, file_id, analysis_run_id, identity, kind, qualified_name,
                        change, content_hash, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                    "#,
                )?
                .execute(params![
                    project_id,
                    file_id,
                    run_id,
                    c.identity,
                    c.kind.as_str(),
                    c.qualified_name,
                    c.change.as_str(),
                    content_hash,
                    now
                ])?;
            }
        }
        Ok(if record_changes { changes } else { Vec::new() })
    }

    pub fn delete_file_symbols(&mut self, file_id: i64) -> Result<()> {
        self.connection_mut()
            .execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    pub fn get_file_symbols(&self, file_id: i64) -> Result<Vec<StoredSymbol>> {
        let sql = format!(
            "SELECT {SYMBOL_COLUMNS} FROM symbols s JOIN files f ON f.id = s.file_id \
             WHERE s.file_id = ?1 ORDER BY s.ordinal"
        );
        let mut stmt = self.connection().prepare(&sql)?;
        let rows = stmt.query_map(params![file_id], map_symbol)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Symbols of all files currently on disk, ordered by path and position.
    pub fn get_project_symbols(&self, project_id: i64) -> Result<Vec<StoredSymbol>> {
        let sql = format!(
            "SELECT {SYMBOL_COLUMNS} FROM symbols s JOIN files f ON f.id = s.file_id \
             WHERE s.project_id = ?1 AND f.exists_on_disk = 1 ORDER BY f.path, s.ordinal"
        );
        let mut stmt = self.connection().prepare(&sql)?;
        let rows = stmt.query_map(params![project_id], map_symbol)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_symbol_changes(&self, project_id: i64) -> Result<Vec<StoredSymbolChange>> {
        let mut stmt = self.connection().prepare(
            r#"
            SELECT c.file_id, f.path, c.identity, c.kind, c.qualified_name, c.change, c.created_at
            FROM symbol_changes c JOIN files f ON f.id = c.file_id
            WHERE c.project_id = ?1
            ORDER BY c.id
            "#,
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(StoredSymbolChange {
                file_id: row.get(0)?,
                path: row.get(1)?,
                identity: row.get(2)?,
                kind: row.get(3)?,
                qualified_name: row.get(4)?,
                change: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}

fn map_symbol(row: &Row) -> rusqlite::Result<StoredSymbol> {
    let kind: String = row.get(5)?;
    let modifiers: String = row.get(9)?;
    let at = |i: usize| -> rusqlite::Result<usize> { Ok(row.get::<_, i64>(i)? as usize) };
    Ok(StoredSymbol {
        id: row.get(0)?,
        file_id: row.get(1)?,
        path: row.get(2)?,
        parent_id: row.get(3)?,
        symbol: ExtractedSymbol {
            identity: row.get(4)?,
            kind: SymbolKind::parse(&kind).unwrap_or(SymbolKind::Function),
            name: row.get(6)?,
            qualified_name: row.get(7)?,
            signature: row.get(8)?,
            modifiers: modifiers.split_whitespace().map(str::to_string).collect(),
            range: SourceRange {
                start_line: at(10)?,
                start_column: at(11)?,
                end_line: at(12)?,
                end_column: at(13)?,
                start_byte: at(14)?,
                end_byte: at(15)?,
            },
            body_hash: row.get(16)?,
            complexity: row.get::<_, Option<i64>>(17)?.map(|c| c as usize),
            parent: None,
        },
    })
}
