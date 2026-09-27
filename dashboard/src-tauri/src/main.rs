//! Inspectye dashboard: a desktop window over the Inspectye core.
//!
//! Started by `inspecteye dashboard` as `inspecteye-dashboard --root <project>`.
//! The window asks for data through one command, `query`, which calls the
//! same `ProjectManager` the CLI uses; nothing is computed in the web layer.

// No console window next to the dashboard in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod queries;

use std::path::PathBuf;
use std::sync::Mutex;

pub struct Dashboard {
    root: PathBuf,
    /// One query at a time: a refresh writes the index while others read it.
    lock: Mutex<()>,
}

fn project_root() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--root" {
            if let Some(root) = args.next() {
                return PathBuf::from(root);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn main() {
    let root = project_root();
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());

    tauri::Builder::default()
        .manage(Dashboard {
            root,
            lock: Mutex::new(()),
        })
        .invoke_handler(tauri::generate_handler![queries::query])
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                .title(format!("Inspectye — {name}"))
                .inner_size(1320.0, 860.0)
                .min_inner_size(900.0, 600.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Inspectye dashboard");
}
