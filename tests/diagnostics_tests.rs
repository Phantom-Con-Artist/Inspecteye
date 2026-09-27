use inspecteye::analysis::CodeAnalyzer;
use inspecteye::issues::{DiagnosticCategory, Severity};
use inspecteye::project::ProjectManager;
use std::fs;
use tempfile::tempdir;

fn syntax(manager: &ProjectManager, root: &std::path::Path, paths: &[&str]) -> Vec<String> {
    let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    manager
        .diagnostics(root, Some(DiagnosticCategory::Syntax), &paths)
        .unwrap()
        .into_iter()
        .map(|d| format!("{}:{}", d.path, d.diagnostic.range.start_line))
        .collect()
}

#[test]
fn test_syntax_errors_are_persisted_and_refreshed_incrementally() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/ok.ts"), "export const a = 1;\n").unwrap();
    fs::write(root.join("src/bad.py"), "def f(:\n    pass\n").unwrap();

    let manager = ProjectManager::default();
    let analyzer = CodeAnalyzer::new();
    manager.init(root).unwrap();

    let (_, deep) = manager.refresh(root, &analyzer).unwrap();
    assert_eq!(deep.analyzed, 2);
    let found = manager
        .diagnostics(root, Some(DiagnosticCategory::Syntax), &[])
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "src/bad.py");
    assert_eq!(found[0].diagnostic.severity, Severity::Error);

    // Nothing changed: stored results are reused.
    let (_, deep) = manager.refresh(root, &analyzer).unwrap();
    assert_eq!(deep.analyzed, 0);
    assert_eq!(syntax(&manager, root, &[]), vec!["src/bad.py:1"]);

    // Fixing the file clears its diagnostics after re-analysis.
    fs::write(root.join("src/bad.py"), "def f():\n    pass\n").unwrap();
    let (_, deep) = manager.refresh(root, &analyzer).unwrap();
    assert_eq!(deep.analyzed, 1);
    assert!(syntax(&manager, root, &[]).is_empty());
}

#[test]
fn test_diagnostics_of_removed_or_ignored_files_are_cleared() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("a.rs"), "fn a( {}\n").unwrap();
    fs::write(root.join("b.rs"), "fn b( {}\n").unwrap();

    let manager = ProjectManager::default();
    let analyzer = CodeAnalyzer::new();
    manager.init(root).unwrap();
    manager.refresh(root, &analyzer).unwrap();
    assert_eq!(syntax(&manager, root, &[]).len(), 2);

    fs::remove_file(root.join("a.rs")).unwrap();
    fs::write(root.join(".inspecteyeignore"), "b.rs\n").unwrap();
    manager.refresh(root, &analyzer).unwrap();
    assert!(syntax(&manager, root, &[]).is_empty());
}

#[test]
fn test_diagnostics_filter_by_file_and_directory() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src/deep")).unwrap();
    fs::write(root.join("src/a.rs"), "fn a( {}\n").unwrap();
    fs::write(root.join("src/deep/b.rs"), "fn b( {}\n").unwrap();
    fs::write(root.join("srcx.rs"), "fn c( {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();

    assert_eq!(
        syntax(&manager, root, &["src/deep"]),
        vec!["src/deep/b.rs:1"]
    );
    assert_eq!(syntax(&manager, root, &["./src/a.rs"]), vec!["src/a.rs:1"]);
    // `src` must not select `srcx.rs`.
    assert_eq!(syntax(&manager, root, &["src"]).len(), 2);
    let abs = root.join("srcx.rs").to_string_lossy().into_owned();
    assert_eq!(syntax(&manager, root, &[&abs]), vec!["srcx.rs:1"]);
}
