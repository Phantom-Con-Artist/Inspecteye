use inspecteye::analysis::CodeAnalyzer;
use inspecteye::database::StoredRelationship;
use inspecteye::intelligence::RelationshipKind;
use inspecteye::issues::Certainty;
use inspecteye::project::ProjectManager;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn rels(manager: &ProjectManager, root: &Path) -> Vec<StoredRelationship> {
    manager.relationships(root, &[]).unwrap()
}

fn has(rels: &[StoredRelationship], kind: RelationshipKind, from: &str, to: &str) -> bool {
    rels.iter().any(|r| {
        r.relationship.kind == kind && r.source_path == from && r.target_path.as_deref() == Some(to)
    })
}

#[test]
fn test_typescript_imports_calls_and_dependents() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src/db")).unwrap();
    fs::write(
        root.join("src/db/conn.ts"),
        "export class Conn {}\nexport function open(): Conn { return new Conn(); }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/a.ts"),
        "import { open } from './db/conn';\nimport lodash from 'lodash';\nexport const x = open();\n",
    )
    .unwrap();
    fs::write(
        root.join("src/b.ts"),
        "import { open } from './db/conn';\nopen();\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    let r = rels(&manager, root);

    assert!(has(
        &r,
        RelationshipKind::Imports,
        "src/a.ts",
        "src/db/conn.ts"
    ));
    assert!(has(
        &r,
        RelationshipKind::Calls,
        "src/a.ts",
        "src/db/conn.ts"
    ));
    assert!(has(
        &r,
        RelationshipKind::DependsOn,
        "src/b.ts",
        "src/db/conn.ts"
    ));
    // Package imports are recorded as external, never linked to a file.
    assert!(r
        .iter()
        .any(|x| x.relationship.kind == RelationshipKind::Imports
            && x.relationship.target_name == "lodash"
            && x.target_path.is_none()));

    let metrics = manager.dependency_metrics(root).unwrap();
    let conn = metrics.iter().find(|(p, _)| p == "src/db/conn.ts").unwrap();
    assert_eq!(conn.1.dependents, 2);
    assert_eq!(conn.1.dependencies, 0);
}

#[test]
fn test_editing_one_file_reresolves_references_from_others() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("lib.py"), "def helper():\n    pass\n").unwrap();
    fs::write(root.join("main.py"), "from lib import helper\nhelper()\n").unwrap();

    let manager = ProjectManager::default();
    let analyzer = CodeAnalyzer::new();
    manager.init(root).unwrap();
    manager.refresh(root, &analyzer).unwrap();
    assert!(has(
        &rels(&manager, root),
        RelationshipKind::Calls,
        "main.py",
        "lib.py"
    ));

    // Only lib.py changes; main.py is not re-parsed, but its call must no
    // longer resolve to a declaration that is gone.
    fs::write(root.join("lib.py"), "def renamed():\n    pass\n").unwrap();
    let (_, deep) = manager.refresh(root, &analyzer).unwrap();
    assert_eq!(deep.analyzed, 1);
    let r = rels(&manager, root);
    assert!(!has(&r, RelationshipKind::Calls, "main.py", "lib.py"));
    assert!(has(&r, RelationshipKind::Imports, "main.py", "lib.py"));
}

#[test]
fn test_ambiguous_names_are_not_linked() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("a.rs"), "pub fn run() {}\n").unwrap();
    fs::write(root.join("b.rs"), "pub fn run() {}\n").unwrap();
    fs::write(root.join("c.rs"), "fn main() { run(); }\n").unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    let (_, deep) = manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    assert!(deep.resolution.ambiguous >= 1);
    assert!(!rels(&manager, root)
        .iter()
        .any(|r| r.source_path == "c.rs" && r.relationship.kind == RelationshipKind::Calls));
}

#[test]
fn test_csharp_interfaces_become_implements_and_usings_are_inferences() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("Core.cs"),
        "namespace Shop.Core;\npublic interface IRepo {}\npublic class Base {}\n",
    )
    .unwrap();
    fs::write(
        root.join("Repo.cs"),
        "using Shop.Core;\nnamespace Shop.Data;\npublic class Repo : Base, IRepo {}\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    let r = rels(&manager, root);

    assert!(has(&r, RelationshipKind::Inherits, "Repo.cs", "Core.cs"));
    assert!(has(&r, RelationshipKind::Implements, "Repo.cs", "Core.cs"));
    let using = r
        .iter()
        .find(|x| x.relationship.kind == RelationshipKind::Imports && x.source_path == "Repo.cs")
        .unwrap();
    assert_eq!(using.relationship.certainty, Certainty::Inference);
}

#[test]
fn test_rust_module_declarations_resolve_to_files() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src/db")).unwrap();
    fs::write(root.join("src/lib.rs"), "mod db;\nmod api;\n").unwrap();
    fs::write(root.join("src/db/mod.rs"), "pub mod repo;\n").unwrap();
    fs::write(root.join("src/db/repo.rs"), "pub fn open() {}\n").unwrap();
    fs::write(
        root.join("src/api.rs"),
        "use crate::db::repo::open;\nfn f() { open(); }\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();
    let r = rels(&manager, root);

    assert!(has(
        &r,
        RelationshipKind::Imports,
        "src/lib.rs",
        "src/db/mod.rs"
    ));
    assert!(has(
        &r,
        RelationshipKind::Imports,
        "src/lib.rs",
        "src/api.rs"
    ));
    assert!(has(
        &r,
        RelationshipKind::Imports,
        "src/db/mod.rs",
        "src/db/repo.rs"
    ));
    assert!(has(
        &r,
        RelationshipKind::Imports,
        "src/api.rs",
        "src/db/repo.rs"
    ));
    assert!(has(
        &r,
        RelationshipKind::Calls,
        "src/api.rs",
        "src/db/repo.rs"
    ));
}

#[test]
fn test_partial_class_parts_group_and_resolve_as_one_type() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("Order.cs"),
        "namespace Shop;\npublic partial class Order {\n  public void Place() {}\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Order.Rules.cs"),
        "namespace Shop;\npublic partial class Order {\n  void Check() {}\n  void Audit() {}\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Api.cs"),
        "using Shop;\nnamespace Web;\nclass Api { void Post() { var o = new Order(); } }\n",
    )
    .unwrap();

    let manager = ProjectManager::default();
    manager.init(root).unwrap();
    manager.refresh(root, &CodeAnalyzer::new()).unwrap();

    let groups = manager.partial_types(root).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].qualified_name, "Shop.Order");
    assert_eq!(groups[0].parts.len(), 2);
    assert_eq!(groups[0].members, 3);
    assert_eq!(groups[0].dependents, 1);

    // `new Order()` is not ambiguous just because Order has two parts.
    let r = rels(&manager, root);
    assert!(r
        .iter()
        .any(|x| x.relationship.kind == RelationshipKind::Calls
            && x.source_path == "Api.cs"
            && x.target_symbol.as_deref() == Some("Shop.Order")));
}
