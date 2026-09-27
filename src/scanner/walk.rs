use crate::error::Result;
use crate::scanner::filter::PathFilter;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    pub absolute_path: PathBuf,
    pub relative_path: String,
}

pub fn discover_files(root: &Path, filter: &PathFilter) -> Result<Vec<DiscoveredFile>> {
    let mut files = Vec::new();

    let walker = WalkDir::new(root).into_iter().filter_entry(|entry| {
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if filter.is_ignored_dir_name(name) {
                    return false;
                }
            }
        }
        true
    });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // Ignore permission/unreadable errors during traversal
        };

        if entry.file_type().is_file() {
            let abs_path = entry.path().to_path_buf();
            if let Ok(rel) = abs_path.strip_prefix(root) {
                if filter.is_ignored_path(rel) {
                    continue;
                }
                // Normalize separators to forward slash
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                files.push(DiscoveredFile {
                    absolute_path: abs_path,
                    relative_path: rel_str,
                });
            }
        }
    }

    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_discover_files_excludes_git_and_inspectye() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), "gitconfig").unwrap();

        fs::create_dir_all(root.join(".inspectye")).unwrap();
        fs::write(root.join(".inspectye/inspectye.db"), "db").unwrap();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("README.md"), "# Hello").unwrap();

        let filter = PathFilter::default();
        let discovered = discover_files(root, &filter).unwrap();

        let rel_paths: Vec<&str> = discovered
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert_eq!(rel_paths, vec!["README.md", "src/main.rs"]);
    }
}
