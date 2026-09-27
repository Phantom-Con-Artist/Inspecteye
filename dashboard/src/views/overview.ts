// At a glance: state, problems, composition, hubs, recent churn, pending impact.

import { card, certaintyTag, empty, h, plural, relativeTime, severityBadge } from "../dom.js";
import type { App } from "../main.js";
import type { Ranking, TreeNode } from "../types.js";

export function renderOverview(app: App): HTMLElement {
  const s = app.store;
  const status = s.status!;
  const totals = s.totals();
  const external = s.diagnostics?.external_references.length ?? 0;
  const pending = s.impact?.origins.filter((o) => o.source === "pending").length ?? 0;

  const stat = (label: string, value: number | string, hint: string, cls = "", onclick?: () => void) =>
    h(
      onclick ? "button" : "div",
      { class: `stat ${cls}${onclick ? " clickable" : ""}`, onclick: onclick ?? null },
      h("div", { class: "stat-value" }, typeof value === "number" ? value.toLocaleString() : value),
      h("div", { class: "stat-label" }, label),
      h("div", { class: "stat-hint muted" }, hint),
    );

  const stats = h(
    "div",
    { class: "stats" },
    stat("Files", status.stats.discovered_files, `${status.stats.git_tracked_files} tracked · ${status.stats.untracked_files} untracked`, "", () => app.navigate("files")),
    stat("Errors", totals.errors, "strong evidence of breakage", totals.errors ? "sev-error" : "", () => app.navigate("findings")),
    stat("Warnings", totals.warnings, "may be legitimate; worth a look", totals.warnings ? "sev-warning" : "", () => app.navigate("findings")),
    stat("External paths", external, "absolute paths in code/config", "", () => app.navigate("findings")),
    stat("Pending changes", pending, "edited since the last scan", pending ? "accent" : "", () => app.navigate("changes")),
  );

  return h(
    "div",
    { class: "view overview" },
    h("h1", null, "Overview"),
    stats,
    h(
      "div",
      { class: "grid two" },
      attention(app),
      h("div", { class: "stack" }, composition(s.tree), analysisState(app)),
      rankingCard(app, s.ranking("Dependency hubs"), "Dependency hubs", "No dependencies between project files yet."),
      rankingCard(app, s.ranking("Most changed files (Git)") ?? s.ranking("Most changed files (recorded)"), "Most changed files", "No change history yet."),
    ),
  );
}

function attention(app: App): HTMLElement {
  const top = app.store.findings().filter((f) => f.severity !== "INFO").slice(0, 8);
  return card(
    "Needs attention",
    top.length === 0
      ? empty("No errors or warnings in the stored analysis.")
      : h(
          "ul",
          { class: "list" },
          top.map((f) =>
            h(
              "li",
              { class: "row clickable", onclick: () => app.openFile(f.path) },
              severityBadge(f.severity),
              h("div", { class: "grow" }, h("div", { class: "msg" }, f.message), h("div", { class: "muted mono small" }, `${f.path}${f.line ? `:${f.line}` : ""} · ${f.rule}`)),
              certaintyTag(f.certainty),
            ),
          ),
        ),
    top.length > 0 ? h("button", { class: "link", onclick: () => app.navigate("findings") }, "All findings →") : null,
  );
}

function composition(tree: TreeNode | null): HTMLElement {
  const kinds = new Map<string, number>();
  const languages = new Map<string, number>();
  const walk = (n: TreeNode) => {
    if (n.file) {
      kinds.set(n.file.kind, (kinds.get(n.file.kind) ?? 0) + 1);
      if (n.file.language && n.file.kind === "source") {
        languages.set(n.file.language, (languages.get(n.file.language) ?? 0) + (n.file.lines ?? 0));
      }
    }
    n.children.forEach(walk);
  };
  if (tree) walk(tree);
  const total = [...kinds.values()].reduce((a, b) => a + b, 0) || 1;
  const sorted = [...kinds.entries()].sort((a, b) => b[1] - a[1]);
  const langs = [...languages.entries()].sort((a, b) => b[1] - a[1]).slice(0, 6);
  return card(
    "Composition",
    h(
      "div",
      { class: "stackbar" },
      sorted.map(([k, n]) => h("span", { class: `seg kind-${k}`, style: `flex:${n}`, title: `${k}: ${n}` })),
    ),
    h(
      "div",
      { class: "legend" },
      sorted.map(([k, n]) => h("span", null, h("i", { class: `dot kind-${k}` }), `${k} ${n} (${Math.round((n / total) * 100)}%)`)),
    ),
    langs.length
      ? h(
          "div",
          { class: "muted small langs" },
          "Source lines: ",
          langs.map(([l, n], i) => [i ? " · " : "", h("strong", null, l), ` ${n.toLocaleString()}`]),
        )
      : null,
  );
}

function analysisState(app: App): HTMLElement {
  const st = app.store.status!;
  const deep = st.last_deep_scan;
  const scan = st.last_run;
  return card(
    "Analysis",
    h(
      "dl",
      { class: "kv" },
      h("dt", null, "Last scan"),
      h("dd", null, scan ? `${relativeTime(scan.finished_at)} · ${scan.files_new} new, ${scan.files_changed} changed, ${scan.files_removed} removed` : "never"),
      h("dt", null, "Last deep scan"),
      h("dd", null, deep ? `${relativeTime(deep.finished_at)} · ${plural(deep.files_new, "file")} analyzed, ${deep.files_unchanged} up to date` : "never"),
      deep && deep.files_failed ? [h("dt", null, "Failed"), h("dd", { class: "sev-error-text" }, plural(deep.files_failed, "file"))] : null,
    ),
  );
}

function rankingCard(app: App, r: Ranking | undefined, title: string, none: string): HTMLElement {
  if (!r || r.rows.length === 0) return card(title, empty(none));
  const max = Math.max(...r.rows.map((x) => x.value), 1);
  return card(
    title,
    h("div", { class: "muted small" }, r.metric),
    h(
      "ul",
      { class: "bars" },
      r.rows.slice(0, 6).map((row) =>
        h(
          "li",
          { class: row.path ? "clickable" : "", onclick: row.path ? () => app.openFile(row.path!) : null, title: row.detail },
          h("span", { class: "bar-label mono" }, row.label),
          h("span", { class: "bar" }, h("span", { class: "fill", style: `width:${(row.value / max) * 100}%` })),
          h("span", { class: "bar-value" }, row.value.toLocaleString()),
        ),
      ),
    ),
  );
}

export { rankingCard };
