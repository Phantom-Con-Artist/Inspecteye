// Changes: working-tree edits not yet scanned, and what they may affect.

import { card, certaintyTag, empty, h } from "../dom.js";
import type { App } from "../main.js";

export function renderChanges(app: App): HTMLElement {
  const r = app.store.impact;
  const view = h("div", { class: "view" }, h("h1", null, "Changes & impact"));
  if (!r || r.origins.length === 0) {
    view.append(empty("No changes: the working tree matches the last scan, and the last scan recorded no modifications or removals."));
    return view;
  }
  view.append(
    card(
      `Changed files (${r.origins.length})`,
      h(
        "ul",
        { class: "list" },
        r.origins.map((o) =>
          h(
            "li",
            { class: "row clickable", onclick: () => app.openFile(o.path) },
            h("span", { class: `tag change-${o.change}` }, o.change),
            h("span", { class: "mono grow" }, o.path),
            h(
              "span",
              { class: "muted small" },
              o.whole_file
                ? "whole file"
                : [o.modified_symbols.length ? `modified: ${o.modified_symbols.join(", ")}` : "", o.removed_symbols.length ? `removed: ${o.removed_symbols.join(", ")}` : ""]
                    .filter(Boolean)
                    .join(" · ") || "no declarations changed",
            ),
            h("span", { class: "tag" }, o.source),
          ),
        ),
      ),
    ),
    card(
      `Potentially affected (${r.affected.length})`,
      h("div", { class: "muted small" }, `Files that depend on the changes, up to ${r.max_depth} levels. Certainty is the weakest link of the chain.`),
      r.affected.length === 0
        ? empty("No dependents found in the recorded relationships.")
        : h(
            "table",
            { class: "table" },
            h("thead", null, h("tr", null, h("th", null, "File"), h("th", null, "Depth"), h("th", null, "Via"), h("th", null, "Certainty"))),
            h(
              "tbody",
              null,
              r.affected.map((a) =>
                h(
                  "tr",
                  { class: "clickable", onclick: () => app.openFile(a.path) },
                  h("td", { class: "mono" }, a.path, a.references_removed_symbol ? h("span", { class: "badge sev-error" }, "uses removed symbol") : null),
                  h("td", { class: "num" }, a.depth),
                  h("td", { class: "muted small" }, a.via.slice(0, 3).map((v) => `${v.kind}${v.symbol ? ` ${v.symbol}` : ""} → ${v.dependency}:${v.line}`).join("; ")),
                  h("td", null, certaintyTag(a.certainty)),
                ),
              ),
            ),
          ),
    ),
  );
  return view;
}
