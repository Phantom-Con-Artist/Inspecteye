use inspecteye::analysis::{ChangeTag, TransitionType};
use inspecteye::error::InspectyeError;
use inspecteye::project::ProjectManager;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_unchanged_file_has_no_transition() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();
    // Second scan without modification -> unchanged
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/main.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Unchanged);
    assert!(transition.hunks.is_empty());
    let formatted = transition.format_terminal();
    assert!(formatted.contains("No pending or recorded changes for this file."));
    assert!(formatted.contains("File: src/main.rs"));
}

#[test]
fn test_line_level_addition() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {\n}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Add a line inside main
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"added line\");\n}\n",
    )
    .unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/main.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Modified);
    assert_eq!(transition.added_lines, 1);
    assert_eq!(transition.removed_lines, 0);
    assert_eq!(transition.hunks.len(), 1);

    let hunk = &transition.hunks[0];
    let inserted: Vec<_> = hunk
        .lines
        .iter()
        .filter(|l| l.tag == ChangeTag::Insert)
        .collect();
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0].content, "    println!(\"added line\");");
    assert_eq!(inserted[0].new_line_no, Some(2));

    let output = transition.format_terminal();
    assert!(output.contains("+    2      println!(\"added line\");"));
}

#[test]
fn test_line_level_removal() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    deprecated_call();\n    keep_call();\n}\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Remove deprecated_call
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    keep_call();\n}\n",
    )
    .unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/main.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Modified);
    assert_eq!(transition.added_lines, 0);
    assert_eq!(transition.removed_lines, 1);

    let hunk = &transition.hunks[0];
    let deleted: Vec<_> = hunk
        .lines
        .iter()
        .filter(|l| l.tag == ChangeTag::Delete)
        .collect();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].content, "    deprecated_call();");
    assert_eq!(deleted[0].old_line_no, Some(2));
}

#[test]
fn test_line_level_modification() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "let version = \"1.0.0\";\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Modify version
    fs::write(root.join("src/main.rs"), "let version = \"2.0.0\";\n").unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/main.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Modified);
    assert_eq!(transition.added_lines, 1);
    assert_eq!(transition.removed_lines, 1);

    let output = transition.format_terminal();
    assert!(output.contains("-    1  let version = \"1.0.0\";"));
    assert!(output.contains("+    1  let version = \"2.0.0\";"));
}

#[test]
fn test_multiple_separate_hunks() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let mut lines = Vec::new();
    for i in 1..=35 {
        lines.push(format!("    println!(\"line {}\");", i));
    }
    fs::write(root.join("src/main.rs"), lines.join("\n") + "\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Modify line 3 and line 32
    lines[2] = "    println!(\"line 3 CHANGED\");".to_string();
    lines[31] = "    println!(\"line 32 CHANGED\");".to_string();
    fs::write(root.join("src/main.rs"), lines.join("\n") + "\n").unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/main.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Modified);
    assert_eq!(transition.hunks.len(), 2);
}

#[test]
fn test_new_file_representation() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/new_file.rs"), "pub fn hello() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/new_file.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::New);
    assert!(transition.from_state.is_none());
    assert!(transition.to_state.exists_on_disk);
    assert_eq!(transition.to_state.line_count, Some(1));

    let output = transition.format_terminal();
    assert!(output.contains("Transition: New file (no previous state)"));
}

#[test]
fn test_removed_file_previous_state_available() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/old.rs");
    fs::write(&file, "pub fn old() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Delete file and scan
    fs::remove_file(&file).unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/old.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Removed);
    assert!(transition.from_state.is_some());
    let prev = transition.from_state.as_ref().unwrap();
    assert_eq!(prev.line_count, Some(1));

    let output = transition.format_terminal();
    assert!(output.contains("Transition: Removed"));
    assert!(output.contains("Current state: Unavailable (file does not exist on disk)"));
    assert!(output.contains("Previous state:"));
}

#[test]
fn test_file_restored_with_identical_content() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/auth.rs");

    // State A
    fs::write(&file, "fn auth() { bar(); }\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // State B
    fs::write(&file, "fn auth() { broken(); }\n").unwrap();
    manager.scan(root, "cli").unwrap();

    // State C (identical to State A)
    fs::write(&file, "fn auth() { bar(); }\n").unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/auth.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Restored);
    assert!(transition.restored_to_state.is_some());
    assert_eq!(
        transition.restored_to_state.as_ref().unwrap().content_hash,
        transition.to_state.content_hash
    );

    let output = transition.format_terminal();
    assert!(output.contains("Transition: Restored"));
    assert!(output.contains("-    1  fn auth() { broken(); }"));
    assert!(output.contains("+    1  fn auth() { bar(); }"));
}

#[test]
fn test_file_restored_with_different_content() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/mod.rs");

    fs::write(&file, "pub fn initial() {}\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Delete and scan
    fs::remove_file(&file).unwrap();
    manager.scan(root, "cli").unwrap();

    // Restore with different content
    fs::write(&file, "pub fn different() {}\n").unwrap();
    manager.scan(root, "cli").unwrap();

    let transition = manager.changes(root, "src/mod.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::RestoredModified);
}

#[test]
fn test_multiple_successive_changes_a_b_c() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/config.rs");

    // State A
    fs::write(&file, "port = 8080\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // State B
    fs::write(&file, "port = 9000\n").unwrap();
    manager.scan(root, "cli").unwrap();

    let trans_b = manager.changes(root, "src/config.rs").unwrap();
    assert_eq!(trans_b.transition_type, TransitionType::Modified);
    assert!(trans_b.format_terminal().contains("-    1  port = 8080"));
    assert!(trans_b.format_terminal().contains("+    1  port = 9000"));

    // State C
    fs::write(&file, "port = 9999\n").unwrap();
    manager.scan(root, "cli").unwrap();

    let trans_c = manager.changes(root, "src/config.rs").unwrap();
    assert_eq!(trans_c.transition_type, TransitionType::Modified);
    assert!(trans_c.format_terminal().contains("-    1  port = 9000"));
    assert!(trans_c.format_terminal().contains("+    1  port = 9999"));
}

#[test]
fn test_working_tree_changes_without_scan() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/worker.rs");

    fs::write(&file, "fn run() { start(); }\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Edit file directly in working tree, do NOT run scan!
    fs::write(&file, "fn run() { start(); log(); }\n").unwrap();

    let transition = manager.changes(root, "src/worker.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::Modified);
    assert_eq!(transition.to_state.source, "working_tree");

    let output = transition.format_terminal();
    assert!(output.contains("(working tree)"));
    assert!(output.contains("-    1  fn run() { start(); }"));
    assert!(output.contains("+    1  fn run() { start(); log(); }"));
}

#[test]
fn test_changes_only_reports_requested_file() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    let file_a = root.join("src/a.rs");
    let file_b = root.join("src/b.rs");

    fs::write(&file_a, "fn a1() {}\n").unwrap();
    fs::write(&file_b, "fn b1() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Modify BOTH files
    fs::write(&file_a, "fn a2_modified() {}\n").unwrap();
    fs::write(&file_b, "fn b2_modified() {}\n").unwrap();
    manager.scan(root, "cli").unwrap();

    // Request only a.rs
    let transition_a = manager.changes(root, "src/a.rs").unwrap();
    let out_a = transition_a.format_terminal();

    assert!(out_a.contains("File: src/a.rs"));
    assert!(out_a.contains("a2_modified"));
    // Must NOT contain anything from b.rs
    assert!(!out_a.contains("src/b.rs"));
    assert!(!out_a.contains("b2_modified"));
}

#[test]
fn test_unknown_path_produces_clear_error() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    let res = manager.changes(root, "src/nonexistent.rs");
    assert!(res.is_err());
    match res.err().unwrap() {
        InspectyeError::FileNotFound(msg) => {
            assert!(msg.contains("src/nonexistent.rs"));
            assert!(msg.contains("not tracked by Inspectye"));
        }
        other => panic!("Expected FileNotFound error, got: {:?}", other),
    }
}

#[test]
fn test_scan_summary_counts_edit_then_undo_as_restored() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("a.rs");

    fs::write(&file, "a\nb\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    fs::write(&file, "a\nB\n").unwrap();
    let changed = manager.scan(root, "cli").unwrap();
    assert_eq!(changed.changed_files, 1);
    assert_eq!(changed.restored_files, 0);

    fs::write(&file, "a\nb\n").unwrap();
    let undone = manager.scan(root, "cli").unwrap();
    assert_eq!(undone.changed_files, 0);
    assert_eq!(undone.restored_files, 1);
}

#[test]
fn test_removed_then_restored_with_changes_diffs_against_last_existing_state() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("a.rs");

    fs::write(&file, "keep\nold\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    fs::remove_file(&file).unwrap();
    manager.scan(root, "cli").unwrap();

    fs::write(&file, "keep\nnew\n").unwrap();
    let summary = manager.scan(root, "cli").unwrap();
    assert_eq!(summary.restored_changed_files, 1);

    let transition = manager.changes(root, "a.rs").unwrap();
    assert_eq!(transition.transition_type, TransitionType::RestoredModified);
    assert!(!transition.pending);
    assert_eq!(transition.added_lines, 1);
    assert_eq!(transition.removed_lines, 1);
    let output = transition.format_terminal();
    assert!(output.contains("-    2  old"));
    assert!(output.contains("+    2  new"));
}

#[test]
fn test_unscanned_edit_is_reported_as_pending() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("a.rs");

    fs::write(&file, "one\n").unwrap();
    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    fs::write(&file, "one\ntwo\n").unwrap();
    let pending = manager.changes(root, "a.rs").unwrap();
    assert!(pending.pending);
    assert!(pending
        .format_terminal()
        .contains("pending, not yet recorded"));

    manager.scan(root, "cli").unwrap();
    let recorded = manager.changes(root, "a.rs").unwrap();
    assert!(!recorded.pending);
    assert_eq!(recorded.transition_type, TransitionType::Modified);
    assert!(recorded.format_terminal().contains("last recorded change"));
}
