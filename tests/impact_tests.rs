use inspecteye::analysis::CodeAnalyzer;
use inspecteye::project::{ImpactReport, ProjectManager};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn setup(root: &Path) -> ProjectManager {
    fs::write(
        root.join("db.py"),
        "def open():\n    pass\n\ndef close():\n    pass\n",
    )
    .unwrap();
    fs::write(
        root.join("users.py"),
        "from db import open\n\ndef load():\n    open()\n",
    )
    .unwrap();
    fs::write(
        root.join("jobs.py"),
        "from db import close\n\ndef stop():\n    close()\n",
    )
    .unwrap();
    fs::write(
        root.join("api.py"),
        "from users import load\n\ndef get():\n    load()\n",
    )
    .unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    manager
}

fn affected(report: &ImpactReport) -> Vec<(String, usize)> {
    report
        .affected
        .iter()
        .map(|a| (report.file_paths[&a.file_id].clone(), a.depth))
        .collect()
}

#[test]
fn test_symbol_level_impact_follows_only_users_of_the_symbol() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let manager = setup(root);

    let report = manager
        .impact(root, &["db.py".to_string()], &["open".to_string()], 3)
        .unwrap();
    // jobs.py uses close(), not open(): not affected.
    assert_eq!(
        affected(&report),
        [("users.py".to_string(), 1), ("api.py".to_string(), 2)]
    );

    let whole = manager
        .impact(root, &["db.py".to_string()], &[], 1)
        .unwrap();
    let files: Vec<String> = affected(&whole).into_iter().map(|(p, _)| p).collect();
    assert_eq!(files, ["jobs.py", "users.py"]);
}

#[test]
fn test_pending_change_impact_flags_references_to_removed_declarations() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let manager = setup(root);

    // Remove close(), edit open(): not yet scanned.
    fs::write(root.join("db.py"), "def open():\n    return 1\n").unwrap();
    let report = manager.impact_of_changes(root, 1).unwrap();

    assert_eq!(report.origins.len(), 1);
    let origin = &report.origins[0];
    assert_eq!(origin.source, "pending");
    assert_eq!(origin.modified_symbols, ["open"]);
    assert_eq!(origin.removed_symbols, ["close"]);

    let jobs = report
        .affected
        .iter()
        .find(|a| report.file_paths[&a.file_id] == "jobs.py")
        .unwrap();
    assert!(jobs.references_removed_symbol);
    let users = report
        .affected
        .iter()
        .find(|a| report.file_paths[&a.file_id] == "users.py")
        .unwrap();
    assert!(!users.references_removed_symbol);
}

#[test]
fn test_no_changes_reports_nothing() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let manager = setup(root);
    let report = manager.impact_of_changes(root, 3).unwrap();
    assert!(report.origins.is_empty());
    assert!(report.affected.is_empty());
}
