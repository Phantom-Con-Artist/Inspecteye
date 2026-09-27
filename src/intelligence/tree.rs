//! The project's directory hierarchy, built from the index.

use serde::Serialize;
use std::collections::BTreeMap;

/// What the index knows about a file shown in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeFile {
    pub kind: String,
    pub language: Option<String>,
    pub lines: Option<i64>,
    pub size_bytes: i64,
    pub git_tracked: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeNode {
    pub name: String,
    /// Project-relative path (`""` for the project root).
    pub path: String,
    /// Set for files, `None` for directories.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<TreeFile>,
    /// Directories first, then files, each alphabetical.
    pub children: Vec<TreeNode>,
    /// Files at any depth below this node (1 for a file).
    pub file_count: usize,
    /// Children were cut off by the depth limit.
    pub truncated: bool,
}

/// Builds a tree rooted at `base` (a project-relative directory, `""` for
/// the project root) from `(path, file)` entries under it. With
/// `max_depth`, directories deeper than that are shown collapsed; with
/// `directories_only`, files are counted but not listed.
pub fn build_tree(
    root_name: &str,
    base: &str,
    entries: Vec<(String, TreeFile)>,
    max_depth: Option<usize>,
    directories_only: bool,
) -> TreeNode {
    #[derive(Default)]
    struct Dir {
        dirs: BTreeMap<String, Dir>,
        files: BTreeMap<String, TreeFile>,
    }
    let mut root = Dir::default();
    let prefix = if base.is_empty() {
        String::new()
    } else {
        format!("{base}/")
    };
    for (path, file) in entries {
        let Some(rel) = path.strip_prefix(&prefix) else {
            continue;
        };
        let mut parts: Vec<&str> = rel.split('/').collect();
        let name = parts.pop().unwrap_or_default().to_string();
        let mut dir = &mut root;
        for p in parts {
            dir = dir.dirs.entry(p.to_string()).or_default();
        }
        dir.files.insert(name, file);
    }

    fn convert(
        name: String,
        path: String,
        dir: Dir,
        depth: usize,
        max_depth: Option<usize>,
        directories_only: bool,
    ) -> TreeNode {
        let join = |n: &str| {
            if path.is_empty() {
                n.to_string()
            } else {
                format!("{path}/{n}")
            }
        };
        let mut dirs: Vec<TreeNode> = dir
            .dirs
            .into_iter()
            .map(|(n, d)| {
                convert(
                    n.clone(),
                    join(&n),
                    d,
                    depth + 1,
                    max_depth,
                    directories_only,
                )
            })
            .collect();
        dirs.sort_by_key(|d| d.name.to_lowercase());
        let mut files: Vec<TreeNode> = dir
            .files
            .into_iter()
            .map(|(n, f)| TreeNode {
                path: join(&n),
                name: n,
                file: Some(f),
                children: Vec::new(),
                file_count: 1,
                truncated: false,
            })
            .collect();
        files.sort_by_key(|f| f.name.to_lowercase());
        let file_count = dirs.iter().map(|d| d.file_count).sum::<usize>() + files.len();

        let mut children = dirs;
        if !directories_only {
            children.extend(files);
        }
        let truncated = max_depth.is_some_and(|m| depth >= m) && !children.is_empty();
        if truncated {
            children.clear();
        }
        TreeNode {
            name,
            path,
            file: None,
            children,
            file_count,
            truncated,
        }
    }

    convert(
        root_name.to_string(),
        base.to_string(),
        root,
        0,
        max_depth,
        directories_only,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f() -> TreeFile {
        TreeFile {
            kind: "source".into(),
            language: None,
            lines: Some(1),
            size_bytes: 1,
            git_tracked: true,
            ignored: false,
        }
    }

    fn entries(paths: &[&str]) -> Vec<(String, TreeFile)> {
        paths.iter().map(|p| (p.to_string(), f())).collect()
    }

    #[test]
    fn test_directories_first_and_counts() {
        let t = build_tree(
            "proj",
            "",
            entries(&["README.md", "src/b.rs", "src/a/x.rs", "Cargo.toml"]),
            None,
            false,
        );
        let names: Vec<&str> = t.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["src", "Cargo.toml", "README.md"]);
        assert_eq!(t.file_count, 4);
        assert_eq!(t.children[0].children[0].path, "src/a");
        assert_eq!(t.children[0].file_count, 2);
    }

    #[test]
    fn test_subtree_depth_and_directories_only() {
        let t = build_tree(
            "src",
            "src",
            entries(&["src/a/b/c.rs", "src/d.rs", "other/e.rs"]),
            Some(1),
            false,
        );
        assert_eq!(t.file_count, 2);
        let a = &t.children[0];
        assert!(a.truncated && a.children.is_empty());
        assert_eq!(a.file_count, 1);

        let t = build_tree(
            "src",
            "src",
            entries(&["src/a/c.rs", "src/d.rs"]),
            None,
            true,
        );
        assert_eq!(t.children.len(), 1);
        assert_eq!(t.file_count, 2);
    }
}
