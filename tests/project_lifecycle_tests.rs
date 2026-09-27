use inspecteye::analysis::{compute_bytes_hash, compute_file_hash, BasicDeepAnalyzer};
use inspecteye::database::{open_database, Repository};
use inspecteye::git::MockGitProvider;
use inspecteye::project::{ProjectManager, DB_NAME, INSPECTYE_DIR};
use inspecteye::scanner::{discover_files, PathFilter};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_project_init_creates_inspectye_dir_and_db() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let manager = ProjectManager::default();
    let res = manager.init(root).unwrap();

    assert!(!res.already_initialized);
    assert_eq!(res.db_path, res.root_path.join(INSPECTYE_DIR).join(DB_NAME));
    assert!(res.db_path.is_file());

    // Verify database tables and project record
    let conn = open_database(&res.db_path).unwrap();
    let repo = Repository::new(conn);
    let project = repo
        .get_project_by_root(&res.root_path)
        .unwrap()
        .expect("project must exist");
    assert_eq!(project.root_path, res.root_path.to_string_lossy());
}

#[test]
fn test_project_init_safe_to_rerun() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let manager = ProjectManager::default();
    let res1 = manager.init(root).unwrap();
    assert!(!res1.already_initialized);

    let res2 = manager.init(root).unwrap();
    assert!(res2.already_initialized);
    assert_eq!(res1.root_path, res2.root_path);

    // Verify no duplicate records
    let conn = open_database(&res2.db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_file_discovery_and_exclusions() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    // Create project files
    fs::create_dir_all(root.join("src/nested")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/nested/helper.rs"), "pub fn help() {}\n").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname=\"foo\"\n").unwrap();

    // Create excluded dirs
    fs::create_dir_all(root.join(".git/objects/abc")).unwrap();
    fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(root.join(".git/objects/abc/commit"), "blob\n").unwrap();

    fs::create_dir_all(root.join(".inspectye")).unwrap();
    fs::write(root.join(".inspectye/inspectye.db"), "dummy db\n").unwrap();

    let filter = PathFilter::default();
    let discovered = discover_files(root, &filter).unwrap();

    let rel_paths: Vec<String> = discovered.into_iter().map(|f| f.relative_path).collect();
    assert_eq!(
        rel_paths,
        vec![
            "Cargo.toml".to_string(),
            "src/main.rs".to_string(),
            "src/nested/helper.rs".to_string()
        ]
    );
}

#[test]
fn test_content_hashing_deterministic() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("sample.txt");
    fs::write(&file_path, "deterministic content 12345").unwrap();

    let h1 = compute_file_hash(&file_path).unwrap();
    let h2 = compute_file_hash(&file_path).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);

    let in_memory_hash = compute_bytes_hash(b"deterministic content 12345");
    assert_eq!(h1, in_memory_hash);

    fs::write(&file_path, "modified content 12345").unwrap();
    let h3 = compute_file_hash(&file_path).unwrap();
    assert_ne!(h1, h3);
}

#[test]
fn test_scan_lifecycle_initial_second_modified_removed() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    // Step 1: Create initial files
    fs::create_dir_all(root.join("src")).unwrap();
    let file1 = root.join("src/a.rs");
    let file2 = root.join("src/b.rs");
    let file3 = root.join("README.md");
    fs::write(&file1, "fn a() {}\n").unwrap();
    fs::write(&file2, "fn b() {}\n").unwrap();
    fs::write(&file3, "# Project\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();

    // Step 2: Initial scan
    let scan1 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan1.files_discovered, 3);
    assert_eq!(scan1.new_files, 3);
    assert_eq!(scan1.changed_files, 0);
    assert_eq!(scan1.unchanged_files, 0);
    assert_eq!(scan1.removed_files, 0);

    // Step 3: Second scan with no changes
    let scan2 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan2.files_discovered, 3);
    assert_eq!(scan2.new_files, 0);
    assert_eq!(scan2.changed_files, 0);
    assert_eq!(scan2.unchanged_files, 3);
    assert_eq!(scan2.removed_files, 0);

    // Step 4: Modify one file
    fs::write(&file1, "fn a() { println!(\"updated\"); }\n").unwrap();
    let scan3 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan3.files_discovered, 3);
    assert_eq!(scan3.new_files, 0);
    assert_eq!(scan3.changed_files, 1);
    assert_eq!(scan3.unchanged_files, 2);
    assert_eq!(scan3.removed_files, 0);

    // Step 5: Add a new file
    let file4 = root.join("src/c.rs");
    fs::write(&file4, "fn c() {}\n").unwrap();
    let scan4 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan4.files_discovered, 4);
    assert_eq!(scan4.new_files, 1);
    assert_eq!(scan4.changed_files, 0);
    assert_eq!(scan4.unchanged_files, 3);
    assert_eq!(scan4.removed_files, 0);

    // Step 6: Remove a file
    fs::remove_file(&file2).unwrap();
    let scan5 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan5.files_discovered, 3);
    assert_eq!(scan5.new_files, 0);
    assert_eq!(scan5.changed_files, 0);
    assert_eq!(scan5.unchanged_files, 3);
    assert_eq!(scan5.removed_files, 1);

    // Verify DB reflects removed file
    let status = manager.status(root).unwrap();
    assert_eq!(status.stats.total_known_files, 4);
    assert_eq!(status.stats.discovered_files, 3);
    assert_eq!(status.stats.removed_files, 1);
}

#[test]
fn test_git_tracked_state_integration() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/tracked.rs"), "fn tracked() {}\n").unwrap();
    fs::write(root.join("src/untracked.rs"), "fn untracked() {}\n").unwrap();

    let git_mock = MockGitProvider::new().with_repo(root.to_path_buf(), &["src/tracked.rs"]);

    let manager = ProjectManager::new(git_mock);
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    let status = manager.status(root).unwrap();
    assert!(status.git_active);
    assert_eq!(status.stats.discovered_files, 2);
    assert_eq!(status.stats.git_tracked_files, 1);
    assert_eq!(status.stats.untracked_files, 1);
}

#[test]
fn test_database_persistence_across_sessions() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::write(root.join("a.txt"), "hello").unwrap();

    {
        let manager1 = ProjectManager::default();
        manager1.init(root).unwrap();
        manager1.scan(root, "cli").unwrap();
    } // manager1 drops and SQLite connection closes

    // Create a new independent manager instance
    let manager2 = ProjectManager::default();
    let status = manager2.status(root).unwrap();
    assert_eq!(status.stats.total_known_files, 1);
    assert_eq!(status.stats.discovered_files, 1);
    assert!(status.last_run.is_some());
    let run = status.last_run.unwrap();
    assert_eq!(run.status, "completed");
    assert_eq!(run.files_scanned, 1);
    assert_eq!(run.files_new, 1);
    assert_eq!(run.files_changed, 0);
    assert_eq!(run.files_unchanged, 0);
    assert_eq!(run.files_restored, 0);
    assert_eq!(run.files_restored_changed, 0);
    assert_eq!(run.files_removed, 0);
}

#[test]
fn test_restore_unchanged_file_lifecycle() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let file_path = root.join("service.rs");
    fs::write(&file_path, "pub fn serve() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();

    // Initial scan: new file
    let scan1 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan1.files_discovered, 1);
    assert_eq!(scan1.new_files, 1);
    assert_eq!(scan1.restored_files, 0);

    // File deleted -> scan detects removed
    fs::remove_file(&file_path).unwrap();
    let scan2 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan2.files_discovered, 0);
    assert_eq!(scan2.removed_files, 1);

    // Verify DB still holds record as exists_on_disk = 0 (not deleted)
    let status_after_del = manager.status(root).unwrap();
    assert_eq!(status_after_del.stats.total_known_files, 1);
    assert_eq!(status_after_del.stats.discovered_files, 0);
    assert_eq!(status_after_del.stats.removed_files, 1);

    // Restore file with exact same content -> scan detects Restored
    fs::write(&file_path, "pub fn serve() {}\n").unwrap();
    let scan3 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan3.files_discovered, 1);
    assert_eq!(scan3.new_files, 0);
    assert_eq!(scan3.changed_files, 0);
    assert_eq!(scan3.unchanged_files, 0);
    assert_eq!(scan3.restored_files, 1);
    assert_eq!(scan3.restored_changed_files, 0);
    assert_eq!(scan3.removed_files, 0);

    // Status reflects restored file back in active discovered set
    let status_after_restore = manager.status(root).unwrap();
    assert_eq!(status_after_restore.stats.total_known_files, 1);
    assert_eq!(status_after_restore.stats.discovered_files, 1);
    assert_eq!(status_after_restore.stats.removed_files, 0);
    let last_run = status_after_restore.last_run.unwrap();
    assert_eq!(last_run.files_restored, 1);
    assert_eq!(last_run.files_restored_changed, 0);
}

#[test]
fn test_restore_with_modified_content_lifecycle() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let file_path = root.join("config.toml");
    fs::write(&file_path, "version = 1\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();

    // Initial scan
    let scan1 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan1.new_files, 1);

    // Delete file
    fs::remove_file(&file_path).unwrap();
    let scan2 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan2.removed_files, 1);

    // Restore file at same path but with different content
    fs::write(&file_path, "version = 2\n[updated]\n").unwrap();
    let scan3 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan3.files_discovered, 1);
    assert_eq!(scan3.new_files, 0);
    assert_eq!(scan3.changed_files, 0);
    assert_eq!(scan3.unchanged_files, 0);
    assert_eq!(scan3.restored_files, 0);
    assert_eq!(scan3.restored_changed_files, 1);
    assert_eq!(scan3.removed_files, 0);

    let status = manager.status(root).unwrap();
    assert_eq!(status.stats.discovered_files, 1);
    assert_eq!(status.stats.removed_files, 0);
    let last_run = status.last_run.unwrap();
    assert_eq!(last_run.files_restored, 0);
    assert_eq!(last_run.files_restored_changed, 1);
}

#[test]
fn test_genuinely_new_file_reports_new() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::write(root.join("initial.rs"), "fn init() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Add a genuinely brand new file
    fs::write(root.join("brand_new.rs"), "fn brand_new() {}\n").unwrap();
    let scan2 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan2.files_discovered, 2);
    assert_eq!(scan2.new_files, 1);
    assert_eq!(scan2.changed_files, 0);
    assert_eq!(scan2.unchanged_files, 1);
    assert_eq!(scan2.restored_files, 0);
    assert_eq!(scan2.restored_changed_files, 0);
}

#[test]
fn test_resource_discovery_and_classification() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("vendor")).unwrap();
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();

    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("config.json"), "{\"app\": \"test\"}\n").unwrap();
    fs::write(root.join("README.md"), "# Test Project\n").unwrap();
    fs::write(root.join("assets/icon.png"), [0x89, 0x50, 0x4E, 0x47]).unwrap();
    fs::write(
        root.join("vendor/thirdparty.js"),
        "console.log('vendor');\n",
    )
    .unwrap();
    fs::write(root.join("dist/bundle.js"), "console.log('bundled');\n").unwrap();
    fs::write(root.join("binary.bin"), [0x00, 0x01, 0x02, 0x03]).unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    let scan_res = manager.scan(root, "cli").unwrap();

    assert_eq!(scan_res.files_discovered, 7);
    assert_eq!(scan_res.resources.source, 1);
    assert_eq!(scan_res.resources.config, 1);
    assert_eq!(scan_res.resources.documentation, 1);
    assert_eq!(scan_res.resources.asset, 1);
    assert_eq!(scan_res.resources.dependency, 1);
    assert_eq!(scan_res.resources.generated, 1);
    assert_eq!(scan_res.resources.binary, 1);

    // Verify stored kinds in SQLite
    let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);
    let conn = open_database(&db_path).unwrap();
    let repo = Repository::new(conn);
    let project = repo.get_project_by_root(root).unwrap().unwrap();
    let files = repo.get_all_files(project.id).unwrap();
    let kind_map: std::collections::HashMap<_, _> = files
        .into_iter()
        .map(|f| (f.path.replace('\\', "/"), f.kind))
        .collect();

    assert_eq!(kind_map.get("src/main.rs").unwrap(), "source");
    assert_eq!(kind_map.get("config.json").unwrap(), "config");
    assert_eq!(kind_map.get("README.md").unwrap(), "documentation");
    assert_eq!(kind_map.get("assets/icon.png").unwrap(), "asset");
    assert_eq!(kind_map.get("vendor/thirdparty.js").unwrap(), "dependency");
    assert_eq!(kind_map.get("dist/bundle.js").unwrap(), "generated");
    assert_eq!(kind_map.get("binary.bin").unwrap(), "binary");
}

#[test]
fn test_inspecteyeignore_controls_analysis_depth_not_visibility() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();

    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("scripts/helper.rs"), "fn helper() {}\n").unwrap();
    fs::write(
        root.join(".inspecteyeignore"),
        "# ignore helper script\nscripts/helper.rs\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    let scan_res = manager.scan(root, "cli").unwrap();

    // All 3 files must be discovered (main.rs, helper.rs, and .inspecteyeignore itself)
    assert_eq!(scan_res.files_discovered, 3);

    let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);
    let conn = open_database(&db_path).unwrap();
    let repo = Repository::new(conn);
    let project = repo.get_project_by_root(root).unwrap().unwrap();

    let helper = repo
        .get_file_by_path(project.id, "scripts/helper.rs")
        .unwrap()
        .expect("helper.rs should be indexed in SQLite");
    assert!(
        helper.is_ignored,
        "helper.rs should be marked is_ignored = true"
    );

    let main = repo
        .get_file_by_path(project.id, "src/main.rs")
        .unwrap()
        .expect("main.rs should be indexed in SQLite");
    assert!(!main.is_ignored, "main.rs should NOT be marked is_ignored");
}

#[test]
fn test_deep_scan_analyzes_eligible_skips_ineligible_and_ignored() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("vendor")).unwrap();

    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .unwrap();
    fs::write(root.join("src/ignored.rs"), "fn ignored() {}\n").unwrap();
    fs::write(root.join("config.json"), "{\"key\": \"val\"}\n").unwrap();
    fs::write(root.join("vendor/dep.js"), "var x = 1;\n").unwrap();
    fs::write(root.join(".inspecteyeignore"), "src/ignored.rs\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    // Deep-scan with BasicDeepAnalyzer
    let analyzer = BasicDeepAnalyzer::default();
    let deep_res = manager.deep_scan(root, &analyzer).unwrap();

    // 5 total files indexed:
    // - src/main.rs: eligible (source, not ignored, exists) -> ANALYZED
    // - src/ignored.rs: is_ignored -> INELIGIBLE
    // - config.json: kind="config" -> INELIGIBLE
    // - vendor/dep.js: kind="dependency" -> INELIGIBLE
    // - .inspecteyeignore: kind="config" -> INELIGIBLE
    assert_eq!(deep_res.total_eligible, 1);
    assert_eq!(deep_res.analyzed, 1);
    assert_eq!(deep_res.skipped, 0);
    assert_eq!(deep_res.ineligible, 4);
    assert_eq!(deep_res.failed, 0);

    // Verify file_analysis in SQLite
    let db_path = root.join(INSPECTYE_DIR).join(DB_NAME);
    let conn = open_database(&db_path).unwrap();
    let repo = Repository::new(conn);
    let project = repo.get_project_by_root(root).unwrap().unwrap();

    let main_file = repo
        .get_file_by_path(project.id, "src/main.rs")
        .unwrap()
        .unwrap();
    let analysis = repo.get_file_analysis(main_file.id).unwrap();
    assert!(analysis.is_some());
    let analysis_rec = analysis.unwrap();
    assert_eq!(analysis_rec.content_hash, main_file.content_hash);
    assert_eq!(analysis_rec.status, "completed");
    assert_eq!(analysis_rec.analyzer_version.as_deref(), Some("0.1.0"));

    let ignored_file = repo
        .get_file_by_path(project.id, "src/ignored.rs")
        .unwrap()
        .unwrap();
    let ignored_analysis = repo.get_file_analysis(ignored_file.id).unwrap();
    assert!(
        ignored_analysis.is_none(),
        "Ignored file should not have file_analysis record"
    );
}

#[test]
fn test_deep_scan_incremental_skip_and_modification() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "fn a() {}\n").unwrap();
    fs::write(root.join("src/b.rs"), "fn b() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    let analyzer = BasicDeepAnalyzer::default();

    // Run 1: Both files analyzed
    let run1 = manager.deep_scan(root, &analyzer).unwrap();
    assert_eq!(run1.total_eligible, 2);
    assert_eq!(run1.analyzed, 2);
    assert_eq!(run1.skipped, 0);

    // Run 2 without any changes: Both files skipped as up to date!
    let run2 = manager.deep_scan(root, &analyzer).unwrap();
    assert_eq!(run2.total_eligible, 2);
    assert_eq!(run2.analyzed, 0);
    assert_eq!(run2.skipped, 2);

    // Now edit src/a.rs
    fs::write(root.join("src/a.rs"), "fn a_updated() { let x = 42; }\n").unwrap();

    // Fast scan to update metadata/hash
    let scan_res = manager.scan(root, "cli").unwrap();
    assert_eq!(scan_res.changed_files, 1);
    assert_eq!(scan_res.unchanged_files, 1);

    // Run 3: Only src/a.rs re-analyzed, src/b.rs skipped!
    let run3 = manager.deep_scan(root, &analyzer).unwrap();
    assert_eq!(run3.total_eligible, 2);
    assert_eq!(run3.analyzed, 1);
    assert_eq!(run3.skipped, 1);

    // Run 4: Everything up to date again
    let run4 = manager.deep_scan(root, &analyzer).unwrap();
    assert_eq!(run4.total_eligible, 2);
    assert_eq!(run4.analyzed, 0);
    assert_eq!(run4.skipped, 2);
}

#[test]
fn test_deep_scan_removed_files_not_analyzed() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    let analyzer = BasicDeepAnalyzer::default();
    let run1 = manager.deep_scan(root, &analyzer).unwrap();
    assert_eq!(run1.analyzed, 1);

    // Delete src/main.rs and scan
    fs::remove_file(root.join("src/main.rs")).unwrap();
    let scan2 = manager.scan(root, "cli").unwrap();
    assert_eq!(scan2.removed_files, 1);

    // Run deep-scan: removed file should not be eligible
    let run2 = manager.deep_scan(root, &analyzer).unwrap();
    assert_eq!(run2.total_eligible, 0);
    assert_eq!(run2.analyzed, 0);
    assert_eq!(run2.ineligible, 1); // ineligible because exists_on_disk = false
}

#[test]
fn test_status_reports_both_scan_and_deep_scan_details() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.scan(root, "cli").unwrap();

    let analyzer = BasicDeepAnalyzer::default();
    manager.deep_scan(root, &analyzer).unwrap();

    let status = manager.status(root).unwrap();
    assert!(status.last_run.is_some());
    let last_scan = status.last_run.unwrap();
    assert_eq!(last_scan.trigger, "cli");
    assert_eq!(last_scan.files_scanned, 1);

    assert!(status.last_deep_scan.is_some());
    let last_deep = status.last_deep_scan.unwrap();
    assert_eq!(last_deep.trigger, "deep_scan");
    assert_eq!(last_deep.files_new, 1);
}
