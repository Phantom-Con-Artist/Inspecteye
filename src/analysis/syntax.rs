//! Syntax diagnostics derived from a tree-sitter parse.
//!
//! Tree-sitter recovers from errors by inserting `MISSING` nodes (a token the
//! grammar required but did not find) and wrapping unparseable input in
//! `ERROR` nodes. Both are direct parser observations, so they are reported as
//! facts. When recovery fails badly the `ERROR` node covers most of the file
//! and its position says little about where the mistake is; that case is
//! reported as a warning with an explicit note.

use crate::analysis::language::ParsedFile;
use crate::issues::{
    render_excerpt, Certainty, Diagnostic, DiagnosticCategory, Severity, SourceRange,
};
use tree_sitter::Node;

/// An ERROR node spanning more than this share of the file is treated as
/// imprecise recovery rather than a located error.
const IMPRECISE_ERROR_SHARE: f64 = 0.6;
const MAX_EXPECTED_TOKENS: usize = 8;
const MAX_UNEXPECTED_TEXT: usize = 40;

pub fn syntax_diagnostics(parsed: &ParsedFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    collect_merge_conflicts(parsed, &mut out);
    if parsed.root().has_error() {
        let marker_lines: Vec<usize> = out.iter().map(|d| d.range.start_line).collect();
        let mut parser_errors = Vec::new();
        collect(parsed, parsed.root(), &mut parser_errors);
        // The conflict-marker finding already explains errors starting on those lines.
        parser_errors.retain(|d| !marker_lines.contains(&d.range.start_line));
        out.extend(parser_errors);
    }
    out.sort_by_key(|d| (d.range.start_byte, d.range.end_byte));
    out
}

fn collect(parsed: &ParsedFile, node: Node, out: &mut Vec<Diagnostic>) {
    if node.is_missing() {
        out.push(missing_node(parsed, node));
        return;
    }
    if node.is_error() {
        out.push(error_node(parsed, node));
        // Report nested MISSING nodes, but not nested ERROR nodes: the outer
        // node already covers them.
        report_missing_within(parsed, node, out);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.has_error() || child.is_missing() {
            collect(parsed, child, out);
        }
    }
}

fn report_missing_within(parsed: &ParsedFile, node: Node, out: &mut Vec<Diagnostic>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_missing() {
            out.push(missing_node(parsed, child));
        } else if child.has_error() {
            report_missing_within(parsed, child, out);
        }
    }
}

fn missing_node(parsed: &ParsedFile, node: Node) -> Diagnostic {
    let token = display_kind(node.kind());
    let range = SourceRange::from_node(node, &parsed.source);
    Diagnostic {
        category: DiagnosticCategory::Syntax,
        rule: "syntax.missing".to_string(),
        severity: Severity::Error,
        certainty: Certainty::Fact,
        message: format!("Missing {token}"),
        excerpt: render_excerpt(&parsed.source, &range),
        range,
        expected: vec![token],
        evidence: Some(format!(
            "The {} parser required {} here and inserted it to recover.",
            parsed.language,
            display_kind(node.kind())
        )),
        symbol: None,
    }
}

fn error_node(parsed: &ParsedFile, node: Node) -> Diagnostic {
    let range = SourceRange::from_node(node, &parsed.source);
    let total = parsed.source.len().max(1) as f64;
    let share = (node.end_byte() - node.start_byte()) as f64 / total;
    let imprecise = share > IMPRECISE_ERROR_SHARE && range.end_line > range.start_line + 5;

    let text = parsed.text(node).trim();
    let snippet = first_token(text);
    let expected = expected_tokens(node);

    // An ERROR wrapping complete constructs usually means something around
    // them is wrong (e.g. an unterminated statement), not the text itself.
    let wraps_constructs = (0..node.named_child_count())
        .filter_map(|i| node.named_child(i))
        .any(|c| c.child_count() > 0);
    let mut message = if snippet.is_empty() {
        "Unexpected end of input".to_string()
    } else if wraps_constructs {
        format!("Could not parse `{snippet}`")
    } else {
        format!("Unexpected `{snippet}`")
    };
    if !expected.is_empty() {
        message.push_str(&format!("; expected {}", expected.join(", ")));
    }

    let (severity, certainty, evidence) = if imprecise {
        (
            Severity::Warning,
            Certainty::Inference,
            format!(
                "The {} parser could not recover and marked lines {}-{} as unparseable; \
                 the mistake is somewhere in or before this region.",
                parsed.language, range.start_line, range.end_line
            ),
        )
    } else {
        (
            Severity::Error,
            Certainty::Fact,
            format!(
                "The {} parser could not fit this input into the grammar.",
                parsed.language
            ),
        )
    };

    Diagnostic {
        category: DiagnosticCategory::Syntax,
        rule: if imprecise {
            "syntax.unrecovered".to_string()
        } else {
            "syntax.unexpected".to_string()
        },
        severity,
        certainty,
        message,
        excerpt: render_excerpt(&parsed.source, &range),
        range,
        expected,
        evidence: Some(evidence),
        symbol: None,
    }
}

/// Git merge-conflict markers left in a file. These break every language, and
/// the parser's own error for them is usually confusing.
fn collect_merge_conflicts(parsed: &ParsedFile, out: &mut Vec<Diagnostic>) {
    let mut offset = 0;
    for (i, line) in parsed.source.split_inclusive('\n').enumerate() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let is_marker = trimmed.starts_with("<<<<<<< ")
            || trimmed == "<<<<<<<"
            || trimmed == "======="
            || trimmed.starts_with(">>>>>>> ")
            || trimmed == ">>>>>>>";
        if is_marker {
            let range = SourceRange {
                start_line: i + 1,
                start_column: 1,
                end_line: i + 1,
                end_column: trimmed.chars().count() + 1,
                start_byte: offset,
                end_byte: offset + trimmed.len(),
            };
            out.push(Diagnostic {
                category: DiagnosticCategory::Syntax,
                rule: "syntax.merge_conflict".to_string(),
                severity: Severity::Error,
                certainty: Certainty::Fact,
                message: "Unresolved merge-conflict marker".to_string(),
                excerpt: render_excerpt(&parsed.source, &range),
                range,
                expected: Vec::new(),
                evidence: Some(format!(
                    "Line starts with Git conflict marker `{}`.",
                    &trimmed[..7]
                )),
                symbol: None,
            });
        }
        offset += line.len();
    }
}

/// Tokens the parser could have accepted just before the error, from the
/// parse state of the preceding node. Omitted when the list is too long to help.
fn expected_tokens(error: Node) -> Vec<String> {
    let Some(prev) = error.prev_sibling() else {
        return Vec::new();
    };
    let language = error.language();
    let Some(mut lookahead) = language.lookahead_iterator(prev.next_parse_state()) else {
        return Vec::new();
    };
    let mut tokens: Vec<String> = Vec::new();
    for symbol in lookahead.by_ref() {
        if language.node_kind_is_visible(symbol) && !language.node_kind_is_named(symbol) {
            if let Some(kind) = language.node_kind_for_id(symbol) {
                let shown = format!("`{kind}`");
                if !kind.is_empty() && !tokens.contains(&shown) {
                    tokens.push(shown);
                }
            }
        }
        if tokens.len() > MAX_EXPECTED_TOKENS {
            return Vec::new();
        }
    }
    tokens
}

fn first_token(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("").trim();
    if first_line.chars().count() > MAX_UNEXPECTED_TEXT {
        let cut: String = first_line.chars().take(MAX_UNEXPECTED_TEXT).collect();
        format!("{cut}…")
    } else {
        first_line.to_string()
    }
}

fn display_kind(kind: &str) -> String {
    if kind.chars().all(|c| c.is_alphanumeric() || c == '_') {
        kind.replace('_', " ")
    } else {
        format!("`{kind}`")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::language::LanguageRegistry;
    use std::path::Path;

    fn diagnostics(lang: &str, path: &str, src: &str) -> Vec<Diagnostic> {
        let parsed = LanguageRegistry::default()
            .parse(lang, Path::new(path), src.to_string())
            .unwrap();
        syntax_diagnostics(&parsed)
    }

    #[test]
    fn test_clean_file_has_no_diagnostics() {
        assert!(diagnostics("rust", "a.rs", "fn main() { let x = 1; }\n").is_empty());
    }

    #[test]
    fn test_missing_token_is_confirmed_error() {
        let d = diagnostics(
            "typescript",
            "a.ts",
            "function f() {\n  return (1 + 2;\n}\n",
        );
        assert!(!d.is_empty());
        assert!(d.iter().any(|d| d.severity == Severity::Error));
        assert!(d.iter().all(|d| d.range.start_line >= 1));
    }

    #[test]
    fn test_unexpected_token_reports_location() {
        let d = diagnostics("python", "a.py", "def f(:\n    pass\n");
        let first = &d[0];
        assert_eq!(first.category, DiagnosticCategory::Syntax);
        assert_eq!(first.range.start_line, 1);
        assert!(first.excerpt.contains("def f(:"));
    }

    #[test]
    fn test_csharp_missing_semicolon() {
        let d = diagnostics(
            "csharp",
            "a.cs",
            "class A {\n  void M() {\n    int x = 1\n  }\n}\n",
        );
        // The C# grammar wraps the unterminated statement in an ERROR node
        // rather than inserting a MISSING `;`.
        let err = &d[0];
        assert_eq!(err.range.start_line, 3);
        assert_eq!(err.message, "Could not parse `int x = 1`");
        assert_eq!(err.severity, Severity::Error);
    }

    #[test]
    fn test_merge_conflict_markers() {
        let src = "fn a() {}\n<<<<<<< HEAD\nfn b() {}\n=======\nfn c() {}\n>>>>>>> branch\n";
        let d = diagnostics("rust", "a.rs", src);
        let markers: Vec<_> = d
            .iter()
            .filter(|d| d.rule == "syntax.merge_conflict")
            .collect();
        assert_eq!(markers.len(), 3);
        assert!(d
            .iter()
            .all(|d| d.rule == "syntax.merge_conflict" || d.range.start_line != 2));
        assert_eq!(markers[0].range.start_line, 2);
    }
}
