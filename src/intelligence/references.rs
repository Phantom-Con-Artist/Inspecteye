//! Raw references found in one file, before project-wide resolution.
//!
//! Extraction only records what the syntax says (a call to `save`, an import
//! of `./db`). Deciding which file or symbol that names happens later in
//! [`crate::intelligence::resolve`], once every file's symbols are known.

use crate::analysis::language::{LanguageAnalyzer, ParsedFile};
use crate::intelligence::ExtractedSymbol;
use crate::issues::SourceRange;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceKind {
    /// Import of another module (`import`, `use`, `using`, `require`).
    Import,
    /// Rust `mod name;` — declares a child module file.
    ModuleDecl,
    Call,
    /// Use of a type or instantiation that is not a call to a function.
    TypeReference,
    Inherits,
    Implements,
}

impl ReferenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReferenceKind::Import => "import",
            ReferenceKind::ModuleDecl => "module_decl",
            ReferenceKind::Call => "call",
            ReferenceKind::TypeReference => "type_reference",
            ReferenceKind::Inherits => "inherits",
            ReferenceKind::Implements => "implements",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            ReferenceKind::Import,
            ReferenceKind::ModuleDecl,
            ReferenceKind::Call,
            ReferenceKind::TypeReference,
            ReferenceKind::Inherits,
            ReferenceKind::Implements,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// What a language analyzer reports for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSpec {
    pub kind: ReferenceKind,
    /// Module specifier for imports; the referenced simple name otherwise.
    pub target: String,
    /// Qualifier written before the name (`util` in `util::run`), or the
    /// names imported from a module.
    pub detail: Vec<String>,
}

impl ReferenceSpec {
    pub fn new(kind: ReferenceKind, target: impl Into<String>) -> Self {
        Self {
            kind,
            target: target.into(),
            detail: Vec::new(),
        }
    }

    pub fn with_detail(mut self, detail: Vec<String>) -> Self {
        self.detail = detail;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedReference {
    pub kind: ReferenceKind,
    pub target: String,
    pub detail: Vec<String>,
    /// Identity of the innermost enclosing symbol, if any.
    pub from_symbol: Option<String>,
    pub range: SourceRange,
}

pub fn extract_references(
    parsed: &ParsedFile,
    analyzer: &dyn LanguageAnalyzer,
    symbols: &[ExtractedSymbol],
) -> Vec<ExtractedReference> {
    let mut out = Vec::new();
    let mut stack = vec![parsed.root()];
    while let Some(node) = stack.pop() {
        for spec in analyzer.reference_specs(node, parsed) {
            if spec.target.is_empty() {
                continue;
            }
            let range = SourceRange::from_node(node, &parsed.source);
            out.push(ExtractedReference {
                kind: spec.kind,
                target: spec.target,
                detail: spec.detail,
                from_symbol: enclosing_symbol(symbols, &range),
                range,
            });
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

/// Innermost symbol containing the reference. Inheritance clauses sit inside
/// their declaration, so the declaration itself becomes the source.
fn enclosing_symbol(symbols: &[ExtractedSymbol], range: &SourceRange) -> Option<String> {
    symbols
        .iter()
        .filter(|s| s.range.start_byte <= range.start_byte && range.end_byte <= s.range.end_byte)
        .min_by_key(|s| s.range.end_byte - s.range.start_byte)
        .map(|s| s.identity.clone())
}

/// True when `node` is the name being declared by its parent, not a use.
pub fn is_declaration_name(node: Node) -> bool {
    node.parent()
        .and_then(|p| p.child_by_field_name("name"))
        .is_some_and(|n| n.id() == node.id())
}
