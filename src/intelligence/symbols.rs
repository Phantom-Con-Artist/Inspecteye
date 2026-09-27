//! Language-independent symbol model and extraction.

use crate::analysis::compute_bytes_hash;
use crate::analysis::language::{LanguageAnalyzer, ParsedFile};
use crate::issues::SourceRange;
use serde::Serialize;
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Namespace,
    Module,
    Class,
    Interface,
    Struct,
    Enum,
    Trait,
    Impl,
    TypeAlias,
    Function,
    Method,
    Constructor,
    Property,
    Field,
    Constant,
    Variant,
    Macro,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Namespace => "namespace",
            SymbolKind::Module => "module",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Constructor => "constructor",
            SymbolKind::Property => "property",
            SymbolKind::Field => "field",
            SymbolKind::Constant => "constant",
            SymbolKind::Variant => "variant",
            SymbolKind::Macro => "macro",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        const ALL: [SymbolKind; 17] = [
            SymbolKind::Namespace,
            SymbolKind::Module,
            SymbolKind::Class,
            SymbolKind::Interface,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Impl,
            SymbolKind::TypeAlias,
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Constructor,
            SymbolKind::Property,
            SymbolKind::Field,
            SymbolKind::Constant,
            SymbolKind::Variant,
            SymbolKind::Macro,
        ];
        ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Kinds that contain other declarations.
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            SymbolKind::Namespace
                | SymbolKind::Module
                | SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Trait
                | SymbolKind::Impl
        )
    }
}

/// What a language analyzer reports for a declaration node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSpec {
    pub kind: SymbolKind,
    pub name: String,
    pub modifiers: Vec<String>,
    /// The symbol's scope covers the node's following siblings too
    /// (C# file-scoped namespaces).
    pub scopes_following_siblings: bool,
}

impl SymbolSpec {
    pub fn new(kind: SymbolKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            modifiers: Vec::new(),
            scopes_following_siblings: false,
        }
    }

    pub fn with_modifiers(mut self, modifiers: Vec<String>) -> Self {
        self.modifiers = modifiers;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractedSymbol {
    pub kind: SymbolKind,
    pub name: String,
    /// Names of enclosing symbols and this one, joined with `.`.
    pub qualified_name: String,
    /// Stable identity within a file: independent of line numbers, so the
    /// same logical declaration keeps it across edits.
    pub identity: String,
    /// Index of the enclosing symbol in the extraction result.
    pub parent: Option<usize>,
    pub signature: String,
    pub modifiers: Vec<String>,
    pub range: SourceRange,
    /// Hash of the declaration's text; changes when the symbol's code changes.
    pub body_hash: String,
    /// Cyclomatic complexity for functions/methods; set by analysis.
    pub complexity: Option<usize>,
}

impl ExtractedSymbol {
    pub fn has_modifier(&self, m: &str) -> bool {
        self.modifiers.iter().any(|x| x == m)
    }
}

const MAX_SIGNATURE: usize = 200;

/// Extracts all symbols of a parsed file, in source order.
pub fn extract_symbols(
    parsed: &ParsedFile,
    analyzer: &dyn LanguageAnalyzer,
) -> Vec<ExtractedSymbol> {
    let mut out = Vec::new();
    walk(parsed, analyzer, parsed.root(), None, &mut out);
    assign_identities(&mut out);
    out
}

fn walk(
    parsed: &ParsedFile,
    analyzer: &dyn LanguageAnalyzer,
    node: Node,
    parent: Option<usize>,
    out: &mut Vec<ExtractedSymbol>,
) {
    let mut cursor = node.walk();
    let mut scope = parent;
    for child in node.named_children(&mut cursor) {
        let parent_kind = scope.map(|i| out[i].kind);
        let specs = analyzer.symbol_specs(child, parsed, parent_kind);
        let mut child_parent = scope;
        for spec in specs {
            let idx = push_symbol(parsed, child, spec.clone(), scope, out);
            // Declarations nested in any symbol (e.g. locals in a method)
            // see that symbol as their parent.
            child_parent = Some(idx);
            if spec.scopes_following_siblings {
                // Extend the scope to the end of the enclosing node.
                out[idx].range.end_line = node.end_position().row + 1;
                out[idx].range.end_byte = node.end_byte();
                scope = Some(idx);
            }
        }
        walk(parsed, analyzer, child, child_parent, out);
    }
}

fn push_symbol(
    parsed: &ParsedFile,
    node: Node,
    spec: SymbolSpec,
    parent: Option<usize>,
    out: &mut Vec<ExtractedSymbol>,
) -> usize {
    let qualified_name = match parent {
        Some(p) => format!("{}.{}", out[p].qualified_name, spec.name),
        None => spec.name.clone(),
    };
    let text = parsed.text(node).replace("\r\n", "\n");
    out.push(ExtractedSymbol {
        kind: spec.kind,
        name: spec.name,
        qualified_name,
        identity: String::new(),
        parent,
        signature: signature_of(parsed, node),
        modifiers: spec.modifiers,
        range: SourceRange::from_node(node, &parsed.source),
        body_hash: compute_bytes_hash(text.as_bytes()),
        complexity: None,
    });
    out.len() - 1
}

/// Declaration text up to its body, whitespace-collapsed.
fn signature_of(parsed: &ParsedFile, node: Node) -> String {
    let end = node
        .child_by_field_name("body")
        .map(|b| b.start_byte())
        .unwrap_or(node.end_byte());
    let raw = parsed.source.get(node.start_byte()..end).unwrap_or("");
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_end_matches(['{', ':', ';', ' ']).to_string();
    if trimmed.chars().count() > MAX_SIGNATURE {
        let cut: String = trimmed.chars().take(MAX_SIGNATURE).collect();
        format!("{cut}…")
    } else {
        trimmed
    }
}

/// `kind:qualified.name`, with `#n` appended for the n-th duplicate
/// (overloads, partial declarations of the same name in one file).
fn assign_identities(symbols: &mut [ExtractedSymbol]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for s in symbols.iter_mut() {
        let base = format!("{}:{}", s.kind.as_str(), s.qualified_name);
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        s.identity = if *n == 1 { base } else { format!("{base}#{n}") };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolChangeKind {
    Added,
    Removed,
    Modified,
}

impl SymbolChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolChangeKind::Added => "added",
            SymbolChangeKind::Removed => "removed",
            SymbolChangeKind::Modified => "modified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolChange {
    pub identity: String,
    pub kind: SymbolKind,
    pub qualified_name: String,
    pub change: SymbolChangeKind,
    /// Range in the old version (Removed/Modified).
    pub old_range: Option<SourceRange>,
    /// Range in the new version (Added/Modified).
    pub new_range: Option<SourceRange>,
}

/// Compares two versions of a file's symbols by identity. A container whose
/// only change is inside a changed member is still reported, since its text changed.
pub fn diff_symbols(old: &[ExtractedSymbol], new: &[ExtractedSymbol]) -> Vec<SymbolChange> {
    let old_by_id: HashMap<&str, &ExtractedSymbol> =
        old.iter().map(|s| (s.identity.as_str(), s)).collect();
    let new_by_id: HashMap<&str, &ExtractedSymbol> =
        new.iter().map(|s| (s.identity.as_str(), s)).collect();

    let mut changes = Vec::new();
    for s in new {
        match old_by_id.get(s.identity.as_str()) {
            None => changes.push(SymbolChange {
                identity: s.identity.clone(),
                kind: s.kind,
                qualified_name: s.qualified_name.clone(),
                change: SymbolChangeKind::Added,
                old_range: None,
                new_range: Some(s.range),
            }),
            Some(o) if o.body_hash != s.body_hash => changes.push(SymbolChange {
                identity: s.identity.clone(),
                kind: s.kind,
                qualified_name: s.qualified_name.clone(),
                change: SymbolChangeKind::Modified,
                old_range: Some(o.range),
                new_range: Some(s.range),
            }),
            Some(_) => {}
        }
    }
    for s in old {
        if !new_by_id.contains_key(s.identity.as_str()) {
            changes.push(SymbolChange {
                identity: s.identity.clone(),
                kind: s.kind,
                qualified_name: s.qualified_name.clone(),
                change: SymbolChangeKind::Removed,
                old_range: Some(s.range),
                new_range: None,
            });
        }
    }
    changes
}

/// Innermost symbols whose ranges contain any of the given 1-based lines.
pub fn symbols_touching_lines<'a>(
    symbols: &'a [ExtractedSymbol],
    lines: &[usize],
) -> Vec<&'a ExtractedSymbol> {
    let mut hits: Vec<usize> = Vec::new();
    for &line in lines {
        let innermost = symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.range.start_line <= line && line <= s.range.end_line)
            .min_by_key(|(_, s)| s.range.end_byte - s.range.start_byte)
            .map(|(i, _)| i);
        if let Some(i) = innermost {
            if !hits.contains(&i) {
                hits.push(i);
            }
        }
    }
    hits.sort();
    hits.into_iter().map(|i| &symbols[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::language::LanguageRegistry;
    use std::path::Path;

    fn ids(lang: &str, path: &str, src: &str) -> Vec<String> {
        let registry = LanguageRegistry::default();
        let parsed = registry
            .parse(lang, Path::new(path), src.to_string())
            .unwrap();
        extract_symbols(&parsed, registry.get(lang).unwrap())
            .into_iter()
            .map(|s| s.identity)
            .collect()
    }

    #[test]
    fn test_rust_symbols() {
        let got = ids(
            "rust",
            "a.rs",
            "struct S { a: u8 }\ntrait T { fn f(&self); }\nimpl T for S { fn f(&self) {} }\nconst C: u8 = 1;\n",
        );
        assert_eq!(
            got,
            [
                "struct:S",
                "field:S.a",
                "trait:T",
                "method:T.f",
                "impl:impl T for S",
                "method:impl T for S.f",
                "constant:C"
            ]
        );
    }

    #[test]
    fn test_typescript_symbols_skip_locals() {
        let got = ids(
            "typescript",
            "a.ts",
            "export class A { constructor() {} go() { const local = 1; } }\nconst f = () => 1;\nenum E { X }\n",
        );
        assert_eq!(
            got,
            [
                "class:A",
                "constructor:A.constructor",
                "method:A.go",
                "function:f",
                "enum:E",
                "variant:E.X"
            ]
        );
    }

    #[test]
    fn test_python_symbols() {
        let got = ids(
            "python",
            "a.py",
            "LIMIT = 1\nlocal = 2\nclass A:\n    x = 0\n    def __init__(self): pass\n    def go(self):\n        y = 1\ndef f(): pass\n",
        );
        assert_eq!(
            got,
            [
                "constant:LIMIT",
                "class:A",
                "field:A.x",
                "constructor:A.__init__",
                "method:A.go",
                "function:f"
            ]
        );
    }

    #[test]
    fn test_csharp_overloads_partials_and_file_scoped_namespace() {
        let src = "namespace N;\npublic partial class A {\n  void M() {}\n  void M(int x) {}\n  const int K = 1;\n}\n";
        let got = ids("csharp", "a.cs", src);
        assert_eq!(
            got,
            [
                "namespace:N",
                "class:N.A",
                "method:N.A.M",
                "method:N.A.M#2",
                "constant:N.A.K"
            ]
        );
    }

    #[test]
    fn test_diff_symbols_by_identity_not_lines() {
        let registry = LanguageRegistry::default();
        let lang = registry.get("rust").unwrap();
        let parse = |s: &str| {
            let p = registry
                .parse("rust", Path::new("a.rs"), s.to_string())
                .unwrap();
            extract_symbols(&p, lang)
        };
        // Inserting a function above shifts lines but must not mark `b` modified.
        let old = parse("fn b() { 1; }\nfn c() {}\n");
        let new = parse("fn a() {}\n\nfn b() { 1; }\nfn c() { 2; }\n");
        let changes: Vec<(String, SymbolChangeKind)> = diff_symbols(&old, &new)
            .into_iter()
            .map(|c| (c.identity, c.change))
            .collect();
        assert_eq!(
            changes,
            [
                ("function:a".to_string(), SymbolChangeKind::Added),
                ("function:c".to_string(), SymbolChangeKind::Modified)
            ]
        );
    }
}
