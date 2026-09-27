use std::fs;
use std::path::Path;

pub const IGNORE_FILE_NAME: &str = ".inspecteyeignore";

/// `.inspecteyeignore` rules, following `.gitignore` semantics:
///
/// - `#` starts a comment; blank lines are skipped
/// - `!pattern` re-includes a path excluded by an earlier pattern
/// - a trailing `/` matches directories only
/// - a pattern containing `/` (other than trailing) is anchored to the project root;
///   otherwise it matches a file or directory name at any depth
/// - `*` and `?` never match `/`; `**` matches across directories; `[abc]` classes
/// - the last matching pattern wins, and a file inside an ignored directory stays ignored
#[derive(Debug, Clone, Default)]
pub struct InspectyeIgnore {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    pattern: Vec<char>,
    negated: bool,
    dir_only: bool,
    anchored: bool,
}

impl InspectyeIgnore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_file(file_path: &Path) -> Self {
        match fs::read_to_string(file_path) {
            Ok(content) => Self::parse(&content),
            Err(_) => Self::default(),
        }
    }

    pub fn parse(content: &str) -> Self {
        let mut ignore = Self::default();
        for line in content.lines() {
            ignore.add_pattern(line);
        }
        ignore
    }

    pub fn add_pattern(&mut self, pattern: impl Into<String>) {
        let raw = pattern.into().replace('\\', "/");
        let mut p = raw.trim();
        if p.is_empty() || p.starts_with('#') {
            return;
        }
        let negated = p.starts_with('!');
        if negated {
            p = &p[1..];
        }
        let dir_only = p.ends_with('/');
        let p = p.trim_end_matches('/');
        let anchored = p.contains('/');
        let p = p.trim_start_matches('/');
        if p.is_empty() {
            return;
        }
        self.rules.push(Rule {
            pattern: p.chars().collect(),
            negated,
            dir_only,
            anchored,
        });
    }

    pub fn is_ignored(&self, rel_path: &str) -> bool {
        let normalized = rel_path.replace('\\', "/");
        let path = normalized.trim_start_matches('/');
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return false;
        }

        // An excluded ancestor directory cannot be re-included by a later rule
        // for its contents, matching Git.
        let mut prefix = String::new();
        for (i, segment) in segments.iter().enumerate() {
            if i > 0 {
                prefix.push('/');
            }
            prefix.push_str(segment);
            let is_dir = i + 1 < segments.len();
            if self.decide(&prefix, segment, is_dir) {
                return true;
            }
        }
        false
    }

    /// Last matching rule wins; no match means not ignored.
    fn decide(&self, path: &str, name: &str, is_dir: bool) -> bool {
        let path: Vec<char> = path.chars().collect();
        let name: Vec<char> = name.chars().collect();
        let mut ignored = false;
        for rule in &self.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let target = if rule.anchored { &path } else { &name };
            if glob_match(&rule.pattern, target) {
                ignored = !rule.negated;
            }
        }
        ignored
    }
}

fn glob_match(p: &[char], t: &[char]) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') if p.get(1) == Some(&'*') => {
            let rest = &p[2..];
            if rest.first() == Some(&'/') {
                // `**/` matches zero or more whole directories.
                let rest = &rest[1..];
                (0..=t.len())
                    .filter(|&i| i == 0 || t[i - 1] == '/')
                    .any(|i| glob_match(rest, &t[i..]))
            } else {
                (0..=t.len()).any(|i| glob_match(rest, &t[i..]))
            }
        }
        Some('*') => {
            let rest = &p[1..];
            for i in 0..=t.len() {
                if glob_match(rest, &t[i..]) {
                    return true;
                }
                if i < t.len() && t[i] == '/' {
                    break;
                }
            }
            false
        }
        Some('?') => !t.is_empty() && t[0] != '/' && glob_match(&p[1..], &t[1..]),
        Some('[') => match (parse_class(&p[1..]), t.first()) {
            (Some((matches, used)), Some(&c)) if c != '/' => {
                matches(c) && glob_match(&p[1 + used..], &t[1..])
            }
            (Some(_), _) => false,
            // Unterminated class: treat `[` literally.
            (None, Some('[')) => glob_match(&p[1..], &t[1..]),
            (None, _) => false,
        },
        Some('\\') if p.len() > 1 => t.first() == Some(&p[1]) && glob_match(&p[2..], &t[1..]),
        Some(&c) => t.first() == Some(&c) && glob_match(&p[1..], &t[1..]),
    }
}

/// Parses a character class body (after `[`). Returns a predicate and the
/// number of pattern chars consumed, including the closing `]`.
fn parse_class(p: &[char]) -> Option<(impl Fn(char) -> bool, usize)> {
    let mut i = 0;
    let negated = matches!(p.first(), Some('!') | Some('^'));
    if negated {
        i += 1;
    }
    let mut ranges = Vec::new();
    let start = i;
    while i < p.len() && (p[i] != ']' || i == start) {
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            ranges.push((p[i], p[i + 2]));
            i += 3;
        } else {
            ranges.push((p[i], p[i]));
            i += 1;
        }
    }
    if i >= p.len() {
        return None;
    }
    let matcher = move |c: char| ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi) != negated;
    Some((matcher, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ignore_comments_and_blank_lines() {
        let ignore = InspectyeIgnore::parse("# This is a comment\n\n  \n*.tmp\n# Another comment");
        assert_eq!(ignore.rules.len(), 1);
        assert!(ignore.is_ignored("cache/foo.tmp"));
        assert!(ignore.is_ignored("bar.tmp"));
        assert!(!ignore.is_ignored("foo.rs"));
    }

    #[test]
    fn test_ignore_directory_patterns() {
        let ignore = InspectyeIgnore::parse("vendor/\nnode_modules/\nbuild/");
        assert!(ignore.is_ignored("vendor/lib.rs"));
        assert!(ignore.is_ignored("src/vendor/helper.rs"));
        assert!(ignore.is_ignored("node_modules/pkg/index.js"));
        assert!(ignore.is_ignored("build/output.bin"));
        assert!(!ignore.is_ignored("src/lib.rs"));
        // Directory-only pattern does not match a file with that name.
        assert!(!ignore.is_ignored("src/build"));
    }

    #[test]
    fn test_ignore_specific_files_and_globs() {
        let ignore = InspectyeIgnore::parse("secret.env\n*.log\ntemp/**/data.json");
        assert!(ignore.is_ignored("secret.env"));
        assert!(ignore.is_ignored("config/secret.env"));
        assert!(ignore.is_ignored("app.log"));
        assert!(ignore.is_ignored("logs/server.log"));
        assert!(ignore.is_ignored("temp/2026/09/data.json"));
        assert!(ignore.is_ignored("temp/data.json"));
        assert!(!ignore.is_ignored("temp/data.txt"));
    }

    #[test]
    fn test_single_star_stays_in_one_segment() {
        let ignore = InspectyeIgnore::parse("src/*.rs");
        assert!(ignore.is_ignored("src/a.rs"));
        assert!(!ignore.is_ignored("src/deep/z.rs"));
        assert!(!ignore.is_ignored("other/src/a.rs"));
    }

    #[test]
    fn test_negation_last_match_wins() {
        let ignore = InspectyeIgnore::parse("*.ts\n!keep.ts");
        assert!(ignore.is_ignored("src/a.ts"));
        assert!(!ignore.is_ignored("src/keep.ts"));
    }

    #[test]
    fn test_negation_cannot_reinclude_inside_ignored_directory() {
        let ignore = InspectyeIgnore::parse("gen/\n!gen/keep.rs");
        assert!(ignore.is_ignored("gen/keep.rs"));
    }

    #[test]
    fn test_anchored_and_double_star_patterns() {
        let ignore = InspectyeIgnore::parse("/root_only.txt\n**/fixtures/**\ndocs/**/*.png");
        assert!(ignore.is_ignored("root_only.txt"));
        assert!(!ignore.is_ignored("sub/root_only.txt"));
        assert!(ignore.is_ignored("a/b/fixtures/x.json"));
        assert!(ignore.is_ignored("docs/img/a/logo.png"));
        assert!(ignore.is_ignored("docs/logo.png"));
    }

    #[test]
    fn test_question_mark_and_classes() {
        let ignore = InspectyeIgnore::parse("file?.txt\nlog[0-9].txt\n[!a]*.md");
        assert!(ignore.is_ignored("file1.txt"));
        assert!(!ignore.is_ignored("file10.txt"));
        assert!(ignore.is_ignored("log7.txt"));
        assert!(!ignore.is_ignored("logx.txt"));
        assert!(ignore.is_ignored("b.md"));
        assert!(!ignore.is_ignored("a.md"));
    }

    #[test]
    fn test_windows_separators_and_case_sensitivity() {
        let ignore = InspectyeIgnore::parse("Build\\");
        assert!(ignore.is_ignored("Build\\x.ts"));
        assert!(!ignore.is_ignored("build/x.ts"));
    }
}
