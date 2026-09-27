mod diagnostic;

pub use diagnostic::{render_excerpt, Certainty, Diagnostic, DiagnosticCategory, SourceRange};
use serde::Serialize;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Error => "ERROR",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "INFO" => Some(Severity::Info),
            "WARNING" => Some(Severity::Warning),
            "ERROR" => Some(Severity::Error),
            _ => None,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub id: Option<i64>,
    pub project_id: i64,
    pub file_id: Option<i64>,
    pub symbol_id: Option<i64>,
    pub kind: String,
    pub severity: Severity,
    pub message: String,
    pub status: String,
    pub evidence: Option<String>,
}
