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

const TAB_TITLES: Record<Tab, string> = { score: "Scorecard", improve: "Improve", learn: "Learn", sources: "Sources", settings: "Settings" };

/** Small line icons beside each tab's name (decoration: the name is the label). */
const svg = (d: string) => raw(`<svg class="tab-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${d}"/></svg>`);
const TAB_ICONS: Record<Tab, ReturnType<typeof raw>> = {
  // Bar chart.
  score: svg("M4 20V10M10 20V4M16 20v-7M22 20H2"),
  // A line climbing to a target.
  improve: svg("M3 17l6-6 4 4 8-8M15 7h6v6"),
  // An open book.
  learn: svg("M2 5h6a4 4 0 0 1 4 4v11a3 3 0 0 0-3-3H2zM22 5h-6a4 4 0 0 0-4 4v11a3 3 0 0 1 3-3h7z"),
  // Folders stacked: where the history comes from.
  sources: svg("M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"),
  // Cog.
  settings: svg(
    "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  ),
};

export function statusPillView(status: SyncStatus, demo: boolean): Html {
  if (status.last_error)
    return html`<button class="sync-pill bad" data-action="sync-now" data-id="top" title="${status.last_error}">Couldn't check. Retry</button>`;
  if (status.syncing) return html`<span class="sync-pill"><i class="dot live"></i>Checking…</span>`;
  const checked = `Checked ${ago(status.last_sync)}`;
  return html`<span class="sync-pill" title="${checked}"><i class="dot"></i>${demo ? "Demo data" : checked}</span>`;
}

/** Set up once and rarely visited: icons only, so the main sections have room. */
const UTILITY: readonly Tab[] = ["sources", "settings"];

export function headerView(tab: Tab, status: SyncStatus, demo: boolean): Html {
  const button = (t: Tab) => {
    const cls = `tab${UTILITY.includes(t) ? " icon-only" : ""}${tab === t ? " on" : ""}`;
    return html`<button class="${cls}" data-action="tab" data-id="${t}" title="${TAB_TITLES[t]}"${tab === t ? raw(' aria-current="page"') : ""}>${TAB_ICONS[t]}<span class="tab-name">${TAB_TITLES[t]}</span></button>`;
  };
  const tabs = html`${TABS.filter((t) => !UTILITY.includes(t)).map(button)}<span class="nav-gap" aria-hidden="true"></span>${UTILITY.map(button)}`;
  // The logo goes home: the scorecard's overview.
  const home = html`<button class="home" data-action="home" title="Go to the scorecard">${wordmarkView()}<span class="sr-only">, go to the scorecard</span></button>`;
  return html`<header class="top">${home}<div class="top-right"><nav aria-label="Main">${tabs}</nav>${statusPillView(status, demo)}</div></header>`;
}
