//! Finds absolute filesystem paths in a file, for external-reference
//! intelligence. Parsed code is searched in string literals only (comments
//! and docstrings are prose, not dependencies); other text files are
//! searched line by line, skipping comment lines.

use crate::analysis::language::ParsedFile;
use crate::intelligence::{
    find_paths, is_test_code, symbols_touching_lines, ExternalReference, ExtractedSymbol,
    ScanOptions,
};
use crate::issues::SourceRange;
use std::path::Path;
use tree_sitter::Node;

/// Languages where `~/` is commonly a module alias rather than a home path.
const TILDE_ALIAS_LANGUAGES: &[&str] =
    &["typescript", "javascript", "vue", "svelte", "css", "html"];

/// Configuration files that hold patterns, not paths.
fn holds_patterns(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    name.ends_with("ignore")
        || matches!(
            name.as_str(),
            ".gitattributes" | ".gitmodules" | ".editorconfig"
        )
}

/// Absolute paths in the string literals of a parsed file.
pub fn from_parsed(parsed: &ParsedFile, symbols: &[ExtractedSymbol]) -> Vec<ExternalReference> {
    let index = LineIndex::new(&parsed.source);
    let home_paths = !TILDE_ALIAS_LANGUAGES.contains(&parsed.language);
    let mut out = Vec::new();
    let mut stack = vec![parsed.root()];
    while let Some(node) = stack.pop() {
        if is_string_literal(node) {
            if !is_bare_statement(node) {
                let text = parsed.text(node);
                let opts = ScanOptions {
                    escaped: !is_raw_literal(node.kind(), text),
                    home_paths,
                };
                push_matches(
                    &mut out,
                    &index,
                    &parsed.source,
                    text,
                    node.start_byte(),
                    opts,
                    "string_literal",
                );
            }
            continue; // outermost literal only
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    out.sort_by_key(|r| r.range.start_byte);
    attach_symbols(&mut out, symbols);
    out
}

/// Absolute paths in a text file (configuration, or code in a language
/// without a parser). Lines that start a comment are skipped.
pub fn from_text(path: &Path, source: &str, language: Option<&str>) -> Vec<ExternalReference> {
    if holds_patterns(path) {
        return Vec::new();
    }
    let index = LineIndex::new(source);
    let opts = ScanOptions {
        escaped: matches!(language, Some("json")),
        home_paths: !language.is_some_and(|l| TILDE_ALIAS_LANGUAGES.contains(&l)),
    };
    let mut out = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if !is_comment_line(line) {
            push_matches(&mut out, &index, source, line, offset, opts, "text");
        }
        offset += line.len();
    }
    out
}

fn push_matches(
    out: &mut Vec<ExternalReference>,
    index: &LineIndex,
    source: &str,
    fragment: &str,
    offset: usize,
    opts: ScanOptions,
    context: &str,
) {
    for m in find_paths(fragment, opts) {
        let (start, end) = (offset + m.start, offset + m.end);
        out.push(ExternalReference {
            raw: fragment[m.start..m.end].to_string(),
            path: m.path,
            style: m.style,
            context: context.to_string(),
            range: index.range(source, start, end),
            symbol: None,
            in_test_code: false,
        });
    }
}

fn is_string_literal(node: Node) -> bool {
    let kind = node.kind();
    kind.contains("string") && !kind.contains("content") && !kind.contains("fragment")
}

/// A string that is a whole statement: a docstring or a directive such as
/// `"use strict"`.
fn is_bare_statement(node: Node) -> bool {
    node.parent()
        .is_some_and(|p| p.kind() == "expression_statement" && p.named_child_count() == 1)
}

/// Raw and verbatim literals do not use backslash escapes.
fn is_raw_literal(kind: &str, text: &str) -> bool {
    if kind.contains("raw") || kind.contains("verbatim") {
        return true;
    }
    let prefix: String = text
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || matches!(c, '@' | '$'))
        .collect();
    prefix.contains(['r', 'R', '@'])
}

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    [
        "#", ";", "//", "<!--", "/*", "*", "--", "::", "REM ", "rem ",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

/// Marks references that sit in test code (by file path or enclosing symbol).
pub fn mark_test_code(refs: &mut [ExternalReference], file_path: &Path) {
    let path = file_path.to_string_lossy().replace('\\', "/");
    for r in refs {
        r.in_test_code = is_test_code(&path, r.symbol.as_deref());
    }
}

fn attach_symbols(refs: &mut [ExternalReference], symbols: &[ExtractedSymbol]) {
    for r in refs {
        r.symbol = symbols_touching_lines(symbols, &[r.range.start_line])
            .first()
            .map(|s| s.qualified_name.clone());
    }
}

/// Byte offset → line/column lookup.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    fn position(&self, source: &str, byte: usize) -> (usize, usize) {
        let line = self.starts.partition_point(|s| *s <= byte) - 1;
        let column = source[self.starts[line]..byte].chars().count() + 1;
        (line + 1, column)
    }

    fn range(&self, source: &str, start: usize, end: usize) -> SourceRange {
        let (start_line, start_column) = self.position(source, start);
        let (end_line, end_column) = self.position(source, end);
        SourceRange {
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte: start,
            end_byte: end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::language::LanguageRegistry;

    fn parsed_paths(lang: &str, file: &str, src: &str) -> Vec<String> {
        let parsed = LanguageRegistry::default()
            .parse(lang, Path::new(file), src.to_string())
            .unwrap();
        from_parsed(&parsed, &[])
            .into_iter()
            .map(|r| r.path)
            .collect()
    }

    #[test]
    fn test_string_literals_not_comments() {
        let src = "// C:\\old\\thing.png\nconst logo = \"C:/assets/bear_butt.png\";\nconst x = `${a}/b`;\n";
        assert_eq!(
            parsed_paths("typescript", "a.ts", src),
            vec!["C:/assets/bear_butt.png"]
        );
    }

    #[test]
    fn test_escaping_follows_literal_kind() {
        let src = "fn f() { let a = \"C:\\\\data\\\\a.csv\"; let b = r\"C:\\data\\b.csv\"; }\n";
        assert_eq!(
            parsed_paths("rust", "a.rs", src),
            vec![r"C:\data\a.csv", r"C:\data\b.csv"]
        );
        let src = "class A { string p = @\"\\\\server\\share\\f.txt\"; }\n";
        assert_eq!(
            parsed_paths("csharp", "a.cs", src),
            vec![r"\\server\share\f.txt"]
        );
    }

    #[test]
    fn test_docstrings_are_skipped() {
        let src = "def f():\n    \"\"\"Reads C:\\\\data\\\\x.csv\"\"\"\n    return open(\"/etc/app.conf\")\n";
        assert_eq!(parsed_paths("python", "a.py", src), vec!["/etc/app.conf"]);
    }

    #[test]
    fn test_text_files_skip_comments_and_ignore_files() {
        let src =
            "# cache: /var/cache/old\nlog_dir: /var/log/app\nicon: \"C:\\\\icons\\\\a.ico\"\n";
        let refs = from_text(Path::new("config.yaml"), src, Some("yaml"));
        assert_eq!(
            refs.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec!["/var/log/app", r"C:\\icons\\a.ico"]
        );
        assert_eq!(refs[0].range.start_line, 2);
        assert_eq!(refs[0].range.start_column, 10);
        assert!(from_text(Path::new(".gitignore"), "/tmp/build\n", None).is_empty());
        let json = from_text(
            Path::new("a.json"),
            "{\"p\": \"C:\\\\x\\\\y.txt\"}\n",
            Some("json"),
        );
        assert_eq!(json[0].path, r"C:\x\y.txt");
    }
}
