//! The `--json` machine interface, exercised through the real binary.

use serde_json::Value;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn run(dir: &Path, args: &[&str]) -> (Value, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_inspecteye"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run inspecteye");
    let text = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let value: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}):\n{text}"));
    (value, out.status.code().unwrap_or(-1))
}

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/auth.ts"),
        "export function login(user: string) {\n  if (user == user) { return true; }\n  return false;\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/app.ts"),
        "import { login } from './auth';\nlogin('a');\n",
    )
    .unwrap();
    run(dir.path(), &["--json", "init"]);
    run(dir.path(), &["--json", "scan"]);
    run(dir.path(), &["--json", "deep-scan"]);
    dir
}

#[test]
fn test_every_output_is_a_single_envelope() {
    let dir = project();
    for args in [
        &["--json", "status"][..],
        &["--json", "scan"],
        &["--json", "deep-scan"],
        &["--json", "inspect", "symbols"],
        &["--json", "inspect", "symbol", "login"],
        &["--json", "inspect", "dependencies", "src/app.ts"],
        &["--json", "inspect", "dependents", "src/auth.ts"],
        &["--json", "inspect", "impact", "src/auth.ts"],
        &["--json", "inspect", "impact"],
        &["--json", "inspect", "diagnostics"],
        &["--json", "inspect", "references"],
        &["--json", "inspect", "history", "src/auth.ts"],
        &["--json", "check", "syntax"],
        &["--json", "check", "logic"],
        &["--json", "check", "dependencies"],
        &["--json", "check"],
        &["--json", "insights"],
        &["--json", "insights", "dependencies", "src"],
        &["--json", "insights", "partial-classes"],
        &["--json", "tree"],
        &["--json", "tree", "src", "--depth", "1"],
        &["status", "--json"],
    ] {
        let (v, code) = run(dir.path(), args);
        assert_eq!(code, 0, "{args:?}: {v}");
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["ok"], true);
        assert!(v.get("data").is_some(), "{args:?}");
    }
}

#[test]
fn test_check_fails_on_errors_and_strict_fails_on_warnings() {
    let dir = project();
    // A logic warning only: passes, unless strict.
    let (v, code) = run(dir.path(), &["--json", "check"]);
    assert_eq!((code, &v["data"]["passed"]), (0, &Value::Bool(true)), "{v}");
    let (v, code) = run(dir.path(), &["--json", "check", "--strict"]);
    assert_eq!(
        (code, &v["data"]["passed"]),
        (1, &Value::Bool(false)),
        "{v}"
    );

    // A broken relative import is an error.
    std::fs::write(
        dir.path().join("src/app.ts"),
        "import { login } from './auth';
import { gone } from './gone';
login('a');
",
    )
    .unwrap();
    let (v, code) = run(dir.path(), &["--json", "check", "dependencies"]);
    assert_eq!(code, 1);
    let f = &v["data"]["findings"][0];
    assert_eq!(f["rule"], "dependency.broken_import");
    assert_eq!(f["severity"], "ERROR");
    assert_eq!(f["line"], 2);
    assert_eq!(v["data"]["analysis"]["analyzed"], 1);
}

#[test]
fn test_tree_and_symbol_shapes() {
    let dir = project();
    let (v, _) = run(dir.path(), &["--json", "tree"]);
    let src = v["data"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "src")
        .unwrap();
    assert_eq!(src["file_count"], 2);
    assert_eq!(src["children"][0]["file"]["kind"], "source");

    let (v, _) = run(dir.path(), &["--json", "inspect", "symbol", "login"]);
    let d = &v["data"][0];
    assert_eq!(d["symbol"]["path"], "src/auth.ts");
    assert_eq!(d["dependent_files"][0], "src/app.ts");
    assert_eq!(
        d["diagnostics"][0]["diagnostic"]["rule"],
        "logic.self_comparison"
    );

    let (v, code) = run(
        dir.path(),
        &["--json", "inspect", "symbol", "nothing_like_this"],
    );
    assert_eq!((code, &v["error"]["kind"]), (1, &Value::from("not_found")));
}

#[test]
fn test_file_summary_links_both_directions() {
    let dir = project();
    let (v, _) = run(dir.path(), &["--json", "inspect", "file", "src/auth.ts"]);
    let data = &v["data"];
    assert_eq!(data["file"]["path"], "src/auth.ts");
    assert_eq!(data["analysis_current"], true);
    assert_eq!(data["symbols"][0]["symbol"]["qualified_name"], "login");
    let dependents = data["dependents"].as_array().unwrap();
    assert!(dependents.iter().all(|r| r["source_path"] == "src/app.ts"));
    assert!(!dependents.is_empty());
    assert_eq!(
        data["diagnostics"][0]["diagnostic"]["rule"],
        "logic.self_comparison"
    );
}

#[test]
fn test_logic_findings_carry_severity_and_certainty() {
    let dir = project();
    let (v, _) = run(dir.path(), &["--json", "check", "logic"]);
    let f = &v["data"]["findings"][0];
    assert_eq!(f["path"], "src/auth.ts");
    assert_eq!(f["category"], "logic");
    assert_eq!(f["severity"], "WARNING");
    assert!(f["certainty"].is_string());
    assert_eq!(f["related_files"][0], "src/app.ts");
}

#[test]
fn test_errors_are_json_with_a_kind() {
    let dir = TempDir::new().unwrap();
    let (v, code) = run(dir.path(), &["--json", "status"]);
    assert_ne!(code, 0);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["kind"], "not_initialized");

    let (v, code) = run(dir.path(), &["--json", "no-such-command"]);
    assert_eq!(code, 2);
    assert_eq!(v["error"]["kind"], "invalid_argument");
}
