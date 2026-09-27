use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PathFilter {
    ignored_dir_names: HashSet<String>,
}

impl Default for PathFilter {
    fn default() -> Self {
        let mut ignored = HashSet::new();
        ignored.insert(".inspectye".to_string());
        ignored.insert(".git".to_string());
        ignored.insert(".svn".to_string());
        ignored.insert(".hg".to_string());
        Self {
            ignored_dir_names: ignored,
        }
    }
}

impl PathFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_additional_ignored_dir(mut self, dir_name: impl Into<String>) -> Self {
        self.ignored_dir_names.insert(dir_name.into());
        self
    }

    pub fn is_ignored_dir_name(&self, name: &str) -> bool {
        self.ignored_dir_names.contains(name)
    }

    pub fn is_ignored_path(&self, rel_path: &Path) -> bool {
        for component in rel_path.components() {
            if let std::path::Component::Normal(os_str) = component {
                if let Some(s) = os_str.to_str() {
                    if self.is_ignored_dir_name(s) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_filter_ignores_git_and_inspectye() {
        let filter = PathFilter::default();
        assert!(filter.is_ignored_dir_name(".git"));
        assert!(filter.is_ignored_dir_name(".inspectye"));
        assert!(!filter.is_ignored_dir_name("src"));
        assert!(!filter.is_ignored_dir_name("tests"));

        assert!(filter.is_ignored_path(Path::new(".git/config")));
        assert!(filter.is_ignored_path(Path::new(".inspectye/inspectye.db")));
        assert!(filter.is_ignored_path(Path::new("sub/.git/HEAD")));
        assert!(!filter.is_ignored_path(Path::new("src/main.rs")));
    }
}
