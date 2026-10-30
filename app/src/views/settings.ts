// Settings: sources, launch and updates, and what's kept for the Hall of fame, as sub-tabs.
import { isLevel, type Level, type MenuBar, type Retention, type SourceInfo, type SyncStatus, type UpdateStatus } from "../api";
import { type Html, html, raw } from "../html";
import { SETTINGS_PANES, type SettingsPane } from "../state";
import { updateLine } from "../update";
import { sectionNav, svg } from "./chrome";
import { sourcesView } from "./sources";

/** Retention days are a u32 on the Rust side; anything past ten years is "forever" in practice. */
export const RETENTION_MIN_DAYS = 1;
export const RETENTION_MAX_DAYS = 3650;
const DEFAULT_FULL_DAYS = 30;
const DEFAULT_SNIPPET_DAYS = 365;

/** The raw values of the privacy form. */
export type RetentionFields = { level: string | null; fullDays: string; snippetDays: string };

const days = (value: string, fallback: number) => {
  const n = Number(value);
  if (!n || !Number.isFinite(n)) return fallback; // blank, 0, or not a number
  return Math.min(RETENTION_MAX_DAYS, Math.max(RETENTION_MIN_DAYS, Math.round(n)));
};

/** Reads the privacy form: a known level, and whole days clamped to 1–3650 (blank or nonsense means the default). */
export function parseRetention(f: RetentionFields): Retention {
  if (!isLevel(f.level)) throw new Error("Pick what to keep first.");
  return { level: f.level, full_days: days(f.fullDays, DEFAULT_FULL_DAYS), snippet_days: days(f.snippetDays, DEFAULT_SNIPPET_DAYS) };
}

export type SettingsContext = {
  pane: SettingsPane;
  sources: SourceInfo[];
  status: SyncStatus;
  retention: Retention;
  autostart: boolean;
  menuBar: MenuBar;
  update: UpdateStatus | null;
  demo: boolean;
};

const attr = (on: boolean, name: string) => (on ? raw(` ${name}`) : "");

const PANE_TITLES: Record<SettingsPane, string> = { sources: "Sources", general: "General", privacy: "Privacy" };
const PANE_ICONS: Record<SettingsPane, Html> = {
  // Folders stacked: where the history comes from.
  sources: svg("M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"),
  // Sliders.
  general: svg("M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1M15 4v4M9 10v4M17 16v4"),
  // A padlock.
  privacy: svg("M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3"),
};

/** Settings: Sources, General and Privacy as sub-tabs, one showing at a time. */
export function settingsView(ctx: SettingsContext): Html {
  const nav = sectionNav({
    label: "Settings sections",
    action: "settings-pane",
    items: SETTINGS_PANES,
    current: ctx.pane,
    titles: PANE_TITLES,
    icons: PANE_ICONS,
  });
  const body = ctx.pane === "sources" ? sourcesView(ctx.sources, ctx.status) : ctx.pane === "general" ? generalView(ctx) : privacyView(ctx);
  return html`<h1 class="sr-only">Settings</h1><div class="views-row">${nav}</div><div class="view-body"><h2 class="sr-only">${PANE_TITLES[ctx.pane]}</h2>${body}</div>`;
}

function generalView({ autostart, menuBar, update, demo }: SettingsContext): Html {
  const off = attr(demo, "disabled");
  // Without a tray there's nothing to keep running in, so the toggle is locked.
  const trayOff = attr(demo || !menuBar.available, "disabled");
  const inTray = menuBar.enabled && menuBar.available;
  return html`
    <section>
      <div class="settings-row">
        <label class="toggle"><input id="menu-bar" data-action="menu-bar" type="checkbox" aria-describedby="menu-bar-help"${attr(menuBar.enabled, "checked")}${trayOff}/><span>Keep running in the menu bar</span></label>
        <span class="muted small" id="menu-bar-help">${menuBar.available ? "" : "Your system has no menu bar tray, so closing the window quits. "}On: closing the window keeps it counting from the menu bar (the system tray on Windows and Linux). Off: it's an ordinary app that only counts while it's open, and quits when you close it.</span>
      </div>
      <div class="settings-row">
        <label class="toggle"><input id="autostart" data-action="autostart" type="checkbox" aria-describedby="autostart-help"${attr(autostart, "checked")}${off}/><span>Launch at login</span></label>
        <span class="muted small" id="autostart-help">${inTray ? "Starts quietly in the menu bar so new corrections are counted." : "Opens the app when you log in."}</span>
      </div>
      <div class="settings-row">
        <label class="toggle"><input id="auto-update" data-action="auto-update" type="checkbox" aria-describedby="auto-update-help"${attr(!!update?.auto, "checked")}${off}/><span>Check for app updates automatically</span></label>
        <span class="muted small" id="auto-update-help">Asks GitHub for the latest version number at launch and every 6 hours. Sends nothing about you.</span>
      </div>
      <div class="settings-row">
        <button class="ghost" data-action="check-update"${off}>Check for app updates</button>
        <span class="muted small" id="update-line" aria-live="polite">${update ? updateLine(update) : ""}</span>
      </div>
    </section>`;
}

function privacyView({ retention: r }: SettingsContext): Html {
  const opt = (level: Level, title: string, body: string) =>
    html`<label class="level${r.level === level ? " on" : ""}"><input type="radio" name="level" value="${level}" data-action="level"${attr(
      r.level === level,
      "checked",
    )}/><span><b>${title}</b><br/><span class="muted small">${body}</span></span></label>`;
  return html`
    <section><div class="head">
      <p class="muted">Ordinary messages are never stored, only counted. This controls what's kept for corrections you'll see in the Hall of fame. Text always steps down as it ages, never up.</p></div>
      <fieldset class="levels"><legend class="sr-only">What to keep</legend>
        ${opt("full", "Full message", "Your whole message and the start of the AI's reply. Secrets, emails and links are still redacted.")}
        ${opt("snippet", "Snippet (recommended)", "Only the sentence that matched, up to 140 characters, redacted.")}
        ${opt("category", "Category only", "No text at all. Just “Made it up”, “Still broken”, and so on.")}
      </fieldset>
      <div class="ttl">
        <label>Keep full text for <input id="full-days" type="number" min="${RETENTION_MIN_DAYS}" max="${RETENTION_MAX_DAYS}" value="${r.full_days}"/> days, then snippet</label>
        <label>Keep snippets for <input id="snippet-days" type="number" min="${RETENTION_MIN_DAYS}" max="${RETENTION_MAX_DAYS}" value="${r.snippet_days}"/> days, then category only</label>
        <button data-action="save-privacy">Save</button><span id="privacy-status" class="muted small" role="status" aria-live="polite"></span>
      </div>
      <p class="muted small">Pinned corrections keep their text until you unpin them.</p>
      <p class="muted small fine-print">For fun, not a measure of job performance. Not affiliated with or endorsed by Anthropic, OpenAI or any other AI company. Product names are their owners' trademarks.</p>
    </section>`;
}
