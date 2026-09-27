use std::fs;
use std::path::Path;

pub const IGNORE_FILE_NAME: &str = ".inspecteyeignore";

#[derive(Debug, Clone, Default)]
pub struct InspectyeIgnore {
    patterns: Vec<String>,
}

impl InspectyeIgnore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_file(file_path: &Path) -> Self {
        if !file_path.is_file() {
            return Self::default();
        }

        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Self {
        let mut patterns = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Normalize path separators in pattern to forward slash
            let normalized = trimmed.replace('\\', "/");
            patterns.push(normalized);
        }

        Self { patterns }
    }

    pub fn add_pattern(&mut self, pattern: impl Into<String>) {
        let normalized = pattern.into().replace('\\', "/");
        self.patterns.push(normalized);
    }

    pub fn is_ignored(&self, rel_path: &str) -> bool {
        let normalized_path = rel_path.replace('\\', "/");
        let path_str = normalized_path.trim_start_matches('/');

        for pattern in &self.patterns {
            if Self::matches_pattern(pattern, path_str) {
                return true;
            }
        }

        false
    }

    fn matches_pattern(pattern: &str, path: &str) -> bool {
        let pat = pattern.trim_start_matches('/');

        // If pattern ends with '/', it targets directories
        if let Some(dir_prefix) = pat.strip_suffix('/') {
            if path == dir_prefix
                || path.starts_with(&format!("{dir_prefix}/"))
                || path.contains(&format!("/{dir_prefix}/"))
            {
                return true;
            }
        }

        // Exact match
        if pat == path {
            return true;
        }

        // Check if pattern is a file name match (e.g. *.tmp or temp.txt matching foo/bar/temp.txt)
        if !pat.contains('/') {
            let file_name = path.rsplit('/').next().unwrap_or(path);
            if Self::simple_glob_match(pat, file_name) {
                return true;
            }
        }

        // Full glob match
        Self::simple_glob_match(pat, path)
    }

    fn simple_glob_match(pattern: &str, text: &str) -> bool {
        if pattern == "*" || pattern == "**" {
            return true;
        }

        if pattern.contains("**") {
            let parts: Vec<&str> = pattern.split("**").collect();
            if parts.len() == 2 {
                let prefix = parts[0].trim_end_matches('/');
                let suffix = parts[1].trim_start_matches('/');

                let prefix_match = prefix.is_empty() || text.starts_with(prefix);
                let suffix_match = suffix.is_empty() || text.ends_with(suffix);

                return prefix_match && suffix_match;
            }
        }

        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let prefix = parts[0];
                let suffix = parts[1];

                if text.starts_with(prefix) && text.ends_with(suffix) {
                    return text.len() >= prefix.len() + suffix.len();
                }
                return false;
            }
        }

        pattern == text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ignore_comments_and_blank_lines() {
        let content = "# This is a comment\n\n  \n*.tmp\n# Another comment";
        let ignore = InspectyeIgnore::parse(content);
        assert_eq!(ignore.patterns, vec!["*.tmp"]);
        assert!(ignore.is_ignored("cache/foo.tmp"));
        assert!(ignore.is_ignored("bar.tmp"));
        assert!(!ignore.is_ignored("foo.rs"));
    }

    #[test]
    fn test_ignore_directory_patterns() {
        let content = "vendor/\nnode_modules/\nbuild/";
        let ignore = InspectyeIgnore::parse(content);

        assert!(ignore.is_ignored("vendor/lib.rs"));
        assert!(ignore.is_ignored("src/vendor/helper.rs"));
        assert!(ignore.is_ignored("node_modules/pkg/index.js"));
        assert!(ignore.is_ignored("build/output.bin"));
        assert!(!ignore.is_ignored("src/lib.rs"));
    }

    #[test]
    fn test_ignore_specific_files_and_globs() {
        let content = "secret.env\n*.log\ntemp/**/data.json";
        let ignore = InspectyeIgnore::parse(content);

        assert!(ignore.is_ignored("secret.env"));
        assert!(ignore.is_ignored("config/secret.env"));
        assert!(ignore.is_ignored("app.log"));
        assert!(ignore.is_ignored("logs/server.log"));
        assert!(ignore.is_ignored("temp/2026/09/data.json"));
        assert!(!ignore.is_ignored("temp/data.txt"));
    }
}
