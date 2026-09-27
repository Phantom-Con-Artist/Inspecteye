use crate::analysis::complexity::function_complexity;
use crate::analysis::external_refs;
use crate::analysis::hash::compute_bytes_hash;
use crate::analysis::language::{LanguageRegistry, ParsedFile};
use crate::analysis::logic::logic_diagnostics;
use crate::analysis::syntax::syntax_diagnostics;
use crate::database::FileRecord;
use crate::error::Result;
use crate::intelligence::{
    extract_references, extract_symbols, symbols_touching_lines, ExternalReference,
    ExtractedReference, ExtractedSymbol,
};
use crate::issues::Diagnostic;
use std::fs;
use std::path::Path;

/// Language-independent result of analysing one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileAnalysis {
    pub language: String,
    pub line_count: usize,
    pub syntax_diagnostics: Vec<Diagnostic>,
    pub logic_diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<ExtractedSymbol>,
    pub references: Vec<ExtractedReference>,
    /// Absolute filesystem paths written in the file.
    pub external_references: Vec<ExternalReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepAnalysisResult {
    pub file_id: i64,
    /// Hash of the content that was actually analysed.
    pub content_hash: String,
    pub status: String,
    pub parser_version: String,
    pub analyzer_version: String,
    pub error: Option<String>,
    pub analysis: Option<FileAnalysis>,
}

pub trait DeepAnalyzer: Send + Sync {
    fn analyzer_name(&self) -> &str;
    fn analyzer_version(&self) -> &str;
    fn parser_version(&self) -> &str;
    fn is_eligible(&self, file: &FileRecord) -> bool;
    fn analyze(&self, project_root: &Path, file: &FileRecord) -> Result<DeepAnalysisResult>;
}

/// Tree-sitter backed analyzer covering every language in the registry.
pub struct CodeAnalyzer {
    registry: LanguageRegistry,
    analyzer_version: String,
}

impl Default for CodeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeAnalyzer {
    /// Bump when the language-independent analysis output changes.
    const CORE_VERSION: &'static str = "9";

    pub fn new() -> Self {
        let registry = LanguageRegistry::default();
        let analyzer_version = format!("{}+{}", Self::CORE_VERSION, registry.combined_version());
        Self {
            registry,
            analyzer_version,
        }
    }

    pub fn registry(&self) -> &LanguageRegistry {
        &self.registry
    }

    /// Parses source text and runs every analysis over the single parse tree.
    pub fn analyze_source(
        &self,
        language: &str,
        path: &Path,
        source: String,
    ) -> Result<FileAnalysis> {
        let parsed = self.registry.parse(language, path, source)?;
        let mut analysis = self.analyze_parsed(&parsed);
        external_refs::mark_test_code(&mut analysis.external_references, path);
        Ok(analysis)
    }

    fn analyze_parsed(&self, parsed: &ParsedFile) -> FileAnalysis {
        let (symbols, references, mut logic) = match self.registry.get(parsed.language) {
            Some(lang) => {
                let mut symbols = extract_symbols(parsed, lang);
                let complexity = function_complexity(parsed, lang, &symbols);
                for s in &mut symbols {
                    s.complexity = complexity.get(&s.identity).copied();
                }
                let references = extract_references(parsed, lang, &symbols);
                (symbols, references, logic_diagnostics(parsed, lang))
            }
            None => (Vec::new(), Vec::new(), Vec::new()),
        };
        let external_references = external_refs::from_parsed(parsed, &symbols);
        let mut diagnostics = syntax_diagnostics(parsed);
        attach_symbols(&mut diagnostics, &symbols);
        attach_symbols(&mut logic, &symbols);
        FileAnalysis {
            language: parsed.language.to_string(),
            line_count: parsed.source.lines().count(),
            syntax_diagnostics: diagnostics,
            logic_diagnostics: logic,
            symbols,
            references,
            external_references,
        }
    }

    /// Analysis of a file without a parser: only external references.
    fn analyze_text(&self, file: &FileRecord, source: &str) -> FileAnalysis {
        let mut external_references =
            external_refs::from_text(Path::new(&file.path), source, file.language.as_deref());
        external_refs::mark_test_code(&mut external_references, Path::new(&file.path));
        FileAnalysis {
            language: file.language.clone().unwrap_or_else(|| file.kind.clone()),
            line_count: source.lines().count(),
            external_references,
            ..Default::default()
        }
    }

    fn result(&self, file: &FileRecord, hash: String) -> DeepAnalysisResult {
        DeepAnalysisResult {
            file_id: file.id,
            content_hash: hash,
            status: "completed".to_string(),
            parser_version: self.parser_version().to_string(),
            analyzer_version: self.analyzer_version.clone(),
            error: None,
            analysis: None,
        }
    }
}

impl DeepAnalyzer for CodeAnalyzer {
    fn analyzer_name(&self) -> &str {
        "code-analyzer"
    }

    fn analyzer_version(&self) -> &str {
        &self.analyzer_version
    }

    fn parser_version(&self) -> &str {
        "tree-sitter-0.25"
    }

    /// Source files in any language and configuration files. Files without
    /// a parser get text-only analysis (external references).
    fn is_eligible(&self, file: &FileRecord) -> bool {
        file.exists_on_disk && !file.is_ignored && (file.kind == "source" || file.kind == "config")
    }

    fn analyze(&self, project_root: &Path, file: &FileRecord) -> Result<DeepAnalysisResult> {
        let abs_path = project_root.join(&file.path);
        let bytes = match fs::read(&abs_path) {
            Ok(b) => b,
            Err(e) => {
                let mut r = self.result(file, file.content_hash.clone());
                r.status = "failed".to_string();
                r.error = Some(format!("failed reading file: {e}"));
                return Ok(r);
            }
        };
        let hash = compute_bytes_hash(&bytes);
        let mut result = self.result(file, hash);

        let source = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                result.status = "failed".to_string();
                result.error = Some("file is not valid UTF-8".to_string());
                return Ok(result);
            }
        };
        let language = file.language.as_deref().unwrap_or_default();
        if file.kind != "source" || !self.registry.supports(language) {
            result.analysis = Some(self.analyze_text(file, &source));
            return Ok(result);
        }
        match self.analyze_source(language, Path::new(&file.path), source) {
            Ok(analysis) => result.analysis = Some(analysis),
            Err(e) => {
                result.status = "failed".to_string();
                result.error = Some(e.to_string());
            }
        }
        Ok(result)
    }
}

/// Names the innermost symbol containing each diagnostic.
pub fn attach_symbols(diagnostics: &mut [Diagnostic], symbols: &[ExtractedSymbol]) {
    for d in diagnostics {
        if d.symbol.is_none() {
            d.symbol = symbols_touching_lines(symbols, &[d.range.start_line])
                .first()
                .map(|s| s.qualified_name.clone());
        }
    }
}
