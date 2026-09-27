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

    #[error("Analysis error: {0}")]
    Analysis(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
}

impl InspectyeError {
    /// Stable machine-readable name of the error kind (used in JSON output).
    pub fn kind(&self) -> &'static str {
        match self {
            InspectyeError::Io(_) => "io",
            InspectyeError::Database(_) => "database",
            InspectyeError::NotInitialized(_) => "not_initialized",
            InspectyeError::AlreadyInitialized(_) => "already_initialized",
            InspectyeError::InvalidProjectLocation(_) => "invalid_project_location",
            InspectyeError::Git(_) => "git",
            InspectyeError::Migration(_) => "migration",
            InspectyeError::FileScan { .. } => "file_scan",
            InspectyeError::FileNotFound(_) => "file_not_found",
            InspectyeError::Analysis(_) => "analysis",
            InspectyeError::NotFound(_) => "not_found",
            InspectyeError::InvalidArgument(_) => "invalid_argument",
        }
    }
}

pub type Result<T> = std::result::Result<T, InspectyeError>;
