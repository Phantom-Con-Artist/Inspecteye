// App shell: header, navigation, and view switching.

import { query, QueryError } from "./api.js";
import { clear, h, loading, relativeTime } from "./dom.js";
import { Store } from "./store.js";
import type { CheckReport, RefreshResult } from "./types.js";
import { renderChanges } from "./views/changes.js";
import { renderFiles } from "./views/files.js";
import { renderFindings } from "./views/findings.js";
import { renderInsights } from "./views/insights.js";
import { renderOverview } from "./views/overview.js";

export type ViewName = "overview" | "files" | "findings" | "insights" | "changes";

const VIEWS: { name: ViewName; label: string; icon: string }[] = [
  { name: "overview", label: "Overview", icon: "◎" },
  { name: "files", label: "Files", icon: "▤" },
  { name: "findings", label: "Findings", icon: "⚑" },
  { name: "insights", label: "Insights", icon: "▥" },
  { name: "changes", label: "Changes", icon: "↻" },
];

export class App {
  readonly store = new Store();
  view: ViewName = "overview";
  /** Selected file in the Files view. */
  selectedFile: string | null = null;
  private busy: string | null = null;
  private notice: { text: string; kind: "ok" | "error" } | null = null;

  constructor(private readonly root: HTMLElement) {}

  async start(): Promise<void> {
    this.render();
    await this.store.loadAll();
    this.render();
  }

  navigate(view: ViewName, file?: string): void {
    this.view = view;
    if (file !== undefined) this.selectedFile = file;
    this.render();
  }

  openFile(path: string): void {
    this.navigate("files", path);
  }

  async refresh(): Promise<void> {
    await this.task("Refreshing analysis…", async () => {
      const r = await query<RefreshResult>("refresh");
      await this.store.loadAll();
      const changed = r.scan.new_files + r.scan.changed_files + r.scan.removed_files;
      return `Analysis up to date: ${changed} changed file(s), ${r.deep_scan.analyzed} re-analyzed.`;
    });
  }

  async runCheck(): Promise<void> {
    await this.task("Running checks…", async () => {
      const report = await query<CheckReport>("check");
      this.store.lastCheck = report;
      await this.store.loadAll();
      this.view = "findings";
      const c = report.counts;
      return `Check ${report.passed ? "passed" : "failed"}: ${c.errors} error(s), ${c.warnings} warning(s), ${c.info} info.`;
    });
  }

  private async task(label: string, work: () => Promise<string>): Promise<void> {
    if (this.busy) return;
    this.busy = label;
    this.notice = null;
    this.render();
    try {
      this.notice = { text: await work(), kind: "ok" };
    } catch (e) {
      this.notice = { text: e instanceof QueryError ? e.message : String(e), kind: "error" };
    } finally {
      this.busy = null;
      this.render();
    }
  }

  render(): void {
    clear(this.root);
    const s = this.store;
    const status = s.status;

    const header = h(
      "header",
      { class: "topbar" },
      h("div", { class: "brand" }, h("img", { src: "icon.png", alt: "" }), h("span", null, "Inspectye")),
      h(
        "div",
        { class: "project" },
        h("strong", null, status?.project_name ?? "…"),
        h("span", { class: "muted mono" }, status?.root_path ?? ""),
      ),
      h(
        "div",
        { class: "scan-state muted" },
        status
          ? [
              h("span", null, `Scanned ${relativeTime(status.last_run?.finished_at)}`),
              h("span", null, `Analyzed ${relativeTime(status.last_deep_scan?.finished_at)}`),
              h("span", null, status.git_active ? "Git ✓" : "No Git"),
            ]
          : null,
      ),
      h(
        "div",
        { class: "actions" },
        h(
          "button",
          { class: "btn", onclick: () => void this.refresh(), disabled: !!this.busy, title: "Scan the working tree and re-analyze changed files" },
          "↻ Refresh analysis",
        ),
        h(
          "button",
          { class: "btn primary", onclick: () => void this.runCheck(), disabled: !!this.busy, title: "Run syntax, logic and dependency checks" },
          "✓ Run checks",
        ),
      ),
    );

    const totals = s.status ? s.totals() : null;
    const nav = h(
      "nav",
      { class: "sidebar" },
      VIEWS.map((v) =>
        h(
          "button",
          {
            class: `nav-item${this.view === v.name ? " active" : ""}`,
            onclick: () => this.navigate(v.name),
          },
          h("span", { class: "nav-icon" }, v.icon),
          h("span", null, v.label),
          v.name === "findings" && totals && totals.errors + totals.warnings > 0
            ? h("span", { class: `count ${totals.errors ? "sev-error" : "sev-warning"}` }, totals.errors + totals.warnings)
            : null,
        ),
      ),
    );

    const main = h("main", { class: "content" });
    if (this.busy) main.append(h("div", { class: "notice busy" }, h("span", { class: "spinner" }), this.busy));
    if (this.notice) {
      main.append(
        h(
          "div",
          { class: `notice ${this.notice.kind}` },
          this.notice.text,
          h("button", { class: "link", onclick: () => ((this.notice = null), this.render()) }, "dismiss"),
        ),
      );
    }

    if (s.fatal) {
      main.append(this.fatalView(s.fatal));
    } else if (!s.status) {
      main.append(loading("Reading project intelligence…"));
    } else {
      const views: Record<ViewName, () => HTMLElement> = {
        overview: () => renderOverview(this),
        files: () => renderFiles(this),
        findings: () => renderFindings(this),
        insights: () => renderInsights(this),
        changes: () => renderChanges(this),
      };
      main.append(views[this.view]());
    }

    this.root.append(header, h("div", { class: "body" }, nav, main));
  }

  private fatalView(e: QueryError): HTMLElement {
    const notInit = e.kind === "not_initialized";
    return h(
      "div",
      { class: "fatal card" },
      h("h2", null, notInit ? "This project is not initialized" : "Could not read the project"),
      h("p", null, e.message),
      notInit
        ? h("p", null, "In a terminal in the project directory, run:", h("pre", null, "inspecteye init\ninspecteye scan\ninspecteye deep-scan"))
        : null,
      h("button", { class: "btn", onclick: () => void this.start() }, "Try again"),
    );
  }
}

void new App(document.getElementById("app")!).start();
