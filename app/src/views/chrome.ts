// The frame around every page: wordmark, sync status and tabs.
import type { Stats, SyncStatus } from "../api";
import { ago, fmtDay } from "../format";
import { type Html, html, raw } from "../html";
import { TABS, type Tab } from "../state";

/** The app icon's mark: a check in the text color over a red underline (same geometry as the icon). */
const MARK = raw(
  `<svg class="mark" viewBox="9 10.5 27 27" aria-hidden="true"><path class="mark-check" d="M12.9 24.1 L18.9 30.1 L31.8 13.8" transform="rotate(-4 22 22)"/><path class="mark-line" d="M10.7 35.2 Q22 32.7 34 35"/></svg>`,
);

export const wordmarkView = (cls = "brand"): Html => html`<span class="${cls}">${MARK}<span>No, Actually...</span></span>`;

const TAB_TITLES: Record<Tab, string> = { score: "Scorecard", improve: "Improve", learn: "Learn", settings: "Settings" };

/** Small line icons beside each tab's name (decoration: the name is the label). */
export const svg = (d: string) => raw(`<svg class="tab-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${d}"/></svg>`);
export const TAB_ICONS: Record<Tab, ReturnType<typeof raw>> = {
  // Bar chart.
  score: svg("M4 20V10M10 20V4M16 20v-7M22 20H2"),
  // A line climbing to a target.
  improve: svg("M3 17l6-6 4 4 8-8M15 7h6v6"),
  // An open book.
  learn: svg("M2 5h6a4 4 0 0 1 4 4v11a3 3 0 0 0-3-3H2zM22 5h-6a4 4 0 0 0-4 4v11a3 3 0 0 1 3-3h7z"),
  // Cog.
  settings: svg(
    "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  ),
};

const REFRESH = raw(`<svg class="refresh" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>`);

const day = (iso: string) => fmtDay(iso, { month: "short", day: "numeric", year: "numeric" });

/** "Oct 2, 2026 – Oct 29, 2026 · 2,083 messages": what the numbers cover, for the status's tooltip. */
export function coverage(s: Pick<Stats, "first_day" | "last_day" | "messages"> | null): string {
  if (!s) return "";
  return `${s.first_day ? `${day(s.first_day)} – ${day(s.last_day ?? s.first_day)} · ` : ""}${s.messages.toLocaleString()} messages`;
}

/**
 * The sync status (hover or focus it to see what the numbers cover) and, beside it, a separate
 * "check now" button. Tooltips are the app's own (`data-tip`), mirrored for screen readers.
 */
export function statusPillView(status: SyncStatus, demo: boolean, covers = ""): Html {
  if (status.last_error)
    return html`<span class="sync-pill"><button class="sync-bad" data-action="sync-now" data-id="top" data-tip="${status.last_error}">Couldn't check. Retry</button></span>`;
  const checked = status.syncing ? "Checking…" : demo ? "Demo data" : `Checked ${ago(status.last_sync)}`;
  const text = covers
    ? html`<span class="sync-text" tabindex="0" data-tip="${covers}" aria-label="${checked}. Showing ${covers}">${checked}</span>`
    : html`<span class="sync-text">${checked}</span>`;
  const refresh = status.syncing
    ? ""
    : html`<button class="sync-now" data-action="sync-now" data-id="top" data-tip="Check now" aria-label="Check for new chats now">${REFRESH}</button>`;
  return html`<span class="sync-pill"><i class="dot${status.syncing ? " live" : ""}"></i>${text}${refresh}</span>`;
}

/** Set up once and rarely visited: icons only, so the main sections have room. */
const UTILITY: readonly Tab[] = ["settings"];

export function headerView(tab: Tab, status: SyncStatus, demo: boolean, covers = ""): Html {
  const button = (t: Tab) => {
    const cls = `tab${UTILITY.includes(t) ? " icon-only" : ""}${tab === t ? " on" : ""}`;
    return html`<button class="${cls}" data-action="tab" data-id="${t}" data-tip="${TAB_TITLES[t]}"${tab === t ? raw(' aria-current="page"') : ""}>${TAB_ICONS[t]}<span class="tab-name">${TAB_TITLES[t]}</span></button>`;
  };
  const tabs = html`${TABS.filter((t) => !UTILITY.includes(t)).map(button)}<span class="nav-gap" aria-hidden="true"></span>${UTILITY.map(button)}`;
  // The logo goes home: the scorecard's overview.
  const home = html`<button class="home" data-action="home" data-tip="Go to the scorecard">${wordmarkView()}<span class="sr-only">, go to the scorecard</span></button>`;
  return html`<header class="top">${home}<div class="top-right"><nav aria-label="Main">${tabs}</nav>${statusPillView(status, demo, covers)}</div></header>`;
}

/** A tier ladder, top tier first. Your tier's row carries your number ("88 · 80–89"), so no
 * note below is needed; before you're ranked, `unranked` says how far a ranking is. */
export function ladderView(o: {
  label: string;
  tiers: { id: string; name: string; range: string }[];
  on: string | null;
  at: string | null;
  unranked: string;
}): Html {
  const row = (t: { id: string; name: string; range: string }) =>
    t.id === o.on && o.at != null
      ? html`<li class="on" aria-current="true"><span>${t.name}</span><span><b class="at">${o.at}</b><span class="sr-only">, you're at ${o.at}. Range</span> · ${t.range}</span></li>`
      : html`<li><span>${t.name}</span><span>${t.range}</span></li>`;
  return html`<ol class="ladder" aria-label="${o.label}">${o.tiers.map(row)}</ol>${o.on ? "" : html`<p class="hand-note">${o.unranked}</p>`}`;
}

/** A verdict word, hand-written, with a screen-reader version. `word` is null until it's ranked ("Hmm."). */
export function verdictMark(word: string | null, srText: string, still: boolean): Html {
  return html`<div class="no${word ? "" : " maybe"}${still ? " still" : ""}" aria-hidden="true">${word ?? "Hmm."}</div>
    <span class="sr-only">${srText}</span>`;
}

/** Word-answered tiers label their ladder rows by the answer ("Religiously"), plainer than the name. */
export const byAnswer = <T extends { verdict: string }>(tiers: T[]): (T & { name: string })[] => tiers.map((t) => ({ ...t, name: answerWord(t) }));

/** A tier's answer as a label, without its full stop: "Religiously". */
export const answerWord = (t: { verdict: string }): string => t.verdict.replace(/\.$/, "");

/** The tier's name and what it means, under its answer: "Hawk-eyed. You catch AI slipping nearly every time." */
export const tierBlurb = (tier: { name: string; blurb: string } | undefined): Html | "" =>
  tier ? html`<p class="tier-blurb"><b>${tier.name}.</b> ${tier.blurb}</p>` : "";

/** The share icon: a box with an arrow leaving it. */
const SHARE_ICON = raw(
  `<svg class="btn-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v12M7 8l5-5 5 5M5 13v6a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-6"/></svg>`,
);

/** Every share button looks the same: the icon and a short label. `id` names what to share. */
export const shareButton = (action: string, label = "Share", o: { id?: number; ghost?: boolean; small?: boolean } = {}): Html => {
  const cls = [o.ghost ? "ghost" : "", o.small ? "small" : "", "share-btn-icon"].filter(Boolean).join(" ");
  return html`<button class="${cls}" data-action="${action}"${o.id == null ? "" : html` data-id="${o.id}"`}>${SHARE_ICON}${label}</button>`;
};
