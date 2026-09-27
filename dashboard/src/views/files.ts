// Files: the project tree beside everything known about the selected file.

import { query, QueryError } from "../api.js";
import { card, certaintyTag, clear, empty, h, loading, plural, relativeTime, severityBadge } from "../dom.js";
import type { App } from "../main.js";
import type { FileSummary, HistoryReport, ImpactReport, StoredRelationship, StoredSymbol, TreeNode } from "../types.js";

const expanded = new Set<string>([""]);
let filterText = "";

export function renderFiles(app: App): HTMLElement {
  const detail = h("div", { class: "file-detail" });
  const treeBox = h("div", { class: "tree-scroll" });
  const drawTree = () => {
    clear(treeBox);
    const tree = app.store.tree;
    if (!tree) treeBox.append(empty("No files indexed."));
    else treeBox.append(treeList(app, tree, filterText.trim().toLowerCase(), drawTree));
  };
  const filter = h("input", {
    class: "filter",
    type: "search",
    placeholder: "Filter files…",
    value: filterText,
    oninput: (e: Event) => {
      filterText = (e.target as HTMLInputElement).value;
      drawTree();
    },
  });
  drawTree();

  if (app.selectedFile) void loadDetail(app, app.selectedFile, detail);
  else detail.append(empty("Select a file to see its symbols, findings, dependencies and history."));

  return h(
    "div",
    { class: "view files" },
    h("aside", { class: "tree-pane" }, filter, treeBox),
    detail,
  );
}

function treeList(app: App, root: TreeNode, filter: string, redraw: () => void): HTMLElement {
  const counts = app.store.countsByFile();
  const matches = (n: TreeNode): boolean =>
    n.file ? n.path.toLowerCase().includes(filter) : n.children.some(matches);

  const node = (n: TreeNode): HTMLElement | null => {
    if (filter && !matches(n)) return null;
    if (n.file) {
      const c = counts.get(n.path);
      return h(
        "li",
        {
          class: `file${app.selectedFile === n.path ? " selected" : ""}`,
          onclick: () => app.openFile(n.path),
          title: n.path,
        },
        h("span", { class: `dot kind-${n.file.kind}` }),
        h("span", { class: "name" }, n.name),
        c?.errors ? h("span", { class: "count sev-error" }, c.errors) : null,
        c?.warnings ? h("span", { class: "count sev-warning" }, c.warnings) : null,
      );
    }
    const open = filter !== "" || expanded.has(n.path);
    const li = h(
      "li",
      { class: "dir" },
      h(
        "div",
        {
          class: "dir-label",
          onclick: () => {
            if (expanded.has(n.path)) expanded.delete(n.path);
            else expanded.add(n.path);
            redraw();
          },
        },
        h("span", { class: "caret" }, open ? "▾" : "▸"),
        h("span", { class: "name" }, n.name),
        h("span", { class: "muted small" }, n.file_count),
      ),
    );
    if (open) li.append(h("ul", null, n.children.map(node)));
    return li;
  };
  return h("ul", { class: "tree" }, root.children.map(node));
}

async function loadDetail(app: App, path: string, box: HTMLElement): Promise<void> {
  box.append(loading());
  let f: FileSummary;
  try {
    f = await query<FileSummary>("file", path);
  } catch (e) {
    clear(box);
    box.append(empty(e instanceof QueryError ? e.message : String(e)));
    return;
  }
  clear(box);
  const file = f.file;
  const findings = app.store.findings().filter((x) => x.path === path);
  const others = (rels: StoredRelationship[], pick: (r: StoredRelationship) => string | null) =>
    [...new Set(rels.map(pick).filter((p): p is string => !!p && p !== path))].sort();
  const dependsOn = others(f.dependencies, (r) => r.target_path);
  const usedBy = others(f.dependents, (r) => r.source_path);
  const packages = [...new Set(f.dependencies.filter((r) => !r.target_path && r.relationship.kind === "imports").map((r) => r.relationship.target_name))].sort();

  const extra = h("div", { class: "stack" });
  box.append(
    h(
      "div",
      { class: "detail-head" },
      h("h1", { class: "mono" }, file.path),
      h(
        "div",
        { class: "chips" },
        h("span", { class: `chip kind-${file.kind}` }, file.kind),
        file.language ? h("span", { class: "chip" }, file.language) : null,
        file.line_count !== null ? h("span", { class: "chip" }, plural(file.line_count, "line")) : null,
        h("span", { class: "chip" }, file.git_tracked ? "Git-tracked" : "untracked"),
        f.analysis ? h("span", { class: "chip" }, `analyzed ${relativeTime(f.analysis.analyzed_at)}`) : h("span", { class: "chip warn" }, "not analyzed"),
      ),
      !f.analysis_current && f.analysis
        ? h("div", { class: "notice error" }, "The file changed after its last analysis; what follows describes an older version. Use “Refresh analysis”.")
        : null,
      h(
        "div",
        { class: "actions" },
        h("button", { class: "btn", onclick: () => void showImpact(path, extra) }, "What could this affect?"),
        h("button", { class: "btn", onclick: () => void showHistory(path, extra) }, "History"),
      ),
    ),
    extra,
    h(
      "div",
      { class: "grid two" },
      card(
        `Findings (${findings.length})`,
        findings.length === 0
          ? empty("None in the stored analysis.")
          : h(
              "ul",
              { class: "list" },
              findings.map((x) =>
                h(
                  "li",
                  { class: "row col" },
                  h("div", { class: "row" }, severityBadge(x.severity), h("span", { class: "grow" }, x.message), certaintyTag(x.certainty)),
                  h("div", { class: "muted small mono" }, `line ${x.line ?? "-"} · ${x.rule}${x.symbol ? ` · in ${x.symbol}` : ""}`),
                  x.excerpt ? h("pre", { class: "excerpt" }, x.excerpt) : null,
                ),
              ),
            ),
      ),
      card(
        "Connections",
        linkList(app, `Depends on (${dependsOn.length})`, dependsOn),
        linkList(app, `Used by (${usedBy.length})`, usedBy),
        packages.length ? h("div", { class: "sub" }, h("h4", null, `Packages (${packages.length})`), h("div", { class: "mono small wrap" }, packages.join(", "))) : null,
      ),
      card(`Symbols (${f.symbols.length})`, symbolTable(f.symbols)),
      card(
        "External paths & recent changes",
        f.external_references.length
          ? h(
              "ul",
              { class: "list" },
              f.external_references.map((e) =>
                h(
                  "li",
                  { class: "row" },
                  e.check ? severityBadge(e.check.severity) : null,
                  h("span", { class: "mono grow" }, e.reference.path),
                  h("span", { class: "muted small" }, e.check?.existence.state ?? "unchecked"),
                ),
              ),
            )
          : h("div", { class: "muted small" }, "No absolute filesystem paths."),
        h("h4", null, "Recorded changes"),
        f.recent_changes.length
          ? h(
              "ul",
              { class: "list compact" },
              f.recent_changes.map((c) =>
                h("li", { class: "row" }, h("span", { class: "grow" }, c.transition), h("span", { class: "add" }, `+${c.added_lines}`), h("span", { class: "del" }, `−${c.removed_lines}`), h("span", { class: "muted small" }, relativeTime(c.recorded_at))),
              ),
            )
          : h("div", { class: "muted small" }, "None recorded."),
      ),
    ),
  );
}

function linkList(app: App, title: string, paths: string[]): HTMLElement {
  return h(
    "div",
    { class: "sub" },
    h("h4", null, title),
    paths.length === 0
      ? h("div", { class: "muted small" }, "none")
      : h("ul", { class: "list compact" }, paths.map((p) => h("li", { class: "row clickable mono small", onclick: () => app.openFile(p) }, p))),
  );
}

function symbolTable(symbols: StoredSymbol[]): HTMLElement {
  if (symbols.length === 0) return empty("No symbols (not a parsed language, or not analyzed).");
  const depth = new Map<number, number>();
  const byId = new Map(symbols.map((s) => [s.id, s]));
  const depthOf = (s: StoredSymbol): number => {
    if (depth.has(s.id)) return depth.get(s.id)!;
    const d = s.parent_id && byId.has(s.parent_id) ? depthOf(byId.get(s.parent_id)!) + 1 : 0;
    depth.set(s.id, d);
    return d;
  };
  return h(
    "table",
    { class: "table" },
    h("thead", null, h("tr", null, h("th", null, "Symbol"), h("th", null, "Kind"), h("th", { class: "num" }, "Lines"), h("th", { class: "num", title: "Cyclomatic complexity" }, "CC"))),
    h(
      "tbody",
      null,
      symbols.map((s) =>
        h(
          "tr",
          { title: s.symbol.signature },
          h("td", { class: "mono", style: `padding-left:${8 + depthOf(s) * 16}px` }, s.symbol.name),
          h("td", { class: "muted" }, s.symbol.kind),
          h("td", { class: "num muted" }, `${s.symbol.range.start_line}–${s.symbol.range.end_line}`),
          h("td", { class: `num${(s.symbol.complexity ?? 0) >= 10 ? " sev-warning-text" : ""}` }, s.symbol.complexity ?? ""),
        ),
      ),
    ),
  );
}

async function showHistory(path: string, box: HTMLElement): Promise<void> {
  clear(box);
  box.append(loading("Reading Git history…"));
  try {
    const r = await query<HistoryReport>("history", path);
    clear(box);
    const commits = r.entries.filter((e) => e.commit).map((e) => e.commit!);
    box.append(
      card(
        `History (${commits.length} commit${commits.length === 1 ? "" : "s"})`,
        !r.git_available ? h("div", { class: "muted" }, "Not a Git repository.") : null,
        h(
          "ul",
          { class: "list compact" },
          commits.map((c) =>
            h("li", { class: "row" }, h("span", { class: "mono small" }, c.hash.slice(0, 8)), h("span", { class: "grow" }, c.subject), h("span", { class: "muted small" }, `${c.author} · ${relativeTime(c.date)}`)),
          ),
        ),
        r.symbol_changes.length
          ? [
              h("h4", null, "Symbol changes recorded by Inspectye"),
              h(
                "ul",
                { class: "list compact" },
                r.symbol_changes.slice(-15).reverse().map((c) =>
                  h("li", { class: "row" }, h("span", { class: `tag change-${c.change}` }, c.change), h("span", { class: "mono grow" }, `${c.kind} ${c.qualified_name}`), h("span", { class: "muted small" }, relativeTime(c.created_at))),
                ),
              ),
            ]
          : null,
      ),
    );
  } catch (e) {
    clear(box);
    box.append(empty(e instanceof QueryError ? e.message : String(e)));
  }
}

async function showImpact(path: string, box: HTMLElement): Promise<void> {
  clear(box);
  box.append(loading("Following dependents…"));
  try {
    const r = await query<ImpactReport>("file-impact", path);
    clear(box);
    box.append(
      card(
        `If ${path} changes`,
        r.affected.length === 0
          ? empty("No dependents found in the recorded relationships.")
          : h(
              "ul",
              { class: "list compact" },
              r.affected.map((a) =>
                h(
                  "li",
                  { class: "row" },
                  h("span", { class: "tag" }, `depth ${a.depth}`),
                  h("span", { class: "mono grow" }, a.path),
                  certaintyTag(a.certainty),
                  h("span", { class: "muted small" }, a.via.slice(0, 2).map((v) => `${v.kind}${v.symbol ? ` ${v.symbol}` : ""} (line ${v.line})`).join(", ")),
                ),
              ),
            ),
        h("div", { class: "muted small" }, "Potential impact from static relationships, not a guarantee of breakage."),
      ),
    );
  } catch (e) {
    clear(box);
    box.append(empty(e instanceof QueryError ? e.message : String(e)));
  }
}
