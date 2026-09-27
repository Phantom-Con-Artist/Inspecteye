// Findings: every stored diagnostic and external-reference finding, filterable.

import { card, certaintyTag, clear, empty, h, severityBadge } from "../dom.js";
import type { App } from "../main.js";
import type { Finding, Severity } from "../types.js";

const shown = new Set<Severity>(["ERROR", "WARNING"]);
let source = "all";
let search = "";

export function renderFindings(app: App): HTMLElement {
  const all = app.store.findings();
  const list = h("div", { class: "findings-list" });
  const count = h("span", { class: "muted" });

  const draw = () => {
    clear(list);
    const q = search.toLowerCase();
    const rows = all.filter(
      (f) =>
        shown.has(f.severity) &&
        (source === "all" || f.source === source) &&
        (!q || `${f.message} ${f.path} ${f.rule}`.toLowerCase().includes(q)),
    );
    count.textContent = `${rows.length} of ${all.length}`;
    if (rows.length === 0) {
      list.append(empty(all.length ? "Nothing matches the filters." : "No findings in the stored analysis."));
      return;
    }
    list.append(h("table", { class: "table findings" }, h("tbody", null, rows.slice(0, 500).map((f) => row(app, f)))));
    if (rows.length > 500) list.append(h("div", { class: "muted small" }, `Showing the first 500.`));
  };

  const sevToggle = (s: Severity) =>
    h(
      "label",
      { class: `toggle sev-${s.toLowerCase()}` },
      h("input", {
        type: "checkbox",
        checked: shown.has(s),
        onchange: (e: Event) => {
          if ((e.target as HTMLInputElement).checked) shown.add(s);
          else shown.delete(s);
          draw();
        },
      }),
      s,
    );

  const sourceSelect = h(
    "select",
    { onchange: (e: Event) => ((source = (e.target as HTMLSelectElement).value), draw()) },
    [
      ["all", "All sources"],
      ["syntax", "Syntax"],
      ["logic", "Logic"],
      ["external", "External paths"],
    ].map(([v, label]) => h("option", { value: v, selected: v === source }, label)),
  );

  draw();
  return h(
    "div",
    { class: "view" },
    h("h1", null, "Findings"),
    checkBanner(app),
    h(
      "div",
      { class: "toolbar" },
      sevToggle("ERROR"),
      sevToggle("WARNING"),
      sevToggle("INFO"),
      sourceSelect,
      h("input", {
        class: "filter",
        type: "search",
        placeholder: "Search message, file or rule…",
        value: search,
        oninput: (e: Event) => ((search = (e.target as HTMLInputElement).value), draw()),
      }),
      count,
    ),
    list,
  );
}

function row(app: App, f: Finding): HTMLElement {
  return h(
    "tr",
    { class: "clickable", onclick: () => app.openFile(f.path) },
    h("td", null, severityBadge(f.severity)),
    h(
      "td",
      { class: "grow" },
      h("div", null, f.message),
      h("div", { class: "muted small mono" }, `${f.path}${f.line ? `:${f.line}:${f.column}` : ""}${f.symbol ? ` · in ${f.symbol}` : ""}`),
      f.evidence ? h("div", { class: "muted small" }, f.evidence) : null,
    ),
    h("td", { class: "muted small mono nowrap" }, f.rule),
    h("td", null, certaintyTag(f.certainty)),
  );
}

/** Result of the last "Run checks", including dependency findings that are
 * computed on demand (broken imports, cycles). */
function checkBanner(app: App): HTMLElement | null {
  const r = app.store.lastCheck;
  if (!r) return null;
  const deps = r.findings.filter((f) => f.category === "dependency");
  return card(
    `Last check: ${r.passed ? "PASSED" : "FAILED"} — ${r.counts.errors} error(s), ${r.counts.warnings} warning(s), ${r.counts.info} info`,
    deps.length === 0
      ? h("div", { class: "muted small" }, "No broken imports or import cycles.")
      : h(
          "ul",
          { class: "list compact" },
          deps.map((f) =>
            h(
              "li",
              { class: "row clickable", onclick: () => app.openFile(f.path) },
              severityBadge(f.severity),
              h("span", { class: "grow" }, f.message),
              h("span", { class: "mono small muted" }, `${f.path}${f.line ? `:${f.line}` : ""}${f.related_files.length ? ` ↔ ${f.related_files.join(", ")}` : ""}`),
            ),
          ),
        ),
  );
}
