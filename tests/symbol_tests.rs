use inspecteye::analysis::CodeAnalyzer;
use inspecteye::intelligence::SymbolChangeKind;
use inspecteye::project::ProjectManager;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_symbols_keep_ids_and_record_history_across_edits() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("svc.ts");
    fs::write(&file, "export class Svc {\n  run() {}\n  stop() {}\n}\n").unwrap();

    let manager = ProjectManager::default();
    let analyzer = CodeAnalyzer::new();
    manager.init(root).unwrap();
    manager.refresh(root, &analyzer).unwrap();

    let before = manager.symbols(root, &[]).unwrap();
    let run_id = before
        .iter()
        .find(|s| s.symbol.identity == "method:Svc.run")
        .unwrap()
        .id;
    // First analysis is a baseline, not history.
    assert!(manager.symbol_history(root, &[]).unwrap().is_empty());

    fs::write(
        &file,
        "// header\nexport class Svc {\n  run() {}\n  stop() { cleanup(); }\n  pause() {}\n}\n",
    )
    .unwrap();
    manager.refresh(root, &analyzer).unwrap();

    let after = manager.symbols(root, &[]).unwrap();
    let run = after
        .iter()
        .find(|s| s.symbol.identity == "method:Svc.run")
        .unwrap();
    assert_eq!(run.id, run_id, "unchanged symbol keeps its id");
    assert_eq!(run.symbol.range.start_line, 3, "range follows the move");

    let history: Vec<(String, String)> = manager
        .symbol_history(root, &[])
        .unwrap()
        .into_iter()
        .map(|c| (c.identity, c.change))
        .collect();
    assert!(history.contains(&("method:Svc.stop".into(), "modified".into())));
    assert!(history.contains(&("method:Svc.pause".into(), "added".into())));
    assert!(!history.iter().any(|(id, _)| id == "method:Svc.run"));
}

#[test]
fn test_changes_maps_diff_to_symbols() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("m.py");
    fs::write(&file, "def a():\n    return 1\n\ndef b():\n    return 2\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    fs::write(&file, "def a():\n    return 1\n\ndef b():\n    return 3\n").unwrap();
    let t = manager.changes(root, "m.py").unwrap();
    assert!(t.pending);
    let changed: Vec<(&str, SymbolChangeKind)> = t
        .symbol_changes
        .iter()
        .map(|c| (c.qualified_name.as_str(), c.change))
        .collect();
    assert_eq!(changed, [("b", SymbolChangeKind::Modified)]);
    assert!(t.format_terminal().contains("Symbols changed:"));
}
