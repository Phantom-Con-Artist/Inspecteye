//! External filesystem references through the whole pipeline: extraction
//! during deep scan, evaluation on this machine, re-checking when files
//! outside the project change, and cleanup when the referencing file goes.

use inspecteye::analysis::CodeAnalyzer;
use inspecteye::database::StoredExternalReference;
use inspecteye::issues::Severity;
use inspecteye::project::ProjectManager;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Forward slashes work for Windows drive paths too and need no escaping.
fn literal(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn by_path<'a>(refs: &'a [StoredExternalReference], needle: &str) -> &'a StoredExternalReference {
    refs.iter()
        .find(|r| r.reference.path.contains(needle))
        .unwrap_or_else(|| panic!("no reference containing {needle}: {refs:#?}"))
}

#[test]
fn test_external_references_are_found_checked_and_rechecked() {
    let outside = tempdir().unwrap();
    let project = tempdir().unwrap();
    let root = project.path();
    let asset = outside.path().join("bear_butt.png");
    fs::write(&asset, b"png").unwrap();
    let missing = outside.path().join("no_dir").join("gone.png");

    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/assets.ts"),
        format!(
            "// {comment}\nexport const logo = \"{asset}\";\nexport const gone = \"{missing}\";\n",
            comment = literal(&missing),
            asset = literal(&asset),
            missing = literal(&missing),
        ),
    )
    .unwrap();
    fs::write(
        root.join("settings.json"),
        format!("{{\"logo\": \"{}\"}}\n", literal(&asset)),
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    let (_, deep) = manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    assert_eq!(deep.external_references.total, 3, "comment must be ignored");
    assert_eq!(deep.external_references.errors, 1);

    let refs = manager.external_references(root, &[]).unwrap();
    let logo = by_path(&refs, "bear_butt");
    let check = logo.check.as_ref().unwrap();
    assert_eq!(logo.file_path, "settings.json"); // sorted by path
    assert_eq!(check.rule, "external.dependency");
    assert_eq!(check.severity, Severity::Warning);
    assert!(!check.inside_repository);

    let gone = by_path(&refs, "gone.png");
    assert_eq!(gone.reference.range.start_line, 3);
    assert_eq!(gone.reference.symbol.as_deref(), Some("gone"));
    assert_eq!(gone.check.as_ref().unwrap().severity, Severity::Error);

    // The outside file disappears; the project does not change. The next
    // deep scan re-checks without re-analyzing.
    fs::remove_file(&asset).unwrap();
    let (_, deep) = manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    assert_eq!(deep.analyzed, 0);
    let refs = manager
        .external_references(root, &["src".to_string()])
        .unwrap();
    assert!(refs.iter().all(|r| r.file_path.starts_with("src/")));
    let logo = by_path(&refs, "bear_butt");
    assert_eq!(
        logo.check.as_ref().unwrap().rule,
        "external.missing_parent_exists"
    );

    // An on-demand check sees the file coming back immediately.
    fs::write(&asset, b"png").unwrap();
    let refs = manager.check_external_references(root, &[]).unwrap();
    assert_eq!(
        by_path(&refs, "bear_butt").check.as_ref().unwrap().rule,
        "external.dependency"
    );

    // Removing the referencing file removes its references.
    fs::remove_file(root.join("src/assets.ts")).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    let refs = manager.external_references(root, &[]).unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].file_path, "settings.json");
}

#[test]
fn test_absolute_path_into_the_repository_and_test_code() {
    let project = tempdir().unwrap();
    let root = project.path();
    fs::create_dir(root.join("tests")).unwrap();
    fs::write(root.join("logo.svg"), "<svg/>").unwrap();
    let inside = literal(&root.canonicalize().unwrap().join("logo.svg"))
        .trim_start_matches("//?/")
        .to_string();
    fs::write(root.join("app.py"), format!("LOGO = \"{inside}\"\n")).unwrap();
    fs::write(
        root.join("tests/test_app.py"),
        "FIXTURE = \"/no/such/root/fixture.png\"\nOTHER = \"Q:/no/such/fixture.png\"\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    let refs = manager.external_references(root, &[]).unwrap();

    let logo = by_path(&refs, "logo.svg");
    let check = logo.check.as_ref().unwrap();
    assert_eq!(check.rule, "external.absolute_into_repository");
    assert!(check.inside_repository);

    let fixture = by_path(&refs, "Q:/no/such");
    assert!(fixture.reference.in_test_code);
    assert_eq!(fixture.check.as_ref().unwrap().severity, Severity::Info);
}
