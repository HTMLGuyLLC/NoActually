// The frame around every page: wordmark, sync status and tabs.
import type { SyncStatus } from "../api";
import { ago } from "../format";
import { type Html, html, raw } from "../html";
import { TABS, type Tab } from "../state";

/** The app icon's mark: a check in the text color over a red underline (same geometry as the icon). */
const MARK = raw(
  `<svg class="mark" viewBox="9 10.5 27 27" aria-hidden="true"><path class="mark-check" d="M12.9 24.1 L18.9 30.1 L31.8 13.8" transform="rotate(-4 22 22)"/><path class="mark-line" d="M10.7 35.2 Q22 32.7 34 35"/></svg>`,
);

export const wordmarkView = (cls = "brand"): Html => html`<span class="${cls}">${MARK}<span>No, Actually...</span></span>`;

const TAB_TITLES: Record<Tab, string> = { score: "Scorecard", sources: "Sources", settings: "Settings" };

export function statusPillView(status: SyncStatus, demo: boolean): Html {
  if (status.last_error) return html`<button class="sync-pill bad" data-action="sync-now" data-id="top" title="${status.last_error}">Couldn't check. Retry</button>`;
  if (status.syncing) return html`<span class="sync-pill"><i class="dot live"></i>Checking…</span>`;
  const checked = `Checked ${ago(status.last_sync)}`;
  return html`<span class="sync-pill" title="${checked}"><i class="dot"></i>${demo ? "Demo data" : checked}</span>`;
}

export function headerView(tab: Tab, status: SyncStatus, demo: boolean): Html {
  const tabs = TABS.map(
    (t) => html`<button class="tab${tab === t ? " on" : ""}" data-action="tab" data-id="${t}"${tab === t ? raw(' aria-current="page"') : ""}>${TAB_TITLES[t]}</button>`,
  );
  return html`<header class="top">${wordmarkView()}<div class="top-right">${statusPillView(status, demo)}<nav aria-label="Main">${tabs}</nav></div></header>`;
}
