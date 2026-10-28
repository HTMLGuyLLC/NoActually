// Markup helpers. Every string from chats goes through `esc` (or the `html` template, which calls it)
// before it touches innerHTML.

/** Escapes text for use in HTML content or a quoted attribute. */
export const esc = (s: unknown): string =>
  String(s ?? "").replace(/[&<>"']/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : c === '"' ? "&quot;" : "&#39;"));

export const num = (n: number): string => n.toLocaleString();

/** Markup that is already safe: built by `html`, or a trusted constant wrapped with `raw`. */
export class Html {
  constructor(readonly value: string) {}
  toString(): string {
    return this.value;
  }
}

/** Marks a trusted fragment (a constant SVG, say) so `html` inserts it as is. Never pass chat text here. */
export const raw = (markup: string): Html => new Html(markup);

/**
 * What may be interpolated. Booleans are left out on purpose: `${flag && html`…`}` would print
 * "false", so conditionals are written as ternaries and flags as `String(flag)`.
 */
export type HtmlValue = string | number | Html | null | undefined | readonly HtmlValue[];

const part = (v: HtmlValue): string => {
  if (v == null) return "";
  if (v instanceof Html) return v.value;
  if (Array.isArray(v)) return v.map(part).join("");
  return esc(v);
};

/** A template tag that escapes every interpolation unless it is `Html` (or a list of them). */
export function html(strings: TemplateStringsArray, ...values: readonly HtmlValue[]): Html {
  let out = strings[0] ?? "";
  values.forEach((v, i) => {
    out += part(v) + (strings[i + 1] ?? "");
  });
  return new Html(out);
}

export const EMPTY: Html = new Html("");
