//! Command-line interface, organized in command families:
//!
//! - fundamental: `init`, `scan`, `deep-scan`, `status`
//! - `inspect <action>`: read stored intelligence; never re-analyzes
//! - `check [action]`: analyze now (refreshing the analysis first) and
//!   report findings; exits non-zero when the check fails
//! - `insights [action]`: project-wide rankings from stored intelligence
//! - `tree [target]`: the project's structure
//! - `dashboard`: the local web interface
//!
//! Every command accepts `--json` (see `output.rs`).

mod dashboard;
mod output;
mod render;

use crate::analysis::CodeAnalyzer;
use crate::error::Result;
use crate::project::{normalize_path, Direction, ProjectManager, TreeOptions};
use clap::{Parser, Subcommand};
use output::Output;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "inspecteye")]
#[command(bin_name = "inspecteye")]
#[command(version, about = "See inside your codebase — local-first codebase intelligence and observability", long_about = None)]
pub struct Cli {
    /// Print machine-readable JSON instead of human-readable text
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    // ----------------------------------------------------------- fundamental
    /// Initialize Inspectye in the current or specified project directory
    Init {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Fast scan: discover resources and record what changed
    Scan {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Deep analysis (symbols, relationships, diagnostics, external
    /// references) of files changed since the last analysis
    DeepScan {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Show what Inspectye currently knows (reads stored state only)
    Status {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    // ---------------------------------------------------------------- inspect
    /// Read what Inspectye knows about a target (never re-analyzes; run
    /// `deep-scan` to update)
    Inspect {
        /// Project directory path (defaults to current directory)
        #[arg(short, long, default_value = ".", global = true)]
        project: PathBuf,
        #[command(subcommand)]
        action: InspectAction,
    },

    // ------------------------------------------------------------------ check
    /// Analyze now and report findings. Without an action, runs every check
    /// over the project (preflight). Exits with status 1 when the check fails.
    Check {
        #[command(subcommand)]
        action: Option<CheckAction>,
        /// Fail on warnings too, not only on errors
        #[arg(long, global = true)]
        strict: bool,
        /// Check the stored analysis without refreshing it first
        #[arg(long, global = true)]
        no_refresh: bool,
        /// Project directory path (defaults to current directory)
        #[arg(short, long, default_value = ".", global = true)]
        project: PathBuf,
    },

    // --------------------------------------------------------------- insights
    /// Project-wide patterns as explicit, ranked metrics (all when no action)
    Insights {
        #[command(subcommand)]
        action: Option<InsightsAction>,
        /// Rows per ranking
        #[arg(long, default_value_t = 10, global = true)]
        limit: usize,
        /// Project directory path (defaults to current directory)
        #[arg(short, long, default_value = ".", global = true)]
        project: PathBuf,
    },

    // ------------------------------------------------------------------- tree
    /// Show the project's structure (from the index)
    Tree {
        /// Directory or file to show (defaults to the project root)
        target: Option<String>,
        /// Levels of directories to expand
        #[arg(long)]
        depth: Option<usize>,
        /// List file paths only, without the hierarchy
        #[arg(long, conflicts_with = "directories")]
        files: bool,
        /// Show directories only
        #[arg(long)]
        directories: bool,
        /// Only Git-tracked files
        #[arg(long)]
        tracked: bool,
        /// Only ignored files (normally left out)
        #[arg(long)]
        ignored: bool,
        /// Project directory path (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    // -------------------------------------------------------------- dashboard
    /// Open the desktop dashboard window
    Dashboard {
        /// Project directory path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum InspectAction {
    /// Symbols (functions, classes, methods, ...) declared in files
    Symbols {
        /// Files or directories (defaults to the whole project)
        targets: Vec<String>,
    },
    /// One symbol: definition, members, what it uses, what uses it,
    /// diagnostics and recorded changes
    Symbol {
        /// Name, qualified name (`AuthService.login` or `auth::login`) or identity
        name: String,
        /// Only look in these files or directories
        #[arg(long = "in")]
        within: Vec<String>,
    },
    /// What the targets depend on (project files and packages)
    Dependencies {
        /// Files or directories
        #[arg(required = true)]
        targets: Vec<String>,
    },
    /// What depends on the targets
    Dependents {
        /// Files or directories
        #[arg(required = true)]
        targets: Vec<String>,
    },
    /// Files that may be affected if the targets change. Without targets:
    /// the working-tree changes not yet scanned
    Impact {
        /// Files or directories whose change to analyse
        targets: Vec<String>,
        /// Limit to these symbols (name or qualified name); repeatable
        #[arg(short, long = "symbol")]
        symbols: Vec<String>,
        /// How many dependency levels to follow
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    /// Git commits, recorded working-tree changes and symbol changes of a file.
    /// With --symbol, only the versions where that symbol's code changed
    History {
        /// File
        target: String,
        /// Symbol to follow (name, qualified name or identity)
        #[arg(short, long)]
        symbol: Option<String>,
        /// Maximum number of commits to read
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Stored diagnostics (syntax, logic) and external-reference findings
    Diagnostics {
        /// Files or directories (defaults to the whole project)
        targets: Vec<String>,
        /// Only one category: syntax, logic or external
        #[arg(long)]
        category: Option<String>,
    },
    /// Resources the targets reference: project files, packages, and
    /// absolute filesystem paths
    References {
        /// Files or directories (defaults to the whole project)
        targets: Vec<String>,
    },
    /// Everything known about one file in one read (for agents)
    File {
        /// File
        target: String,
    },
    /// Line-level diff of a file's last recorded change
    Changes {
        /// File
        target: String,
    },
    /// Compare a file (or one symbol in it) between two revisions
    /// (Git revisions or `working`; TO defaults to `working`)
    Compare {
        /// File
        target: String,
        /// Older revision (e.g. HEAD~3, a commit hash, a tag)
        from: String,
        /// Newer revision (defaults to the working tree)
        to: Option<String>,
        /// Compare only this symbol's code
        #[arg(short, long)]
        symbol: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CheckAction {
    /// Parser errors
    Syntax {
        /// Files or directories (defaults to the whole project)
        targets: Vec<String>,
    },
    /// Suspicious low-level logic: constant conditions, unreachable code,
    /// self-comparison, empty handlers, overwritten values, ...
    Logic {
        /// Files or directories (defaults to the whole project)
        targets: Vec<String>,
    },
    /// Broken project-relative imports, import cycles, and external
    /// filesystem references (re-checked now)
    Dependencies {
        /// Files or directories (defaults to the whole project)
        targets: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum InsightsAction {
    /// Most changed files (Git) and symbols (recorded changes)
    Changed { target: Option<String> },
    /// Where diagnostics concentrate
    Diagnostics { target: Option<String> },
    /// Files with the most dependents and dependencies
    Dependencies { target: Option<String> },
    /// Most referenced and largest symbols
    Symbols { target: Option<String> },
    /// Largest files
    Size { target: Option<String> },
    /// Most complex functions (cyclomatic)
    Complexity { target: Option<String> },
    /// C# partial classes: groups, parts, members, diagnostics
    PartialClasses { target: Option<String> },
}

impl InsightsAction {
    fn parts(&self) -> (&'static str, Option<&String>) {
        match self {
            InsightsAction::Changed { target } => ("changed", target.as_ref()),
            InsightsAction::Diagnostics { target } => ("diagnostics", target.as_ref()),
            InsightsAction::Dependencies { target } => ("dependencies", target.as_ref()),
            InsightsAction::Symbols { target } => ("symbols", target.as_ref()),
            InsightsAction::Size { target } => ("size", target.as_ref()),
            InsightsAction::Complexity { target } => ("complexity", target.as_ref()),
            InsightsAction::PartialClasses { target } => ("partial-classes", target.as_ref()),
        }
    }
}

/// Parses arguments, runs the command and reports errors. With `--json`,
/// errors (including argument errors) are printed as JSON on stdout.
pub fn run_cli() -> ExitCode {
    let json_requested = std::env::args().any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            use clap::error::ErrorKind;
            let informational = matches!(
                err.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            if json_requested && !informational {
                Output::new(true, "").error("invalid_argument", &err.to_string());
                return ExitCode::from(2);
            }
            err.exit();
        }
    };
    let out = Output::new(cli.json, command_name(&cli.command));
    match execute(cli.command, &out) {
        Ok(code) => code,
        Err(err) => {
            out.error(err.kind(), &err.to_string());
            ExitCode::FAILURE
        }
    }
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Init { .. } => "init",
        Commands::Scan { .. } => "scan",
        Commands::DeepScan { .. } => "deep-scan",
        Commands::Status { .. } => "status",
        Commands::Inspect { action, .. } => match action {
            InspectAction::Symbols { .. } => "inspect symbols",
            InspectAction::Symbol { .. } => "inspect symbol",
            InspectAction::Dependencies { .. } => "inspect dependencies",
            InspectAction::Dependents { .. } => "inspect dependents",
            InspectAction::Impact { .. } => "inspect impact",
            InspectAction::History { .. } => "inspect history",
            InspectAction::Diagnostics { .. } => "inspect diagnostics",
            InspectAction::References { .. } => "inspect references",
            InspectAction::File { .. } => "inspect file",
            InspectAction::Changes { .. } => "inspect changes",
            InspectAction::Compare { .. } => "inspect compare",
        },
        Commands::Check { action, .. } => match action {
            None => "check",
            Some(CheckAction::Syntax { .. }) => "check syntax",
            Some(CheckAction::Logic { .. }) => "check logic",
            Some(CheckAction::Dependencies { .. }) => "check dependencies",
        },
        Commands::Insights { action, .. } => match action.as_ref().map(|a| a.parts().0) {
            None => "insights",
            Some("changed") => "insights changed",
            Some("diagnostics") => "insights diagnostics",
            Some("dependencies") => "insights dependencies",
            Some("symbols") => "insights symbols",
            Some("size") => "insights size",
            Some("complexity") => "insights complexity",
            Some(_) => "insights partial-classes",
        },
        Commands::Tree { .. } => "tree",
        Commands::Dashboard { .. } => "dashboard",
    }
}

fn execute(command: Commands, out: &Output) -> Result<ExitCode> {
    let manager = ProjectManager::default();

    match command {
        Commands::Init { path } => {
            out.emit(&manager.init(&path)?, render::print_init);
        }
        Commands::Scan { path } => {
            out.note("Scanning project...");
            out.emit(&manager.scan(&path, "cli")?, render::print_scan);
        }
        Commands::DeepScan { path } => {
            out.note("Deep scanning project...");
            let summary = manager.deep_scan(&path, &CodeAnalyzer::new())?;
            out.emit(&summary, render::print_deep_scan);
        }
        Commands::Status { path } => {
            out.emit(&manager.status(&path)?, render::print_status);
        }
        Commands::Inspect { project, action } => inspect(&manager, &project, action, out)?,
        Commands::Check {
            action,
            strict,
            no_refresh,
            project,
        } => return check(&manager, &project, action, strict, no_refresh, out),
        Commands::Insights {
            action,
            limit,
            project,
        } => {
            let (category, target) = match &action {
                Some(a) => {
                    let (c, t) = a.parts();
                    (Some(c), t)
                }
                None => (None, None),
            };
            let targets =
                resolve_cli_paths(target.into_iter().cloned().collect::<Vec<_>>().as_slice())?;
            if category == Some("partial-classes") {
                let groups = manager.partial_types(&project)?;
                out.emit(&groups, |g| render::print_partial_types(g));
            } else {
                let categories: Vec<String> = category.into_iter().map(str::to_string).collect();
                let rankings = manager.insights(&project, &categories, limit, &targets)?;
                out.emit(&rankings, |r| render::print_rankings(r));
            }
        }
        Commands::Tree {
            target,
            depth,
            files,
            directories,
            tracked,
            ignored,
            project,
        } => {
            let target = target.map(|t| resolve_cli_path(&t)).transpose()?;
            let tree = manager.tree(
                &project,
                target.as_deref(),
                TreeOptions {
                    max_depth: depth,
                    tracked_only: tracked,
                    ignored_only: ignored,
                    directories_only: directories,
                },
            )?;
            out.emit(&tree, |t| render::print_tree(t, files));
        }
        Commands::Dashboard { path } => {
            let root = manager.find_project_root(&path)?;
            return dashboard::launch(&root);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn inspect(
    manager: &ProjectManager,
    project: &Path,
    action: InspectAction,
    out: &Output,
) -> Result<()> {
    match action {
        InspectAction::Symbols { targets } => {
            let symbols = manager.symbols(project, &resolve_cli_paths(&targets)?)?;
            out.emit(&symbols, |s| render::print_symbols(s));
        }
        InspectAction::Symbol { name, within } => {
            let details = manager.symbol_details(project, &name, &resolve_cli_paths(&within)?)?;
            out.emit(&details, |d| render::print_symbol_details(d));
        }
        InspectAction::Dependencies { targets } => {
            let report = manager.dependency_report(
                project,
                &resolve_cli_paths(&targets)?,
                Direction::Dependencies,
            )?;
            out.emit(&report, render::print_dependency_report);
        }
        InspectAction::Dependents { targets } => {
            let report = manager.dependency_report(
                project,
                &resolve_cli_paths(&targets)?,
                Direction::Dependents,
            )?;
            out.emit(&report, render::print_dependency_report);
        }
        InspectAction::Impact {
            targets,
            symbols,
            depth,
        } => {
            let report = if targets.is_empty() {
                manager.impact_of_changes(project, depth)?
            } else {
                manager.impact(project, &resolve_cli_paths(&targets)?, &symbols, depth)?
            };
            out.emit(&report, render::print_impact);
        }
        InspectAction::History {
            target,
            symbol,
            limit,
        } => {
            let report = manager.history(
                project,
                &resolve_cli_path(&target)?,
                symbol.as_deref(),
                limit,
            )?;
            out.emit(&report, render::print_history);
        }
        InspectAction::Diagnostics { targets, category } => {
            let report = manager.diagnostics_report(
                project,
                &resolve_cli_paths(&targets)?,
                category.as_deref(),
            )?;
            out.emit(&report, render::print_diagnostics_report);
        }
        InspectAction::References { targets } => {
            let report = manager.references_report(project, &resolve_cli_paths(&targets)?)?;
            out.emit(&report, render::print_references_report);
        }
        InspectAction::File { target } => {
            let summary = manager.file_summary(project, &resolve_cli_path(&target)?)?;
            out.emit(&summary, render::print_file_summary);
        }
        InspectAction::Changes { target } => {
            let transition = manager.changes(project, &resolve_cli_path(&target)?)?;
            out.emit(&transition, |t| print!("{}", t.format_terminal()));
        }
        InspectAction::Compare {
            target,
            from,
            to,
            symbol,
        } => {
            let cmp = manager.compare(
                project,
                &resolve_cli_path(&target)?,
                &from,
                to.as_deref(),
                symbol.as_deref(),
            )?;
            out.emit(&cmp, render::print_comparison);
        }
    }
    Ok(())
}

/// A check's result with the refresh that preceded it.
#[derive(Serialize)]
struct CheckOutput {
    analysis: Option<AnalysisSummary>,
    #[serde(flatten)]
    report: crate::project::CheckReport,
}

#[derive(Serialize)]
struct AnalysisSummary {
    analyzed: usize,
    skipped: usize,
    failed: usize,
}

fn check(
    manager: &ProjectManager,
    project: &Path,
    action: Option<CheckAction>,
    strict: bool,
    no_refresh: bool,
    out: &Output,
) -> Result<ExitCode> {
    let analysis = if no_refresh {
        None
    } else {
        let (_, deep) = manager.refresh(project, &CodeAnalyzer::new())?;
        out.note(&format!(
            "Analysis: {} file(s) re-analyzed, {} unchanged, {} failed.\n",
            deep.analyzed, deep.skipped, deep.failed
        ));
        Some(AnalysisSummary {
            analyzed: deep.analyzed,
            skipped: deep.skipped,
            failed: deep.failed,
        })
    };
    let report = match action {
        None => manager.check_project(project, strict)?,
        Some(CheckAction::Syntax { targets }) => {
            manager.check_syntax(project, &resolve_cli_paths(&targets)?, strict)?
        }
        Some(CheckAction::Logic { targets }) => {
            manager.check_logic(project, &resolve_cli_paths(&targets)?, strict)?
        }
        Some(CheckAction::Dependencies { targets }) => {
            manager.check_dependencies(project, &resolve_cli_paths(&targets)?, strict)?
        }
    };
    let passed = report.passed;
    out.emit(&CheckOutput { analysis, report }, |o| {
        render::print_check_report(&o.report)
    });
    Ok(if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn resolve_cli_paths(paths: &[String]) -> Result<Vec<String>> {
    paths.iter().map(|p| resolve_cli_path(p)).collect()
}

/// Resolves a user-supplied file path against the current directory, the way
/// Git does, so `inspect file auth.ts` works from inside `src/`. The file may
/// no longer exist, so only its parent directory is canonicalized.
fn resolve_cli_path(path: &str) -> Result<String> {
    let p = Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()?.join(p)
    };
    let resolved = match (abs.parent(), abs.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|dir| normalize_path(&dir).join(name))
            .unwrap_or(abs),
        _ => abs,
    };
    Ok(resolved.to_string_lossy().into_owned())
}
