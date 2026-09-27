//! Human-readable terminal rendering of core results.

use crate::analysis::ChangeTag;
use crate::database::{StoredDiagnostic, StoredExternalReference, StoredSymbol};
use crate::intelligence::{Existence, PartialTypeGroup, Ranking, RelationshipKind, TreeNode};
use crate::issues::Severity;
use crate::project::{
    CheckReport, DeepScanSummary, DependencyReport, DiagnosticsReport, Direction, FileComparison,
    FileSummary, HistoryReport, ImpactReport, InitResult, ReferencesReport, ScanSummary,
    StatusReport, SymbolDetail,
};
use std::collections::HashMap;

pub fn print_diagnostics(title: &str, diagnostics: &[StoredDiagnostic]) {
    print_diagnostics_with_dependents(title, diagnostics, &HashMap::new());
}

/// `dependents` maps (path, symbol) to files that reference that symbol.
pub fn print_diagnostics_with_dependents(
    title: &str,
    diagnostics: &[StoredDiagnostic],
    dependents: &HashMap<(String, String), Vec<String>>,
) {
    if diagnostics.is_empty() {
        println!("{title}: none found.");
        return;
    }

    let mut current_path: Option<&str> = None;
    for stored in diagnostics {
        if current_path != Some(stored.path.as_str()) {
            if current_path.is_some() {
                println!();
            }
            println!("{}", stored.path);
            current_path = Some(&stored.path);
        }
        let d = &stored.diagnostic;
        println!(
            "  {:<7} {}:{}  {}  [{}, {}]",
            d.severity.as_str(),
            d.range.start_line,
            d.range.start_column,
            d.message,
            d.rule,
            d.certainty.as_str()
        );
        if let Some(symbol) = &d.symbol {
            println!("          in {symbol}");
            if let Some(users) = dependents.get(&(stored.path.clone(), symbol.clone())) {
                println!(
                    "          potentially affected: {} referenced from {}",
                    symbol,
                    users.join(", ")
                );
            }
        }
        for line in d.excerpt.lines() {
            println!("    {line}");
        }
        if !d.expected.is_empty() {
            println!("          expected: {}", d.expected.join(", "));
        }
        if let Some(evidence) = &d.evidence {
            println!("          evidence: {evidence}");
        }
    }

    let count = |s: Severity| {
        diagnostics
            .iter()
            .filter(|d| d.diagnostic.severity == s)
            .count()
    };
    let mut files: Vec<&str> = diagnostics.iter().map(|d| d.path.as_str()).collect();
    files.dedup();
    println!();
    println!(
        "{title}: {} error(s), {} warning(s), {} info in {} file(s).",
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
        files.len()
    );
}

pub fn print_symbols(symbols: &[StoredSymbol]) {
    if symbols.is_empty() {
        println!("No symbols found.");
        return;
    }
    let depth_of: HashMap<i64, usize> = {
        let mut depths = HashMap::new();
        for s in symbols {
            let d = s
                .parent_id
                .and_then(|p| depths.get(&p))
                .map_or(0, |d| d + 1);
            depths.insert(s.id, d);
        }
        depths
    };
    let mut current_path: Option<&str> = None;
    for s in symbols {
        if current_path != Some(s.path.as_str()) {
            if current_path.is_some() {
                println!();
            }
            println!("{}", s.path);
            current_path = Some(&s.path);
        }
        let depth = depth_of.get(&s.id).copied().unwrap_or(0);
        let sym = &s.symbol;
        let mods = if sym.modifiers.is_empty() {
            String::new()
        } else {
            format!("  [{}]", sym.modifiers.join(" "))
        };
        println!(
            "  {:>5}-{:<5} {}{:<12} {}{}",
            sym.range.start_line,
            sym.range.end_line,
            "  ".repeat(depth),
            sym.kind.as_str(),
            sym.name,
            mods
        );
    }
    let mut files: Vec<&str> = symbols.iter().map(|s| s.path.as_str()).collect();
    files.dedup();
    println!();
    println!("{} symbol(s) in {} file(s).", symbols.len(), files.len());
}

pub fn print_impact(report: &ImpactReport) {
    if report.origins.is_empty() {
        println!("No changes found: the working tree matches the last scan, and the last scan recorded no modifications or removals.");
        return;
    }
    println!("Changes analysed:");
    for o in &report.origins {
        let scope = if o.whole_file {
            "whole file".to_string()
        } else if o.modified_symbols.is_empty() && o.removed_symbols.is_empty() {
            "no existing declarations changed".to_string()
        } else {
            let mut parts = Vec::new();
            if !o.modified_symbols.is_empty() {
                parts.push(format!("modified: {}", o.modified_symbols.join(", ")));
            }
            if !o.removed_symbols.is_empty() {
                parts.push(format!("removed: {}", o.removed_symbols.join(", ")));
            }
            parts.join("; ")
        };
        println!("  {}  [{}, {}]  {}", o.path, o.change, o.source, scope);
    }
    println!();

    if report.affected.is_empty() {
        println!("No dependents found in the recorded relationships.");
        return;
    }
    let path = |id: i64| {
        report
            .file_paths
            .get(&id)
            .map(String::as_str)
            .unwrap_or("?")
    };
    let mut depth = 0;
    for a in &report.affected {
        if a.depth != depth {
            depth = a.depth;
            if depth == 1 {
                println!("Direct dependents:");
            } else {
                println!();
                println!("Depth {depth} (depends on a file above):");
            }
        }
        let flag = if a.references_removed_symbol {
            "  -- references a removed declaration"
        } else {
            ""
        };
        println!("  {}  [{}]{}", path(a.file_id), a.certainty.as_str(), flag);
        for step in a.via.iter().take(5) {
            let target = step
                .target_symbol_id
                .and_then(|id| report.symbol_names.get(&id))
                .map(|s| format!("`{s}` in {}", path(step.dependency_file_id)))
                .unwrap_or_else(|| path(step.dependency_file_id).to_string());
            println!(
                "      line {:<4} {} {}",
                step.line,
                step.kind.as_str(),
                target
            );
        }
        if a.via.len() > 5 {
            println!("      ... {} more", a.via.len() - 5);
        }
    }
    println!();
    println!(
        "{} file(s) may be affected (followed up to {} level(s)). Dependents are exposed to the change; this does not mean they are broken.",
        report.affected.len(),
        report.max_depth
    );
}

pub fn print_partial_types(groups: &[PartialTypeGroup]) {
    if groups.is_empty() {
        println!("No partial types found.");
        return;
    }
    for g in groups {
        println!(
            "{} {}  ({} part(s), {} lines, {} members)",
            g.kind.as_str(),
            g.qualified_name,
            g.parts.len(),
            g.lines,
            g.members
        );
        for p in &g.parts {
            println!(
                "    {}:{}-{}  {} member(s)",
                p.path, p.start_line, p.end_line, p.members
            );
        }
        println!(
            "    findings: {} error(s), {} warning(s); recorded symbol changes: {}; depends on {} file(s); {} file(s) depend on it",
            g.errors, g.warnings, g.changes, g.dependencies, g.dependents
        );
        println!();
    }
    println!("{} partial type(s).", groups.len());
}

pub fn print_history(report: &HistoryReport) {
    match &report.symbol {
        Some(sym) => println!("History of `{sym}` in {}", report.path),
        None => println!("History of {}", report.path),
    }
    println!();
    if !report.git_available {
        println!("Git: not a repository; only Inspectye's recorded states are available.");
    } else if report.entries.is_empty() {
        match &report.symbol {
            Some(_) => println!("No commit changed this symbol (within the commits read)."),
            None => println!("No commits touch this file."),
        }
    }
    for e in &report.entries {
        let when = match &e.commit {
            Some(c) => format!(
                "{}  {}  {}  {}",
                c.short_hash(),
                &c.date[..c.date.len().min(10)],
                c.author,
                c.subject
            ),
            None => "working tree (uncommitted)".to_string(),
        };
        match &e.symbol_change {
            Some(rev) => {
                let at = rev
                    .symbol
                    .as_ref()
                    .map(|s| format!(" (lines {}-{})", s.range.start_line, s.range.end_line))
                    .unwrap_or_default();
                println!("  {:<9}{}  {}", rev.change.as_str(), at, when);
            }
            None => {
                let renamed = e
                    .commit
                    .as_ref()
                    .filter(|c| c.path != report.path)
                    .map(|c| format!("  [as {}]", c.path))
                    .unwrap_or_default();
                println!("  {when}{renamed}");
            }
        }
    }
    if report.symbol.is_none() && !report.recorded.is_empty() {
        println!();
        println!("Recorded working-tree states (Inspectye scans):");
        for r in &report.recorded {
            println!(
                "  {}  {:<17} +{} -{}",
                &r.recorded_at[..r.recorded_at.len().min(19)],
                r.transition,
                r.added_lines,
                r.removed_lines
            );
        }
    }
    if !report.symbol_changes.is_empty() && report.symbol.is_none() {
        println!();
        println!("Symbol changes recorded by Inspectye (oldest first):");
        for c in &report.symbol_changes {
            println!(
                "  {}  {:<9} {} {}",
                c.created_at, c.change, c.kind, c.qualified_name
            );
        }
    }
}

pub fn print_comparison(cmp: &FileComparison) {
    match &cmp.symbol {
        Some(s) => println!("`{s}` in {}", cmp.path),
        None => println!("{}", cmp.path),
    }
    println!("  from: {}", cmp.from_label);
    println!("  to:   {}", cmp.to_label);
    println!();
    if cmp.hunks.is_empty() {
        println!("No differences.");
        return;
    }
    if !cmp.symbol_changes.is_empty() {
        println!("Symbols changed:");
        for c in &cmp.symbol_changes {
            println!(
                "  {:<9} {:<12} {}",
                c.change.as_str(),
                c.kind.as_str(),
                c.qualified_name
            );
        }
        println!();
    }
    for hunk in &cmp.hunks {
        println!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start + cmp.from_line_offset,
            hunk.old_lines,
            hunk.new_start + cmp.to_line_offset,
            hunk.new_lines
        );
        for line in &hunk.lines {
            let (mark, no) = match line.tag {
                ChangeTag::Equal => (' ', line.new_line_no.map(|n| n + cmp.to_line_offset)),
                ChangeTag::Delete => ('-', line.old_line_no.map(|n| n + cmp.from_line_offset)),
                ChangeTag::Insert => ('+', line.new_line_no.map(|n| n + cmp.to_line_offset)),
            };
            println!("{mark} {:4}  {}", no.unwrap_or(0), line.content);
        }
    }
    println!();
    println!("+{} -{} lines", cmp.added_lines, cmp.removed_lines);
}

pub fn print_rankings(rankings: &[Ranking]) {
    for r in rankings {
        println!("{}", r.title);
        println!("  metric: {}", r.metric);
        if r.rows.is_empty() {
            println!("  (no data)");
        }
        for (i, row) in r.rows.iter().enumerate() {
            let detail = if row.detail.is_empty() {
                String::new()
            } else {
                format!("   [{}]", row.detail)
            };
            println!("  {:>2}. {:>6}  {}{}", i + 1, row.value, row.label, detail);
        }
        println!();
    }
}

pub fn print_init(res: &InitResult) {
    if res.already_initialized {
        println!("Inspectye project is already initialized.");
    } else {
        println!("Initialized empty Inspectye project.");
    }
    println!("Project:  {}", res.project_name);
    println!("Root:     {}", res.root_path.display());
    println!("Database: {}", res.db_path.display());
}

pub fn print_scan(summary: &ScanSummary) {
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

pub fn print_deep_scan(summary: &DeepScanSummary) {
    println!();
    println!("Project:          {}", summary.project_name);
    println!("Root:             {}", summary.project_root.display());
    println!();
    println!("Deep analysis:");
    println!("  Eligible:              {}", summary.total_eligible);
    println!("  Analyzed:              {}", summary.analyzed);
    println!("  Skipped:               {} (up to date)", summary.skipped);
    println!(
        "  Ineligible:            {} (ignored, removed, docs, assets, dependencies, generated, binary)",
        summary.ineligible
    );
    if summary.failed > 0 {
        println!("  Failed:                {}", summary.failed);
    }
    println!();
    println!("References:");
    println!("  Resolved:              {}", summary.resolution.resolved);
    println!(
        "  Ambiguous:             {} (not linked)",
        summary.resolution.ambiguous
    );
    println!("  External/unresolved:   {}", summary.resolution.unresolved);
    println!();
    println!("External filesystem references:");
    println!(
        "  Found:                 {}",
        summary.external_references.total
    );
    println!(
        "  Errors:                {}",
        summary.external_references.errors
    );
    println!(
        "  Warnings:              {}",
        summary.external_references.warnings
    );
    if summary.external_references.info > 0 {
        println!(
            "  Info:                  {} (in test code)",
            summary.external_references.info
        );
    }
    println!();
    println!("Deep scan complete in {:.2?}.", summary.duration);
}

pub fn print_status(report: &StatusReport) {
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
    if let Some(run) = &report.last_run {
        println!("Last scan:");
        println!("  Status:                {}", run.status);
        println!("  Trigger:               {}", run.trigger);
        println!("  Started:               {}", run.started_at);
        if let Some(finished) = &run.finished_at {
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

    if let Some(deep) = &report.last_deep_scan {
        println!();
        println!("Last deep scan:");
        println!("  Status:                {}", deep.status);
        println!("  Started:               {}", deep.started_at);
        if let Some(finished) = &deep.finished_at {
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

pub fn print_external_references(refs: &[StoredExternalReference]) {
    if refs.is_empty() {
        println!("No absolute filesystem paths found in string literals or configuration.");
        return;
    }
    let (mut errors, mut warnings) = (0, 0);
    for r in refs {
        let loc = format!(
            "{}:{}:{}",
            r.file_path, r.reference.range.start_line, r.reference.range.start_column
        );
        match &r.check {
            None => println!("{loc}  (not checked yet)  {}", r.reference.path),
            Some(c) => {
                match c.severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                    Severity::Info => {}
                }
                println!(
                    "{loc}  {} [{}]  {}",
                    c.severity,
                    c.certainty.as_str(),
                    c.message
                );
                let mut notes = Vec::new();
                if let Some(s) = &r.reference.symbol {
                    notes.push(format!("in `{s}`"));
                }
                notes.push(match &c.existence {
                    Existence::Exists { is_directory: true } => "exists (directory)".to_string(),
                    Existence::Exists { .. } => "exists".to_string(),
                    Existence::Missing { .. } => "missing".to_string(),
                    Existence::Unchecked { reason } => format!("not checked: {reason}"),
                });
                notes.push(format!(
                    "inside repository: {}",
                    if c.inside_repository { "yes" } else { "no" }
                ));
                if let Some(m) = &c.machine_specific {
                    notes.push(format!("machine-specific: {m}"));
                }
                println!("      {}", notes.join(" · "));
            }
        }
    }
    println!();
    println!(
        "{} reference(s): {} error(s), {} warning(s).",
        refs.len(),
        errors,
        warnings
    );
}

pub fn print_file_summary(f: &FileSummary) {
    let file = &f.file;
    println!("File:      {}", file.path);
    println!(
        "Kind:      {}{}",
        file.kind,
        file.language
            .as_deref()
            .map(|l| format!(" ({l})"))
            .unwrap_or_default()
    );
    if let Some(lines) = file.line_count {
        println!("Lines:     {lines}");
    }
    println!("Size:      {} bytes", file.size_bytes);
    println!(
        "State:     {}{}",
        if file.exists_on_disk {
            "on disk"
        } else {
            "removed"
        },
        if file.git_tracked {
            ", Git-tracked"
        } else {
            ", untracked"
        }
    );
    match &f.analysis {
        None => println!("Analysis:  never analyzed (run 'inspecteye deep-scan')"),
        Some(a) => println!(
            "Analysis:  {} at {}{}",
            a.status,
            a.analyzed_at,
            if f.analysis_current {
                ""
            } else {
                "  -- file changed since; results describe an older version"
            }
        ),
    }
    println!();
    println!("Symbols ({}):", f.symbols.len());
    for s in f.symbols.iter().filter(|s| s.parent_id.is_none()) {
        println!(
            "  {:<10} {}  (lines {}-{})",
            s.symbol.kind.as_str(),
            s.symbol.qualified_name,
            s.symbol.range.start_line,
            s.symbol.range.end_line
        );
    }
    println!();
    println!("Diagnostics ({}):", f.diagnostics.len());
    for d in &f.diagnostics {
        let d = &d.diagnostic;
        println!(
            "  {}:{}  {} [{}] {}",
            d.range.start_line,
            d.range.start_column,
            d.severity,
            d.certainty.as_str(),
            d.message
        );
    }
    println!();
    let mut deps: Vec<&str> = f
        .dependencies
        .iter()
        .filter_map(|r| r.target_path.as_deref())
        .filter(|p| *p != file.path)
        .collect();
    deps.sort_unstable();
    deps.dedup();
    println!("Depends on ({} files):", deps.len());
    for p in deps {
        println!("  {p}");
    }
    let mut users: Vec<&str> = f
        .dependents
        .iter()
        .map(|r| r.source_path.as_str())
        .collect();
    users.sort_unstable();
    users.dedup();
    println!("Used by ({} files):", users.len());
    for p in users {
        println!("  {p}");
    }
    if !f.external_references.is_empty() {
        println!();
        println!("External references ({}):", f.external_references.len());
        for r in &f.external_references {
            let status = r
                .check
                .as_ref()
                .map(|c| format!("{} [{}]", c.severity, c.rule))
                .unwrap_or_else(|| "not checked".to_string());
            println!(
                "  {}:{}  {}  {}",
                r.reference.range.start_line,
                r.reference.range.start_column,
                r.reference.path,
                status
            );
        }
    }
    if !f.recent_changes.is_empty() {
        println!();
        println!("Recent recorded changes:");
        for c in &f.recent_changes {
            println!(
                "  {}  {:<18} +{} -{}",
                c.recorded_at, c.transition, c.added_lines, c.removed_lines
            );
        }
    }
}

pub fn print_check_report(report: &CheckReport) {
    // Project-wide INFO findings (e.g. fixture paths in tests) would drown
    // the problems; they stay available through --json and `inspect`.
    let show_info = report.scope != "project";
    let mut hidden_info = 0;
    let mut current: Option<&str> = None;
    for f in &report.findings {
        if f.severity == Severity::Info && !show_info {
            hidden_info += 1;
            continue;
        }
        if current != Some(f.path.as_str()) {
            if current.is_some() {
                println!();
            }
            println!("{}", f.path);
            current = Some(&f.path);
        }
        let at = match (f.line, f.column) {
            (Some(l), Some(c)) => format!("{l}:{c}"),
            (Some(l), None) => l.to_string(),
            _ => "-".to_string(),
        };
        println!(
            "  {:<7} {}  {}  [{}, {}]",
            f.severity.as_str(),
            at,
            f.message,
            f.rule,
            f.certainty.as_str()
        );
        if let Some(symbol) = &f.symbol {
            println!("          in {symbol}");
        }
        if let Some(excerpt) = &f.excerpt {
            for line in excerpt.lines() {
                println!("    {line}");
            }
        }
        if let Some(evidence) = &f.evidence {
            println!("          evidence: {evidence}");
        }
        if !f.related_files.is_empty() {
            let label = if f.category == "logic" {
                "potentially affected (use the enclosing symbol)"
            } else {
                "also involved"
            };
            println!("          {label}: {}", f.related_files.join(", "));
        }
    }
    if current.is_some() {
        println!();
    }
    let c = &report.counts;
    println!(
        "Check ({}): {} error(s), {} warning(s), {} info.{}",
        report.scope,
        c.errors,
        c.warnings,
        c.info,
        if hidden_info > 0 {
            format!(
                " ({hidden_info} info not listed; see --json or 'inspecteye inspect diagnostics'.)"
            )
        } else {
            String::new()
        }
    );
    println!(
        "Result: {}{}",
        if report.passed { "PASSED" } else { "FAILED" },
        if report.strict {
            " (strict: warnings fail)"
        } else {
            ""
        }
    );
}

pub fn print_symbol_details(details: &[SymbolDetail]) {
    for (i, d) in details.iter().enumerate() {
        if i > 0 {
            println!();
            println!("{}", "-".repeat(60));
        }
        let s = &d.symbol.symbol;
        println!("{} {}", s.kind.as_str(), s.qualified_name);
        println!(
            "  Defined:    {}:{}-{}",
            d.symbol.path, s.range.start_line, s.range.end_line
        );
        if !s.signature.is_empty() {
            println!("  Signature:  {}", s.signature);
        }
        if let Some(p) = &d.parent {
            println!("  Inside:     {p}");
        }
        if let Some(c) = s.complexity {
            println!("  Complexity: {c} (cyclomatic)");
        }
        if !d.children.is_empty() {
            println!("  Members ({}):", d.children.len());
            for c in &d.children {
                println!("    {c}");
            }
        }
        let describe = |r: &crate::database::StoredRelationship, incoming: bool| {
            let rel = &r.relationship;
            if incoming {
                format!(
                    "{} {}:{}{}",
                    rel.kind.as_str(),
                    r.source_path,
                    rel.line,
                    r.source_symbol
                        .as_deref()
                        .map(|s| format!(" ({s})"))
                        .unwrap_or_default()
                )
            } else {
                let to = match (&r.target_symbol, &r.target_path) {
                    (Some(s), Some(p)) => format!("{s} in {p}"),
                    (None, Some(p)) => p.clone(),
                    _ => format!("{} (outside the project)", rel.target_name),
                };
                format!("{} {} (line {})", rel.kind.as_str(), to, rel.line)
            }
        };
        println!("  Uses ({}):", d.outgoing.len());
        for r in d.outgoing.iter().take(25) {
            println!(
                "    {}  [{}]",
                describe(r, false),
                r.relationship.certainty.as_str()
            );
        }
        if d.outgoing.len() > 25 {
            println!("    ... {} more (see --json)", d.outgoing.len() - 25);
        }
        println!("  Used by ({}):", d.incoming.len());
        for r in d.incoming.iter().take(25) {
            println!(
                "    {}  [{}]",
                describe(r, true),
                r.relationship.certainty.as_str()
            );
        }
        if d.incoming.len() > 25 {
            println!("    ... {} more (see --json)", d.incoming.len() - 25);
        }
        if !d.dependent_files.is_empty() {
            println!("  Dependent files: {}", d.dependent_files.join(", "));
        }
        if !d.diagnostics.is_empty() {
            println!("  Diagnostics ({}):", d.diagnostics.len());
            for x in &d.diagnostics {
                let x = &x.diagnostic;
                println!(
                    "    {} {}:{}  {}  [{}]",
                    x.severity, x.range.start_line, x.range.start_column, x.message, x.rule
                );
            }
        }
        if !d.external_references.is_empty() {
            println!("  External references ({}):", d.external_references.len());
            for e in &d.external_references {
                let status = e
                    .check
                    .as_ref()
                    .map(|c| format!("{} [{}]", c.severity, c.rule))
                    .unwrap_or_else(|| "not checked".into());
                println!("    {}  {}", e.reference.path, status);
            }
        }
        if !d.changes.is_empty() {
            println!("  Recorded changes ({}):", d.changes.len());
            for c in &d.changes {
                println!("    {}  {}", c.created_at, c.change);
            }
        }
    }
}

pub fn print_dependency_report(r: &DependencyReport) {
    let (title, empty) = match r.direction {
        Direction::Dependencies => ("Depends on", "No project files found."),
        Direction::Dependents => (
            "Depended on by",
            "No dependents found in the recorded relationships.",
        ),
    };
    println!("{title} ({} file(s)):", r.files.len());
    if r.files.is_empty() {
        println!("  {empty}");
    }
    for f in &r.files {
        println!("  {}  [{}]", f.path, f.certainty.as_str());
        println!("      {}", f.evidence);
        for rel in f.relationships.iter().take(8) {
            let x = &rel.relationship;
            let what = match (&rel.target_symbol, x.kind) {
                (Some(s), _) => format!("{} `{s}`", x.kind.as_str()),
                (None, RelationshipKind::Imports) => format!("imports `{}`", x.target_name),
                (None, k) => k.as_str().to_string(),
            };
            println!("      {}:{}  {}", rel.source_path, x.line, what);
        }
        if f.relationships.len() > 8 {
            println!("      ... {} more", f.relationships.len() - 8);
        }
    }
    if !r.packages.is_empty() {
        println!();
        println!(
            "Packages and modules outside the project ({}):",
            r.packages.len()
        );
        println!("  {}", r.packages.join(", "));
    }
    if !r.unresolved_relative.is_empty() {
        println!();
        println!(
            "Relative imports that name no project file ({}; see 'inspecteye check dependencies'):",
            r.unresolved_relative.len()
        );
        println!("  {}", r.unresolved_relative.join(", "));
    }
}

pub fn print_references_report(r: &ReferencesReport) {
    println!("Project files ({}):", r.project_files.len());
    if r.project_files.is_empty() {
        println!("  (none)");
    }
    for rel in &r.project_files {
        println!(
            "  {}:{}  {} {}  [{}]",
            rel.source_path,
            rel.relationship.line,
            rel.relationship.kind.as_str(),
            rel.target_path.as_deref().unwrap_or("?"),
            rel.relationship.certainty.as_str()
        );
    }
    println!();
    println!(
        "Packages and modules outside the project ({}):",
        r.packages.len()
    );
    if r.packages.is_empty() {
        println!("  (none)");
    }
    for p in &r.packages {
        println!("  {}  (used by {})", p.name, p.used_by.join(", "));
    }
    if !r.unresolved_relative.is_empty() {
        println!();
        println!(
            "Relative imports that name no project file ({}):",
            r.unresolved_relative.len()
        );
        for p in &r.unresolved_relative {
            println!("  {}  (in {})", p.name, p.used_by.join(", "));
        }
    }
    println!();
    println!("External filesystem paths ({}):", r.external.len());
    if r.external.is_empty() {
        println!("  (none)");
    } else {
        print_external_references(&r.external);
    }
}

pub fn print_diagnostics_report(r: &DiagnosticsReport) {
    print_diagnostics("Diagnostics", &r.diagnostics);
    if !r.external_references.is_empty() {
        println!();
        println!("External references:");
        print_external_references(&r.external_references);
    }
}

pub fn print_tree(tree: &TreeNode, flat: bool) {
    if flat {
        fn files(node: &TreeNode, out: &mut Vec<String>) {
            if node.file.is_some() {
                out.push(node.path.clone());
            }
            for c in &node.children {
                files(c, out);
            }
        }
        let mut out = Vec::new();
        files(tree, &mut out);
        for p in &out {
            println!("{p}");
        }
        println!();
        println!("{} file(s)", out.len());
        return;
    }
    println!("{}/  ({})", tree.name, files_label(tree.file_count));
    fn walk(node: &TreeNode, prefix: &str) {
        let n = node.children.len();
        for (i, c) in node.children.iter().enumerate() {
            let last = i + 1 == n;
            let branch = if last { "└── " } else { "├── " };
            match &c.file {
                Some(f) => {
                    let mut notes = Vec::new();
                    if let Some(l) = f.lines {
                        notes.push(format!("{l} lines"));
                    }
                    if f.kind != "source" {
                        notes.push(f.kind.clone());
                    }
                    if !f.git_tracked {
                        notes.push("untracked".to_string());
                    }
                    println!("{prefix}{branch}{}  ({})", c.name, notes.join(", "));
                }
                None => println!(
                    "{prefix}{branch}{}/  ({}){}",
                    c.name,
                    files_label(c.file_count),
                    if c.truncated { " …" } else { "" }
                ),
            }
            let next = format!("{prefix}{}", if last { "    " } else { "│   " });
            walk(c, &next);
        }
    }
    walk(tree, "");
}

fn files_label(n: usize) -> String {
    if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    }
}
