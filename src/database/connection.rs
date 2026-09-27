use crate::database::migrations::run_migrations;
use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn open_database(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut conn = Connection::open(db_path)?;
    configure_connection(&mut conn)?;
    run_migrations(&mut conn)?;

    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure_connection(&mut conn)?;
    run_migrations(&mut conn)?;
    Ok(conn)
}

fn configure_connection(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        "#,
    )?;
    Ok(())
}
