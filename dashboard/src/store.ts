// Project data loaded from the core, shared by all views.

import { query, QueryError } from "./api.js";
import type {
  CheckReport,
  DiagnosticsReport,
  Finding,
  ImpactReport,
  Ranking,
  StatusReport,
  TreeNode,
} from "./types.js";

export interface FileCounts {
  errors: number;
  warnings: number;
  info: number;
}

export class Store {
  status: StatusReport | null = null;
  tree: TreeNode | null = null;
  diagnostics: DiagnosticsReport | null = null;
  insights: Ranking[] | null = null;
  impact: ImpactReport | null = null;
  lastCheck: CheckReport | null = null;
  /** Set when the project cannot be read at all (e.g. not initialized). */
  fatal: QueryError | null = null;

  private findingsCache: Finding[] | null = null;
  private countsCache: Map<string, FileCounts> | null = null;

  async loadAll(): Promise<void> {
    this.fatal = null;
    try {
      this.status = await query<StatusReport>("status");
    } catch (e) {
      this.fatal = e instanceof QueryError ? e : new QueryError("error", String(e));
      return;
    }
    // Sequential: the backend serializes queries anyway.
    this.tree = await query<TreeNode>("tree").catch(() => null);
    this.diagnostics = await query<DiagnosticsReport>("diagnostics").catch(() => null);
    this.insights = await query<Ranking[]>("insights").catch(() => null);
    this.impact = await query<ImpactReport>("impact").catch(() => null);
    this.findingsCache = null;
    this.countsCache = null;
  }

  /** Stored diagnostics and external-reference findings, most severe first. */
  findings(): Finding[] {
    if (this.findingsCache) return this.findingsCache;
    const out: Finding[] = [];
    for (const d of this.diagnostics?.diagnostics ?? []) {
      const x = d.diagnostic;
      out.push({
        source: x.category,
        severity: x.severity,
        certainty: x.certainty,
        rule: x.rule,
        message: x.message,
        path: d.path,
        line: x.range.start_line,
        column: x.range.start_column,
        symbol: x.symbol,
        evidence: x.evidence,
        excerpt: x.excerpt || null,
      });
    }
    for (const e of this.diagnostics?.external_references ?? []) {
      if (!e.check) continue;
      out.push({
        source: "external",
        severity: e.check.severity,
        certainty: e.check.certainty,
        rule: e.check.rule,
        message: e.check.message,
        path: e.file_path,
        line: e.reference.range.start_line,
        column: e.reference.range.start_column,
        symbol: e.reference.symbol,
        evidence: e.check.machine_specific,
        excerpt: null,
      });
    }
    const rank = (s: string) => (s === "ERROR" ? 0 : s === "WARNING" ? 1 : 2);
    out.sort((a, b) => rank(a.severity) - rank(b.severity) || a.path.localeCompare(b.path) || (a.line ?? 0) - (b.line ?? 0));
    this.findingsCache = out;
    return out;
  }

  countsByFile(): Map<string, FileCounts> {
    if (this.countsCache) return this.countsCache;
    const map = new Map<string, FileCounts>();
    for (const f of this.findings()) {
      const c = map.get(f.path) ?? { errors: 0, warnings: 0, info: 0 };
      if (f.severity === "ERROR") c.errors++;
      else if (f.severity === "WARNING") c.warnings++;
      else c.info++;
      map.set(f.path, c);
    }
    this.countsCache = map;
    return map;
  }

  totals(): FileCounts {
    const t = { errors: 0, warnings: 0, info: 0 };
    for (const c of this.countsByFile().values()) {
      t.errors += c.errors;
      t.warnings += c.warnings;
      t.info += c.info;
    }
    return t;
  }

  ranking(title: string): Ranking | undefined {
    return this.insights?.find((r) => r.title === title);
  }
}
