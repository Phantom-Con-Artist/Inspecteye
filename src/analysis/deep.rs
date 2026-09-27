use crate::database::FileRecord;
use crate::error::Result;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepAnalysisResult {
    pub file_id: i64,
    pub content_hash: String,
    pub status: String,
    pub parser_version: String,
    pub analyzer_version: String,
    pub error: Option<String>,
    pub parseable: bool,
    pub lines_analyzed: usize,
}

pub trait DeepAnalyzer: Send + Sync {
    fn analyzer_name(&self) -> &str;
    fn analyzer_version(&self) -> &str;
    fn parser_version(&self) -> &str;
    fn is_eligible(&self, file: &FileRecord) -> bool;
    fn analyze(&self, project_root: &Path, file: &FileRecord) -> Result<DeepAnalysisResult>;
}

#[derive(Debug, Default, Clone)]
pub struct BasicDeepAnalyzer;

impl BasicDeepAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl DeepAnalyzer for BasicDeepAnalyzer {
    fn analyzer_name(&self) -> &str {
        "basic-analyzer"
    }

    fn analyzer_version(&self) -> &str {
        "0.1.0"
    }

    fn parser_version(&self) -> &str {
        "text-0.1.0"
    }

    fn is_eligible(&self, file: &FileRecord) -> bool {
        if !file.exists_on_disk || file.is_ignored {
            return false;
        }

        // Only source code resources are eligible for deep analysis
        file.kind == "source"
    }

    fn analyze(&self, project_root: &Path, file: &FileRecord) -> Result<DeepAnalysisResult> {
        let abs_path = project_root.join(&file.path);

        match fs::read_to_string(&abs_path) {
            Ok(content) => {
                let lines = content.lines().count();
                let mut open_braces = 0i32;
                let mut open_parens = 0i32;
                for ch in content.chars() {
                    match ch {
                        '{' => open_braces += 1,
                        '}' => open_braces -= 1,
                        '(' => open_parens += 1,
                        ')' => open_parens -= 1,
                        _ => {}
                    }
                }

                let parseable = open_braces == 0 && open_parens == 0;

                Ok(DeepAnalysisResult {
                    file_id: file.id,
                    content_hash: file.content_hash.clone(),
                    status: "completed".to_string(),
                    parser_version: self.parser_version().to_string(),
                    analyzer_version: self.analyzer_version().to_string(),
                    error: None,
                    parseable,
                    lines_analyzed: lines,
                })
            }
            Err(e) => Ok(DeepAnalysisResult {
                file_id: file.id,
                content_hash: file.content_hash.clone(),
                status: "failed".to_string(),
                parser_version: self.parser_version().to_string(),
                analyzer_version: self.analyzer_version().to_string(),
                error: Some(format!("Failed reading file: {e}")),
                parseable: false,
                lines_analyzed: 0,
            }),
        }
    }
}
