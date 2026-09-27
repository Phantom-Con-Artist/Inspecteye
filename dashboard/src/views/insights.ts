// Insights: every ranking with its metric spelled out.

import { empty, h } from "../dom.js";
import type { App } from "../main.js";
import { rankingCard } from "./overview.js";

export function renderInsights(app: App): HTMLElement {
  const rankings = app.store.insights ?? [];
  return h(
    "div",
    { class: "view" },
    h("h1", null, "Insights"),
    h("p", { class: "muted" }, "Ranked by explicit counts; each list names its metric. There is no combined score."),
    rankings.length === 0
      ? empty("No insights yet. Refresh the analysis first.")
      : h(
          "div",
          { class: "grid two" },
          rankings.map((r) => rankingCard(app, r, r.title, "Nothing to rank.")),
        ),
  );
}
