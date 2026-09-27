use super::Severity;
use serde::Serialize;

/// Which analysis produced a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticCategory {
    Syntax,
    Logic,
}

impl DiagnosticCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticCategory::Syntax => "syntax",
            DiagnosticCategory::Logic => "logic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "syntax" => Some(DiagnosticCategory::Syntax),
            "logic" => Some(DiagnosticCategory::Logic),
            _ => None,
        }
    }
}

/// How strongly the evidence supports a finding (AGENTS.md §18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Certainty {
    /// Directly observed (e.g. the parser required a token that is absent).
    Fact,
    /// Derived from facts by a rule that can be wrong in unusual code.
    Inference,
    /// Worth a look; legitimate code frequently looks like this.
    Possibility,
}

impl Certainty {
    pub fn as_str(&self) -> &'static str {
        match self {
            Certainty::Fact => "fact",
            Certainty::Inference => "inference",
            Certainty::Possibility => "possibility",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fact" => Some(Certainty::Fact),
            "inference" => Some(Certainty::Inference),
            "possibility" => Some(Certainty::Possibility),
            _ => None,
        }
    }
}

/// A region of a file. Lines and columns are 1-based; columns count characters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SourceRange {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

impl SourceRange {
    pub fn from_node(node: tree_sitter::Node, source: &str) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        Self {
            start_line: start.row + 1,
            start_column: char_column(source, node.start_byte(), start.column),
            end_line: end.row + 1,
            end_column: char_column(source, node.end_byte(), end.column),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
        }
    }
}

/// Converts a tree-sitter byte column to a 1-based character column.
fn char_column(source: &str, byte: usize, byte_column: usize) -> usize {
    let line_start = byte.saturating_sub(byte_column);
    source
        .get(line_start..byte)
        .map(|s| s.chars().count())
        .unwrap_or(byte_column)
        + 1
}

/// A finding located in a specific file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    /// Stable identifier of the rule that produced the finding, e.g. `syntax.missing`.
    pub rule: String,
    pub severity: Severity,
    pub certainty: Certainty,
    pub message: String,
    pub range: SourceRange,
    /// Source lines covering the range, with a marker line under the first one.
    pub excerpt: String,
    /// Tokens the parser would have accepted, when known.
    pub expected: Vec<String>,
    /// Why the finding was reported.
    pub evidence: Option<String>,
    /// Innermost enclosing symbol, when symbol data is available.
    pub symbol: Option<String>,
}

/// Renders the source lines of a range with a `^^^` marker under the first line.
pub fn render_excerpt(source: &str, range: &SourceRange) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let first = range.start_line.saturating_sub(1);
    let last = range.end_line.saturating_sub(1).min(first + 2);
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate().take(last + 1).skip(first) {
        let line = line.trim_end_matches('\r');
        out.push_str(&format!("{:>5} | {}\n", i + 1, line));
        if i == first {
            let start = range.start_column.saturating_sub(1);
            let width = if range.end_line == range.start_line {
                range.end_column.saturating_sub(range.start_column).max(1)
            } else {
                line.chars().count().saturating_sub(start).max(1)
            };
            out.push_str(&format!(
                "{:>5} | {}{}\n",
                "",
                " ".repeat(start),
                "^".repeat(width)
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_excerpt_marks_range() {
        let src = "let a = 1;\nlet b = ;\n";
        let range = SourceRange {
            start_line: 2,
            start_column: 9,
            end_line: 2,
            end_column: 10,
            ..Default::default()
        };
        let excerpt = render_excerpt(src, &range);
        assert_eq!(excerpt, "    2 | let b = ;\n      |         ^\n");
    }
}
