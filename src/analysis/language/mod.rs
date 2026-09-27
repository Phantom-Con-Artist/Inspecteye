//! Language-specific analysis.
//!
//! Source text is parsed once into a [`ParsedFile`]; every analysis (syntax
//! diagnostics, symbols, references) then reads that same tree. Each language
//! contributes a [`LanguageAnalyzer`] that knows its grammar and how to map
//! grammar nodes onto Inspectye's language-independent model.

mod csharp;
mod ecmascript;
mod javascript;
mod python;
mod rust;
mod typescript;

use crate::analysis::logic::{LogicNode, TerminatorKind};
use crate::error::{InspectyeError, Result};
use crate::intelligence::{ExtractedReference, ReferenceSpec, SymbolKind, SymbolSpec};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

/// Grammar-backed analysis for one language.
pub trait LanguageAnalyzer: Send + Sync {
    /// Language name as produced by resource classification (e.g. `"rust"`).
    fn language_id(&self) -> &'static str;

    /// Version of the extraction rules; bump when extraction output changes
    /// so stored analysis is invalidated.
    fn version(&self) -> &'static str;

    /// Grammar for a file. Some languages have dialects chosen by extension
    /// (TypeScript vs TSX).
    fn grammar_for(&self, path: &Path) -> tree_sitter::Language;

    /// Symbols declared by `node` (usually zero or one; a C# field
    /// declaration can declare several). `parent` is the kind of the
    /// enclosing symbol, if any.
    fn symbol_specs(
        &self,
        node: Node,
        parsed: &ParsedFile,
        parent: Option<SymbolKind>,
    ) -> Vec<SymbolSpec>;

    /// References made by `node` (imports, calls, type uses, inheritance).
    fn reference_specs(&self, _node: Node, _parsed: &ParsedFile) -> Vec<ReferenceSpec> {
        Vec::new()
    }

    /// Language-independent view of `node` for logic checks.
    fn logic_view<'t>(&self, _node: Node<'t>, _parsed: &ParsedFile) -> Option<LogicNode<'t>> {
        None
    }

    /// Project files an import or module declaration in `from_path` refers
    /// to. Empty when it names something outside the project.
    fn resolve_import(
        &self,
        _reference: &ExtractedReference,
        _from_path: &str,
        _ctx: &ImportContext,
    ) -> Vec<String> {
        Vec::new()
    }

    /// True when the import can only name a file of this project (a relative
    /// path, a child module declaration), so failing to resolve it means the
    /// file is missing rather than that it is an external package.
    fn is_relative_import(&self, _reference: &ExtractedReference) -> bool {
        false
    }
}

/// What import resolution may consult about the project.
pub struct ImportContext<'a> {
    /// Project-relative paths of files currently on disk.
    pub paths: &'a HashSet<String>,
    /// Declared namespace (qualified name) -> files declaring it.
    pub namespaces: &'a HashMap<String, Vec<String>>,
    /// Also accept candidates that exist on disk but are not indexed
    /// (ignored or excluded files). Used when checking for broken imports.
    pub on_disk: Option<&'a dyn Fn(&str) -> bool>,
}

impl ImportContext<'_> {
    /// First candidate path that exists in the project.
    pub fn first_existing(&self, candidates: impl IntoIterator<Item = String>) -> Vec<String> {
        candidates
            .into_iter()
            .map(|c| normalize_rel(&c))
            .find(|c| self.paths.contains(c) || self.on_disk.is_some_and(|exists| exists(c)))
            .into_iter()
            .collect()
    }
}

/// Directory part of a project-relative path (`""` at the root).
pub fn parent_dir(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// Joins path segments and resolves `.` / `..` lexically.
pub fn normalize_rel(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub fn join_rel(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_string()
    } else {
        format!("{dir}/{rest}")
    }
}

/// A source file parsed with its language grammar.
pub struct ParsedFile {
    pub language: &'static str,
    pub source: String,
    pub tree: Tree,
}

impl ParsedFile {
    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    pub fn text(&self, node: Node) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// Text of a named field of `node`, if present.
    pub fn field_text(&self, node: Node, field: &str) -> Option<&str> {
        node.child_by_field_name(field).map(|n| self.text(n))
    }

    /// The full source line (1-based) without its line terminator.
    pub fn line(&self, line_no: usize) -> &str {
        self.source
            .lines()
            .nth(line_no.saturating_sub(1))
            .unwrap_or("")
            .trim_end_matches('\r')
    }
}

/// Texts of the direct children of `node` whose kind is in `kinds`.
pub fn child_texts(parsed: &ParsedFile, node: Node, kinds: &[&str]) -> Vec<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|c| kinds.contains(&c.kind()))
        .map(|c| parsed.text(c).to_string())
        .collect()
}

/// Named children of `node`, excluding comments.
pub fn statements<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| !c.kind().contains("comment"))
        .collect()
}

/// `(expr)` -> `expr`.
pub fn strip_parens(node: Node) -> Node {
    let mut n = node;
    while n.kind() == "parenthesized_expression" && n.named_child_count() == 1 {
        match n.named_child(0) {
            Some(inner) => n = inner,
            None => break,
        }
    }
    n
}

/// A binary node's operator text (`==`, `<`), from its `operator` field or,
/// failing that, the first anonymous child.
pub fn operator_text(node: Node, parsed: &ParsedFile) -> String {
    if let Some(op) = node.child_by_field_name("operator") {
        return parsed.text(op).to_string();
    }
    let mut cursor = node.walk();
    let op = node
        .children(&mut cursor)
        .find(|c| !c.is_named())
        .map(|c| parsed.text(c).to_string());
    op.unwrap_or_default()
}

/// Terminator view for a statement kind shared by several grammars.
pub fn terminator_for(kind: &str) -> Option<LogicNode<'static>> {
    let kind = match kind {
        "return_statement" | "return_expression" => TerminatorKind::Return,
        "throw_statement" | "throw_expression" | "raise_statement" => TerminatorKind::Throw,
        "break_statement" | "break_expression" => TerminatorKind::Break,
        "continue_statement" | "continue_expression" => TerminatorKind::Continue,
        _ => return None,
    };
    Some(LogicNode::Terminator {
        kind,
        conditional: false,
    })
}

pub struct LanguageRegistry {
    analyzers: Vec<Box<dyn LanguageAnalyzer>>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self {
            analyzers: vec![
                Box::new(rust::RustAnalyzer),
                Box::new(typescript::TypeScriptAnalyzer),
                Box::new(javascript::JavaScriptAnalyzer),
                Box::new(python::PythonAnalyzer),
                Box::new(csharp::CSharpAnalyzer),
            ],
        }
    }
}

impl LanguageRegistry {
    pub fn get(&self, language: &str) -> Option<&dyn LanguageAnalyzer> {
        self.analyzers
            .iter()
            .find(|a| a.language_id() == language)
            .map(|a| a.as_ref())
    }

    pub fn supports(&self, language: &str) -> bool {
        self.get(language).is_some()
    }

    pub fn supported_languages(&self) -> Vec<&'static str> {
        self.analyzers.iter().map(|a| a.language_id()).collect()
    }

    /// Combined version of all language analyzers, so that a change to any
    /// extraction rule invalidates stored analysis.
    pub fn combined_version(&self) -> String {
        self.analyzers
            .iter()
            .map(|a| format!("{}@{}", a.language_id(), a.version()))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn parse(&self, language: &str, path: &Path, source: String) -> Result<ParsedFile> {
        let analyzer = self.get(language).ok_or_else(|| {
            InspectyeError::Analysis(format!("no analyzer for language '{language}'"))
        })?;
        let mut parser = Parser::new();
        parser
            .set_language(&analyzer.grammar_for(path))
            .map_err(|e| InspectyeError::Analysis(format!("grammar for '{language}': {e}")))?;
        let tree = parser.parse(&source, None).ok_or_else(|| {
            InspectyeError::Analysis(format!("parser returned no tree for {}", path.display()))
        })?;
        Ok(ParsedFile {
            language: analyzer.language_id(),
            source,
            tree,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parses_each_supported_language() {
        let registry = LanguageRegistry::default();
        let samples = [
            ("rust", "a.rs", "fn main() {}"),
            ("typescript", "a.ts", "const x: number = 1;"),
            ("typescript", "a.tsx", "const x = <div />;"),
            ("javascript", "a.js", "const x = 1;"),
            ("python", "a.py", "def f():\n    pass\n"),
            ("csharp", "a.cs", "class A {}"),
        ];
        for (lang, path, src) in samples {
            let parsed = registry
                .parse(lang, Path::new(path), src.to_string())
                .unwrap();
            assert!(!parsed.root().has_error(), "{path} should parse cleanly");
        }
    }

    #[test]
    fn test_unsupported_language_is_an_error() {
        let registry = LanguageRegistry::default();
        assert!(!registry.supports("cobol"));
        assert!(registry
            .parse("cobol", Path::new("a.cbl"), String::new())
            .is_err());
    }
}
