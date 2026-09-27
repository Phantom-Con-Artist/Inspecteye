// Small DOM helpers: element construction and formatting.

import type { Certainty, Severity } from "./types.js";

export type Child = Node | string | number | null | undefined | false | Child[];
type Attrs = Record<string, string | number | boolean | EventListener | null | undefined>;

/** `h("div", { class: "x", onclick: fn }, "text", child)` */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs?: Attrs | null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs ?? {})) {
    if (value === null || value === undefined || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      el.addEventListener(key.slice(2), value as EventListener);
    } else if (value === true) {
      el.setAttribute(key, "");
    } else {
      el.setAttribute(key, String(value));
    }
  }
  append(el, children);
  return el;
}

export function append(parent: Node, children: Child[]): void {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    if (Array.isArray(c)) append(parent, c);
    else parent.appendChild(c instanceof Node ? c : document.createTextNode(String(c)));
  }
}

export function clear(el: Element): void {
  while (el.firstChild) el.removeChild(el.firstChild);
}

export function plural(n: number, word: string, many = `${word}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? word : many}`;
}

export function relativeTime(iso: string | null | undefined): string {
  if (!iso) return "never";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const s = Math.round((Date.now() - then) / 1000);
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const hours = Math.round(m / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return plural(days, "day") + " ago";
  return new Date(iso).toLocaleDateString();
}

export function severityBadge(s: Severity): HTMLElement {
  return h("span", { class: `badge sev-${s.toLowerCase()}` }, s);
}

export function certaintyTag(c: Certainty): HTMLElement {
  return h("span", { class: `tag cert-${c}`, title: certaintyHelp[c] }, c);
}

const certaintyHelp: Record<Certainty, string> = {
  fact: "Directly observed",
  inference: "Derived by a rule that can be wrong in unusual code",
  possibility: "Worth a look; legitimate code often looks like this",
};

export function severityRank(s: Severity): number {
  return s === "ERROR" ? 0 : s === "WARNING" ? 1 : 2;
}

export function loading(text = "Loading…"): HTMLElement {
  return h("div", { class: "loading" }, h("span", { class: "spinner" }), text);
}

export function empty(text: string): HTMLElement {
  return h("div", { class: "empty" }, text);
}

export function card(title: string, ...body: Child[]): HTMLElement {
  return h("section", { class: "card" }, h("h3", null, title), ...body);
}
