// Small DOM helpers: required lookups, and keeping keyboard focus across a redraw.

/** The element `sel` must match. Throws a clear error instead of failing later on `null`. */
export function must<T extends Element = HTMLElement>(sel: string, parent: ParentNode = document): T {
  const el = parent.querySelector<T>(sel);
  if (!el) throw new Error(`Missing element: ${sel}`);
  return el;
}

/** A data attribute the element must carry (for example `data-id`). */
export function data(el: HTMLElement, key: string): string {
  const v = el.dataset[key];
  if (v === undefined) throw new Error(`Missing data-${key} on <${el.tagName.toLowerCase()}>`);
  return v;
}

export function say(el: Element | null, text: string): void {
  if (el) el.textContent = text;
}

/**
 * A selector that finds "the same control" after a redraw: its id, or its action and the thing
 * it acts on. Null when the element has neither.
 */
export function focusKey(el: Element | null): string | null {
  if (!(el instanceof HTMLElement)) return null;
  if (el.id) return `#${CSS.escape(el.id)}`;
  const action = el.dataset.action;
  if (!action) return null;
  const id = el.dataset.id;
  const quoted = (v: string) => `"${v.replace(/["\\]/g, "\\$&")}"`;
  return `[data-action=${quoted(action)}]${id === undefined ? "" : `[data-id=${quoted(id)}]`}`;
}

/** What had focus inside `root`, so `restoreFocus` can put it back after `root` is redrawn. */
export type FocusMemo = { inside: boolean; key: string | null };

export function captureFocus(root: HTMLElement): FocusMemo {
  const el = document.activeElement;
  const inside = !!el && el !== document.body && root.contains(el);
  return { inside, key: inside ? focusKey(el) : null };
}

/**
 * Puts focus back on the same control, or on the page's heading when that control is gone. Does
 * nothing when focus wasn't in `root` (a background redraw never steals focus).
 */
export function restoreFocus(root: HTMLElement, memo: FocusMemo): void {
  if (!memo.inside) return;
  const now = document.activeElement;
  if (now && now !== document.body && root.contains(now)) return; // the new view already placed focus
  const target = (memo.key && root.querySelector<HTMLElement>(memo.key)) || root.querySelector<HTMLElement>("h1, h2");
  if (!target) return;
  if (!target.matches("a[href], button, input, select, textarea, [tabindex]")) target.tabIndex = -1;
  target.focus({ preventScroll: true });
}
