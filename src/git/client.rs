use crate::error::{InspectyeError, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub trait GitProvider: Send + Sync {
    fn is_git_repository(&self, dir: &Path) -> bool;
    fn get_repository_root(&self, dir: &Path) -> Option<PathBuf>;
    fn get_tracked_files(&self, repo_root: &Path) -> Result<HashSet<String>>;
    fn get_head_commit(&self, repo_root: &Path) -> Option<String>;
}

#[derive(Debug, Default, Clone)]
pub struct GitCli;

impl GitCli {
    pub fn new() -> Self {
        Self
    }
}

impl GitProvider for GitCli {
    fn is_git_repository(&self, dir: &Path) -> bool {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("rev-parse")
            .arg("--is-inside-work-tree")
            .output();

        match output {
            Ok(out) => {
                out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true"
            }
            Err(_) => false,
        }
    }

    fn get_repository_root(&self, dir: &Path) -> Option<PathBuf> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("rev-parse")
            .arg("--show-toplevel")
            .output()
            .ok()?;

        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                return Some(PathBuf::from(path_str));
            }
        }
        None
    }

    fn get_tracked_files(&self, repo_root: &Path) -> Result<HashSet<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("ls-files")
            .output()
            .map_err(|e| InspectyeError::Git(format!("Failed to execute 'git ls-files': {e}")))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(InspectyeError::Git(format!("git ls-files failed: {err}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tracked = HashSet::new();
        for line in stdout.lines() {
            let clean = line.trim();
            if !clean.is_empty() {
                // Normalize slashes
                let normalized = clean.replace('\\', "/");
                tracked.insert(normalized);
            }
        }

        Ok(tracked)
    }

    fn get_head_commit(&self, repo_root: &Path) -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .ok()?;

        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !commit.is_empty() {
                return Some(commit);
            }
        }
        None
    }
}

#[derive(Debug, Default, Clone)]
pub struct MockGitProvider {
    pub is_repo: bool,
    pub root: Option<PathBuf>,
    pub tracked_files: HashSet<String>,
    pub head_commit: Option<String>,
}

impl MockGitProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_repo(mut self, root: PathBuf, tracked: &[&str]) -> Self {
        self.is_repo = true;
        self.root = Some(root);
        self.tracked_files = tracked.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_head_commit(mut self, commit: &str) -> Self {
        self.head_commit = Some(commit.to_string());
        self
    }
}

impl GitProvider for MockGitProvider {
    fn is_git_repository(&self, _dir: &Path) -> bool {
        self.is_repo
    }

    fn get_repository_root(&self, _dir: &Path) -> Option<PathBuf> {
        self.root.clone()
    }

    fn get_tracked_files(&self, _repo_root: &Path) -> Result<HashSet<String>> {
        Ok(self.tracked_files.clone())
    }

    fn get_head_commit(&self, _repo_root: &Path) -> Option<String> {
        self.head_commit.clone()
    }
}
