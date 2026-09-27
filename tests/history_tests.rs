use inspecteye::intelligence::SymbolChangeKind;
use inspecteye::project::ProjectManager;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn git(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn commit(root: &Path, file: &str, content: &str, message: &str) {
    fs::write(root.join(file), content).unwrap();
    assert!(git(root, &["add", "-A"]));
    assert!(git(root, &["commit", "-q", "-m", message]));
}

/// A repository where `run` is added, moved by an unrelated edit, changed,
/// and the file renamed. Returns None when git is unavailable.
fn repo(root: &Path) -> Option<()> {
    if !git(root, &["init", "-q", "."]) {
        return None;
    }
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "core.autocrlf", "false"]);
    commit(root, "a.py", "def run():\n    return 1\n", "add run");
    commit(
        root,
        "a.py",
        "def other():\n    pass\n\ndef run():\n    return 1\n",
        "add other",
    );
    commit(
        root,
        "a.py",
        "def other():\n    pass\n\ndef run():\n    return 2\n",
        "change run",
    );
    assert!(git(root, &["mv", "a.py", "b.py"]));
    assert!(git(root, &["commit", "-q", "-m", "rename"]));
    Some(())
}

#[test]
fn test_symbol_history_follows_identity_and_renames() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    if repo(root).is_none() {
        eprintln!("git unavailable; skipping");
        return;
    }
    let manager = ProjectManager::default();
    manager.init(root).unwrap();

    let all = manager.history(root, "b.py", None, 50).unwrap();
    assert_eq!(all.entries.len(), 4, "rename is followed");

    let run = manager.history(root, "b.py", Some("run"), 50).unwrap();
    let changes: Vec<(SymbolChangeKind, String)> = run
        .entries
        .iter()
        .map(|e| {
            (
                e.symbol_change.as_ref().unwrap().change,
                e.commit.as_ref().unwrap().subject.clone(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            (SymbolChangeKind::Modified, "change run".to_string()),
            (SymbolChangeKind::Added, "add run".to_string())
        ]
    );
}

#[test]
fn test_compare_symbol_between_revisions_and_working_tree() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    if repo(root).is_none() {
        return;
    }
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    fs::write(
        root.join("b.py"),
        "def other():\n    pass\n\ndef run():\n    return 3\n",
    )
    .unwrap();

    // Revision before the rename: the old path is used for that commit.
    let cmp = manager
        .compare(root, "b.py", "HEAD~3", None, Some("run"))
        .unwrap();
    assert_eq!((cmp.added_lines, cmp.removed_lines), (1, 1));
    assert_eq!(cmp.to_line_offset, 3, "run starts at line 4 now");

    let file = manager
        .compare(root, "b.py", "HEAD", Some("working"), None)
        .unwrap();
    assert_eq!(file.symbol_changes.len(), 1);
    assert!(manager
        .compare(root, "b.py", "no-such-rev", None, None)
        .is_err());
}
