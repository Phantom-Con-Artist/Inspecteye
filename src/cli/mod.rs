use crate::analysis::BasicDeepAnalyzer;
use crate::error::Result;
use crate::project::ProjectManager;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "inspecteye")]
#[command(bin_name = "inspecteye")]
#[command(version, about = "See inside your codebase — local-first codebase intelligence and observability", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize Inspectye in the current or specified project directory
    Init {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Scan and index project files
    Scan {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Perform deep semantic analysis on eligible project resources
    DeepScan {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Display current project status and index information
    Status {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Display line-level changes and state transitions for a specific file
    Changes {
        /// Path to the specific project file
        path: String,
        /// Project directory path (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let manager = ProjectManager::default();

    match cli.command {
        Commands::Init { path } => {
            let res = manager.init(&path)?;
            if res.already_initialized {
                println!("Inspectye project is already initialized.");
            } else {
                println!("Initialized empty Inspectye project.");
            }
            println!("Project:  {}", res.project_name);
            println!("Root:     {}", res.root_path.display());
            println!("Database: {}", res.db_path.display());
        }
        Commands::Scan { path } => {
            println!("Scanning project...");
            let summary = manager.scan(&path, "cli")?;
            println!();
            println!("Project:          {}", summary.project_name);
            println!("Root:             {}", summary.project_root.display());
            println!();
            println!("Resources discovered:    {}", summary.files_discovered);
            println!("  Source:                {}", summary.resources.source);
            println!("  Config:                {}", summary.resources.config);
            println!(
                "  Documentation:         {}",
                summary.resources.documentation
            );
            println!("  Asset:                 {}", summary.resources.asset);
            if summary.resources.dependency > 0 {
                println!("  Dependency:            {}", summary.resources.dependency);
            }
            if summary.resources.generated > 0 {
                println!("  Generated:             {}", summary.resources.generated);
            }
            if summary.resources.binary > 0 {
                println!("  Binary:                {}", summary.resources.binary);
            }
            if summary.resources.unknown > 0 {
                println!("  Unknown:               {}", summary.resources.unknown);
            }
            println!();
            println!("Lifecycle changes:");
            println!("  New:                   {}", summary.new_files);
            println!("  Changed:               {}", summary.changed_files);
            println!("  Unchanged:             {}", summary.unchanged_files);
            println!("  Restored:              {}", summary.restored_files);
            println!(
                "  Restored and changed:  {}",
                summary.restored_changed_files
            );
            println!("  Removed:               {}", summary.removed_files);
            if summary.failed_files > 0 {
                println!("  Failed:                {}", summary.failed_files);
            }
            println!();
            println!("Scan complete in {:.2?}.", summary.duration);
        }
        Commands::DeepScan { path } => {
            println!("Deep scanning project...");
            let analyzer = BasicDeepAnalyzer::new();
            let summary = manager.deep_scan(&path, &analyzer)?;
            println!();
            println!("Project:          {}", summary.project_name);
            println!("Root:             {}", summary.project_root.display());
            println!();
            println!("Deep analysis:");
            println!("  Eligible:              {}", summary.total_eligible);
            println!("  Analyzed:              {}", summary.analyzed);
            println!("  Skipped:               {} (up to date)", summary.skipped);
            println!(
                "  Ineligible:            {} (ignored or non-source)",
                summary.ineligible
            );
            if summary.failed > 0 {
                println!("  Failed:                {}", summary.failed);
            }
            println!();
            println!("Deep scan complete in {:.2?}.", summary.duration);
        }
        Commands::Status { path } => {
            let report = manager.status(&path)?;
            println!("Project:  {}", report.project_name);
            println!("Root:     {}", report.root_path.display());
            println!("Database: {}", report.db_path.display());
            println!(
                "Git:      {}",
                if report.git_active {
                    "Active repository"
                } else {
                    "Not a git repository"
                }
            );
            println!();
            println!("Files (current project state):");
            println!("  Total known:  {}", report.stats.total_known_files);
            println!("  Discovered:   {}", report.stats.discovered_files);
            println!("    Git-tracked: {}", report.stats.git_tracked_files);
            println!("    Untracked:   {}", report.stats.untracked_files);
            println!("  Removed:      {}", report.stats.removed_files);
            println!();
            if let Some(run) = report.last_run {
                println!("Last scan:");
                println!("  Status:                {}", run.status);
                println!("  Trigger:               {}", run.trigger);
                println!("  Started:               {}", run.started_at);
                if let Some(finished) = run.finished_at {
                    println!("  Finished:              {}", finished);
                }
                println!("  Discovered:            {}", run.files_scanned);
                println!("    New:                 {}", run.files_new);
                println!("    Changed:             {}", run.files_changed);
                println!("    Unchanged:           {}", run.files_unchanged);
                println!("    Restored:            {}", run.files_restored);
                println!("    Restored and changed: {}", run.files_restored_changed);
                println!("  Removed:               {}", run.files_removed);
                if run.files_failed > 0 {
                    println!("  Failed:                {}", run.files_failed);
                }
            } else {
                println!("Last scan: None (Run 'inspecteye scan' to index the project)");
            }

            if let Some(deep) = report.last_deep_scan {
                println!();
                println!("Last deep scan:");
                println!("  Status:                {}", deep.status);
                println!("  Started:               {}", deep.started_at);
                if let Some(finished) = deep.finished_at {
                    println!("  Finished:              {}", finished);
                }
                println!("  Eligible:              {}", deep.files_scanned);
                println!("  Analyzed:              {}", deep.files_new);
                println!(
                    "  Skipped:               {} (up to date)",
                    deep.files_unchanged
                );
                if deep.files_failed > 0 {
                    println!("  Failed:                {}", deep.files_failed);
                }
            }
        }
        Commands::Changes { path, project } => {
            let transition = manager.changes(&project, &path)?;
            print!("{}", transition.format_terminal());
        }
    }

    Ok(())
}
