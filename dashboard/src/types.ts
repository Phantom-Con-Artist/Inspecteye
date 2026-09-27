// JSON shapes returned by the Inspectye core (same as the CLI's `--json` data).
// Only the fields the dashboard uses are declared.

export type Severity = "INFO" | "WARNING" | "ERROR";
export type Certainty = "fact" | "inference" | "possibility";

export interface SourceRange {
  start_line: number;
  start_column: number;
  end_line: number;
  end_column: number;
}

export interface AnalysisRun {
  started_at: string;
  finished_at: string | null;
  status: string;
  files_scanned: number;
  files_new: number;
  files_changed: number;
  files_unchanged: number;
  files_removed: number;
  files_failed: number;
}

export interface StatusReport {
  project_name: string;
  root_path: string;
  git_active: boolean;
  stats: {
    total_known_files: number;
    discovered_files: number;
    removed_files: number;
    git_tracked_files: number;
    untracked_files: number;
  };
  last_run: AnalysisRun | null;
  last_deep_scan: AnalysisRun | null;
}

export interface TreeFile {
  kind: string;
  language: string | null;
  lines: number | null;
  size_bytes: number;
  git_tracked: boolean;
  ignored: boolean;
}

export interface TreeNode {
  name: string;
  path: string;
  file?: TreeFile;
  children: TreeNode[];
  file_count: number;
  truncated: boolean;
}

export interface Diagnostic {
  category: "syntax" | "logic";
  rule: string;
  severity: Severity;
  certainty: Certainty;
  message: string;
  range: SourceRange;
  excerpt: string;
  evidence: string | null;
  symbol: string | null;
}

export interface StoredDiagnostic {
  file_id: number;
  path: string;
  diagnostic: Diagnostic;
}

export type Existence =
  | { state: "exists"; is_directory: boolean }
  | { state: "missing"; parent_exists: boolean }
  | { state: "unchecked"; reason: string };

export interface ExternalCheck {
  existence: Existence;
  inside_repository: boolean;
  machine_specific: string | null;
  rule: string;
  severity: Severity;
  certainty: Certainty;
  message: string;
}

export interface StoredExternalReference {
  file_path: string;
  reference: {
    raw: string;
    path: string;
    style: string;
    range: SourceRange;
    symbol: string | null;
    in_test_code: boolean;
  };
  check: ExternalCheck | null;
  checked_at: string | null;
}

export interface DiagnosticsReport {
  diagnostics: StoredDiagnostic[];
  external_references: StoredExternalReference[];
}

export interface RankRow {
  label: string;
  value: number;
  detail: string;
  path: string | null;
}

export interface Ranking {
  category: string;
  title: string;
  metric: string;
  rows: RankRow[];
}

export interface StoredSymbol {
  id: number;
  path: string;
  parent_id: number | null;
  symbol: {
    kind: string;
    name: string;
    qualified_name: string;
    signature: string;
    range: SourceRange;
    complexity: number | null;
  };
}

export interface StoredRelationship {
  relationship: {
    kind: string;
    certainty: Certainty;
    line: number;
    evidence: string;
    target_name: string;
  };
  source_path: string;
  source_symbol: string | null;
  target_path: string | null;
  target_symbol: string | null;
}

export interface RecordedChange {
  recorded_at: string;
  transition: string;
  added_lines: number;
  removed_lines: number;
}

export interface FileSummary {
  file: {
    path: string;
    kind: string;
    language: string | null;
    size_bytes: number;
    line_count: number | null;
    git_tracked: boolean;
    exists_on_disk: boolean;
  };
  analysis: { status: string; analyzed_at: string; error: string | null } | null;
  analysis_current: boolean;
  symbols: StoredSymbol[];
  diagnostics: StoredDiagnostic[];
  dependencies: StoredRelationship[];
  dependents: StoredRelationship[];
  external_references: StoredExternalReference[];
  recent_changes: RecordedChange[];
}

export interface CommitInfo {
  hash: string;
  author: string;
  date: string;
  subject: string;
  path: string;
}

export interface HistoryReport {
  path: string;
  git_available: boolean;
  entries: { commit: CommitInfo | null }[];
  recorded: RecordedChange[];
  symbol_changes: { qualified_name: string; kind: string; change: string; created_at: string }[];
}

export interface ImpactReport {
  origins: {
    path: string;
    source: string;
    change: string;
    modified_symbols: string[];
    removed_symbols: string[];
    whole_file: boolean;
  }[];
  max_depth: number;
  affected: {
    path: string;
    depth: number;
    certainty: Certainty;
    references_removed_symbol: boolean;
    via: { kind: string; dependent: string; dependency: string; symbol: string | null; line: number }[];
  }[];
}

export interface CheckFinding {
  category: string;
  rule: string;
  severity: Severity;
  certainty: Certainty;
  message: string;
  path: string;
  line: number | null;
  column: number | null;
  symbol: string | null;
  evidence: string | null;
  related_files: string[];
}

export interface CheckReport {
  scope: string;
  passed: boolean;
  counts: { errors: number; warnings: number; info: number };
  findings: CheckFinding[];
}

export interface RefreshResult {
  scan: { files_discovered: number; new_files: number; changed_files: number; removed_files: number };
  deep_scan: { analyzed: number; skipped: number; failed: number };
}

/** A finding from any source, normalized for lists. */
export interface Finding {
  source: "syntax" | "logic" | "external" | "dependency";
  severity: Severity;
  certainty: Certainty;
  rule: string;
  message: string;
  path: string;
  line: number | null;
  column: number | null;
  symbol: string | null;
  evidence: string | null;
  excerpt: string | null;
}
