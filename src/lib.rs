pub mod analysis;
pub mod cli;
pub mod database;
pub mod error;
pub mod git;
pub mod intelligence;
pub mod issues;
pub mod project;
pub mod scanner;
pub mod watcher;

pub use error::{InspectyeError, Result};
pub use project::{InitResult, ProjectManager, ScanSummary, StatusReport};
