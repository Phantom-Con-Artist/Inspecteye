use crate::error::{InspectyeError, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Line churn of one file over a range of commits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FileChurn {
    pub path: String,
    pub commits: usize,
    pub added: usize,
    pub deleted: usize,
}

/// One commit touching a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    /// ISO 8601 author date.
    pub date: String,
    pub subject: String,
    /// The file's path (relative to the project root) in this commit; it
    /// differs from today's path when the file was renamed since.
    pub path: String,
}

impl CommitInfo {
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(10)]
    }
}

pub trait GitProvider: Send + Sync {
    fn is_git_repository(&self, dir: &Path) -> bool;
    fn get_repository_root(&self, dir: &Path) -> Option<PathBuf>;
    fn get_tracked_files(&self, repo_root: &Path) -> Result<HashSet<String>>;
    fn get_head_commit(&self, repo_root: &Path) -> Option<String>;

    /// Commits touching `path` (project-relative), newest first, following renames.
    fn file_history(&self, _root: &Path, _path: &str, _limit: usize) -> Result<Vec<CommitInfo>> {
        Ok(Vec::new())
    }

    /// Content of `path` at `revision`; `None` if it did not exist there.
    fn file_at(&self, _root: &Path, _revision: &str, _path: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// Per-file commit counts and line churn over the last `limit` commits,
    /// for files under `root`.
    fn churn(&self, _root: &Path, _limit: usize) -> Result<Vec<FileChurn>> {
        Ok(Vec::new())
    }

    /// Full commit hash for a revision expression (`HEAD~2`, a tag, a hash).
    fn resolve_revision(&self, _root: &Path, _revision: &str) -> Option<String> {
        None
    }
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
        let output = git_command()
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
        let output = git_command()
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
        let output = git_command()
            .arg("-C")
            .arg(repo_root)
            .arg("ls-files")
            .arg("-z")
            .output()
            .map_err(|e| InspectyeError::Git(format!("Failed to execute 'git ls-files': {e}")))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(InspectyeError::Git(format!("git ls-files failed: {err}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tracked = HashSet::new();
        // -z gives NUL-separated, unquoted paths, so non-ASCII names stay intact.
        for entry in stdout.split('\0') {
            let clean = entry.trim_end_matches(['\r', '\n']);
            if !clean.is_empty() {
                // Normalize slashes
                let normalized = clean.replace('\\', "/");
                tracked.insert(normalized);
            }
        }

        Ok(tracked)
    }

    fn file_history(&self, root: &Path, path: &str, limit: usize) -> Result<Vec<CommitInfo>> {
        // \x1e separates records, \x1f fields; --name-only appends the path
        // the file had in each commit.
        let output = git_command()
            .arg("-C")
            .arg(root)
            .args(["-c", "core.quotePath=false", "log", "--follow"])
            .arg(format!("-n{limit}"))
            .arg("--format=%x1e%H%x1f%an%x1f%aI%x1f%s")
            .arg("--name-only")
            .arg("--relative")
            .arg("--")
            .arg(path)
            .output()
            .map_err(|e| InspectyeError::Git(format!("Failed to execute 'git log': {e}")))?;
        if !output.status.success() {
            return Err(InspectyeError::Git(format!(
                "git log failed for '{path}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut commits = Vec::new();
        for record in stdout.split('\x1e').filter(|r| !r.trim().is_empty()) {
            let (header, names) = record.split_once('\n').unwrap_or((record, ""));
            let fields: Vec<&str> = header.split('\x1f').collect();
            if fields.len() < 4 {
                continue;
            }
            let file_path = names
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or(path)
                .replace('\\', "/");
            commits.push(CommitInfo {
                hash: fields[0].to_string(),
                author: fields[1].to_string(),
                date: fields[2].to_string(),
                subject: fields[3].to_string(),
                path: file_path,
            });
        }
        Ok(commits)
    }

    fn file_at(&self, root: &Path, revision: &str, path: &str) -> Result<Option<String>> {
        let output = git_command()
            .arg("-C")
            .arg(root)
            .arg("show")
            // `./` makes the path relative to the project root, which may be
            // a subdirectory of the repository.
            .arg(format!("{revision}:./{path}"))
            .output()
            .map_err(|e| InspectyeError::Git(format!("Failed to execute 'git show': {e}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
    }

    fn churn(&self, root: &Path, limit: usize) -> Result<Vec<FileChurn>> {
        let output = git_command()
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "core.quotePath=false",
                "log",
                "--no-renames",
                "--numstat",
                "--relative",
            ])
            .arg(format!("-n{limit}"))
            .arg("--format=%x1e")
            .output()
            .map_err(|e| InspectyeError::Git(format!("Failed to execute 'git log': {e}")))?;
        if !output.status.success() {
            // An empty repository has no history; that is not an error.
            return Ok(Vec::new());
        }
        let mut by_path: std::collections::BTreeMap<String, FileChurn> = Default::default();
        for commit in String::from_utf8_lossy(&output.stdout).split('\x1e') {
            for line in commit.lines() {
                let mut parts = line.splitn(3, '\t');
                let (Some(a), Some(d), Some(p)) = (parts.next(), parts.next(), parts.next()) else {
                    continue;
                };
                let entry = by_path.entry(p.replace('\\', "/")).or_default();
                entry.commits += 1;
                // Binary files report `-`.
                entry.added += a.parse::<usize>().unwrap_or(0);
                entry.deleted += d.parse::<usize>().unwrap_or(0);
            }
        }
        Ok(by_path
            .into_iter()
            .map(|(path, mut c)| {
                c.path = path;
                c
            })
            .collect())
    }

    fn resolve_revision(&self, root: &Path, revision: &str) -> Option<String> {
        let output = git_command()
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{revision}^{{commit}}"))
            .output()
            .ok()?;
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && !hash.is_empty()).then_some(hash)
    }

    fn get_head_commit(&self, repo_root: &Path) -> Option<String> {
        let output = git_command()
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

/// A `git` process that never opens a console window, so GUI hosts (the
/// dashboard) do not flash one per Git call on Windows.
fn git_command() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
