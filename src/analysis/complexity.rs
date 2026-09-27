//! Cyclomatic complexity of functions: 1 + the number of decision points.
//!
//! Decision points are branches (`if`/`elif`), loops, exception handlers,
//! short-circuit boolean operators, conditional expressions and switch/match
//! cases. Nested functions are measured separately and do not add to their
//! container. The number is a count that can be checked by reading the code.

use crate::analysis::language::{operator_text, LanguageAnalyzer, ParsedFile};
use crate::analysis::logic::LogicNode;
use crate::intelligence::{ExtractedSymbol, SymbolKind};
use std::collections::HashMap;
use tree_sitter::Node;

const CASE_KINDS: &[&str] = &[
    "switch_case",    // TypeScript / JavaScript
    "switch_section", // C#
    "match_arm",      // Rust
    "case_clause",    // Python `match`
    "switch_expression_arm",
];
const CONDITIONAL_KINDS: &[&str] = &["ternary_expression", "conditional_expression"];
const SHORT_CIRCUIT: &[&str] = &["&&", "||", "and", "or", "??"];

/// Complexity per function-like symbol, keyed by symbol identity.
pub fn function_complexity(
    parsed: &ParsedFile,
    lang: &dyn LanguageAnalyzer,
    symbols: &[ExtractedSymbol],
) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for s in symbols.iter().filter(|s| {
        matches!(
            s.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
        )
    }) {
        let Some(node) = parsed
            .root()
            .descendant_for_byte_range(s.range.start_byte, s.range.end_byte)
        else {
            continue;
        };
        let body = node.child_by_field_name("body").unwrap_or(node);
        out.insert(s.identity.clone(), 1 + decisions(body, parsed, lang));
    }
    out
}

fn decisions(node: Node, parsed: &ParsedFile, lang: &dyn LanguageAnalyzer) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let view = lang.logic_view(child, parsed);
        if matches!(view, Some(LogicNode::FunctionBoundary)) {
            continue;
        }
        let kind = child.kind();
        let is_decision = match view {
            Some(LogicNode::If { .. } | LogicNode::Loop { .. } | LogicNode::Catch { .. }) => true,
            _ => {
                CASE_KINDS.contains(&kind)
                    || CONDITIONAL_KINDS.contains(&kind)
                    || kind == "boolean_operator"
                    || (kind == "binary_expression"
                        && SHORT_CIRCUIT.contains(&operator_text(child, parsed).as_str()))
            }
        };
        if is_decision {
            count += 1;
        }
        count += decisions(child, parsed, lang);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::language::LanguageRegistry;
    use crate::intelligence::extract_symbols;
    use std::path::Path;

    fn complexity(lang: &str, path: &str, src: &str) -> Vec<(String, usize)> {
        let registry = LanguageRegistry::default();
        let parsed = registry
            .parse(lang, Path::new(path), src.to_string())
            .unwrap();
        let analyzer = registry.get(lang).unwrap();
        let symbols = extract_symbols(&parsed, analyzer);
        let map = function_complexity(&parsed, analyzer, &symbols);
        let mut v: Vec<(String, usize)> = map.into_iter().collect();
        v.sort();
        v
    }

    #[test]
    fn test_straight_line_function_is_one() {
        assert_eq!(
            complexity("rust", "a.rs", "fn f() { g(); }\n"),
            [("function:f".into(), 1)]
        );
    }

    #[test]
    fn test_counts_branches_loops_and_operators() {
        let ts = "function f(a, b) {\n  if (a && b) { x(); } else if (a) { y(); }\n  for (const i of a) { }\n  return a ? 1 : 2;\n}\n";
        // if + && + else-if + for + ternary = 5
        assert_eq!(
            complexity("typescript", "a.ts", ts),
            [("function:f".into(), 6)]
        );
    }

    #[test]
    fn test_nested_functions_are_measured_separately() {
        let py = "def outer(x):\n    if x:\n        pass\n    def inner(y):\n        while y:\n            pass\n    return inner\n";
        assert_eq!(
            complexity("python", "a.py", py),
            [
                ("function:outer".into(), 2),
                ("function:outer.inner".into(), 2)
            ]
        );
    }

    #[test]
    fn test_match_and_switch_cases() {
        let rs = "fn f(x: u8) -> u8 { match x { 0 => 1, 1 => 2, _ => 3 } }\n";
        assert_eq!(complexity("rust", "a.rs", rs), [("function:f".into(), 4)]);
        let cs =
            "class A { int M(int x) { switch (x) { case 1: return 1; default: return 0; } } }\n";
        assert_eq!(complexity("csharp", "a.cs", cs), [("method:A.M".into(), 3)]);
    }
}
