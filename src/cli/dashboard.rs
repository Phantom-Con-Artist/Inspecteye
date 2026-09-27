//! `inspecteye dashboard`: opens the desktop dashboard window for a project.
//!
//! The dashboard is a separate executable (`inspecteye-dashboard`, a Tauri
//! app in `dashboard/`) that links this crate's core and reads the same
//! project intelligence in-process.

use crate::error::{InspectyeError, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const BINARY: &str = if cfg!(windows) {
    "inspecteye-dashboard.exe"
} else {
    "inspecteye-dashboard"
};

/// Starts the dashboard window and returns; the window runs on its own.
pub fn launch(project_root: &Path) -> Result<ExitCode> {
    let exe = find_dashboard().ok_or_else(|| {
        InspectyeError::NotFound(format!(
            "the dashboard is not built. In {}, run 'npm install' then 'npm run build' \
             (or set INSPECTEYE_DASHBOARD to the {BINARY} executable).",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("dashboard")
                .display()
        ))
    })?;
    Command::new(&exe)
        .arg("--root")
        .arg(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            InspectyeError::Io(std::io::Error::new(
                e.kind(),
                format!("could not start {}: {e}", exe.display()),
            ))
        })?;
    println!(
        "Opened the Inspectye dashboard for {}",
        project_root.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// `INSPECTEYE_DASHBOARD`, then next to this executable, then the build
/// output of the source tree this binary was built from (release first).
fn find_dashboard() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("INSPECTEYE_DASHBOARD").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let beside = dir.join(BINARY);
        if beside.is_file() {
            return Some(beside);
        }
    }
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("dashboard/src-tauri/target");
    ["release", "debug"]
        .iter()
        .map(|profile| target.join(profile).join(BINARY))
        .find(|p| p.is_file())
}
