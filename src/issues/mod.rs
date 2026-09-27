use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Error => write!(f, "ERROR"),
        }
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
