use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InspectyeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Project not initialized. Run 'inspecteye init' first. (Searched: {0})")]
    NotInitialized(PathBuf),

    #[error("Project already initialized at: {0}")]
    AlreadyInitialized(PathBuf),

    #[error("Invalid project location: {0}")]
    InvalidProjectLocation(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Database migration error: {0}")]
    Migration(String),

    #[error("File scan error on '{path}': {message}")]
    FileScan { path: PathBuf, message: String },

    #[error("File not found: {0}")]
    FileNotFound(String),
}

pub type Result<T> = std::result::Result<T, InspectyeError>;
