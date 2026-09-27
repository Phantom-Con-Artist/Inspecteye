use super::{ecmascript, ImportContext, LanguageAnalyzer, ParsedFile};
use crate::analysis::logic::LogicNode;
use crate::intelligence::{ExtractedReference, ReferenceSpec, SymbolKind, SymbolSpec};
use std::path::Path;
use tree_sitter::Node;

pub struct JavaScriptAnalyzer;

impl LanguageAnalyzer for JavaScriptAnalyzer {
    fn language_id(&self) -> &'static str {
        "javascript"
    }

    fn version(&self) -> &'static str {
        "4"
    }

    fn grammar_for(&self, _path: &Path) -> tree_sitter::Language {
        tree_sitter_javascript::LANGUAGE.into()
    }

    fn symbol_specs(
        &self,
        node: Node,
        parsed: &ParsedFile,
        parent: Option<SymbolKind>,
    ) -> Vec<SymbolSpec> {
        ecmascript::symbol_specs(node, parsed, parent)
    }

    fn reference_specs(&self, node: Node, parsed: &ParsedFile) -> Vec<ReferenceSpec> {
        ecmascript::reference_specs(node, parsed)
    }

    fn logic_view<'t>(&self, node: Node<'t>, parsed: &ParsedFile) -> Option<LogicNode<'t>> {
        ecmascript::logic_view(node, parsed)
    }

    fn resolve_import(
        &self,
        reference: &ExtractedReference,
        from_path: &str,
        ctx: &ImportContext,
    ) -> Vec<String> {
        ecmascript::resolve_import(reference, from_path, ctx)
    }

    fn is_relative_import(&self, reference: &ExtractedReference) -> bool {
        ecmascript::is_relative_import(reference)
    }
}
