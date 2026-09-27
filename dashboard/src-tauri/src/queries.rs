//! The window's single entry point into the Inspectye core.

use crate::Dashboard;
use inspecteye::analysis::CodeAnalyzer;
use inspecteye::error::InspectyeError;
use inspecteye::project::{Direction, ProjectManager, TreeOptions};
use serde_json::{json, Value};

/// Runs one named query against the project and returns its result as
/// JSON, in the same shape as the CLI's `--json` `data`. Errors come back
/// as `{kind, message}` like the CLI's JSON errors.
///
/// Read queries use the index as stored. `refresh` and `check` analyze
/// first (scan + incremental deep scan), like the CLI.
#[tauri::command(async)]
pub fn query(
    state: tauri::State<'_, Dashboard>,
    view: String,
    target: Option<String>,
) -> Result<Value, Value> {
    let _guard = state.lock.lock().unwrap_or_else(|e| e.into_inner());
    run(&state.root, &view, target.as_deref()).map_err(|e| {
        json!({
            "kind": e.kind(),
            "message": e.to_string(),
        })
    })
}

fn run(root: &std::path::Path, view: &str, target: Option<&str>) -> Result<Value, InspectyeError> {
    let m = ProjectManager::default();
    let required = || {
        target.map(str::to_string).ok_or_else(|| {
            InspectyeError::InvalidArgument(format!("the '{view}' query needs a target"))
        })
    };
    let value = match view {
        "status" => to_json(m.status(root)?),
        "tree" => to_json(m.tree(root, None, TreeOptions::default())?),
        "insights" => to_json(m.insights(root, &[], 10, &[])?),
        "partial-classes" => to_json(m.partial_types(root)?),
        "diagnostics" => to_json(m.diagnostics_report(root, &[], None)?),
        "references" => to_json(m.references_report(root, &[])?),
        "impact" => to_json(m.impact_of_changes(root, 3)?),
        "file" => to_json(m.file_summary(root, &required()?)?),
        "dependencies" => to_json(m.dependency_report(root, &[required()?], Direction::Dependencies)?),
        "dependents" => to_json(m.dependency_report(root, &[required()?], Direction::Dependents)?),
        "file-impact" => to_json(m.impact(root, &[required()?], &[], 3)?),
        "history" => to_json(m.history(root, &required()?, None, 30)?),
        "symbol" => to_json(m.symbol_details(root, &required()?, &[])?),
        "refresh" => {
            let (scan, deep) = m.refresh(root, &CodeAnalyzer::new())?;
            json!({ "scan": scan, "deep_scan": deep })
        }
        "check" => {
            m.refresh(root, &CodeAnalyzer::new())?;
            to_json(m.check_project(root, false)?)
        }
        other => {
            return Err(InspectyeError::InvalidArgument(format!(
                "unknown dashboard query '{other}'"
            )))
        }
    };
    Ok(value)
}

fn to_json<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|e| json!({ "serialization_error": e.to_string() }))
}
