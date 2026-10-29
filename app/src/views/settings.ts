// Settings: launch at login, updates, sync, and what's kept for the Hall of Fame.
import { isLevel, type Level, type Retention, type UpdateStatus } from "../api";
import { type Html, html, raw } from "../html";
import { updateLine } from "../update";

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

export type SettingsContext = { retention: Retention; autostart: boolean; menuBar: boolean; update: UpdateStatus | null; demo: boolean };

const attr = (on: boolean, name: string) => (on ? raw(` ${name}`) : "");

export function settingsView({ retention: r, autostart, menuBar, update, demo }: SettingsContext): Html {
  const opt = (level: Level, title: string, body: string) =>
    html`<label class="level${r.level === level ? " on" : ""}"><input type="radio" name="level" value="${level}" data-action="level"${attr(
      r.level === level,
      "checked",
    )}/><span><b>${title}</b><br/><span class="muted small">${body}</span></span></label>`;
  const off = attr(demo, "disabled");
  return html`
    <section><div class="head"><h2>General</h2></div>
      <div class="settings-row">
        <label class="toggle"><input id="menu-bar" data-action="menu-bar" type="checkbox"${attr(menuBar, "checked")}${off}/><span>Keep running in the menu bar</span></label>
        <span class="muted small">On: closing the window keeps it counting from the menu bar (the system tray on Windows and Linux). Off: it's an ordinary app that only counts while it's open, and quits when you close it.</span>
      </div>
      <div class="settings-row">
        <label class="toggle"><input id="autostart" data-action="autostart" type="checkbox"${attr(autostart, "checked")}${off}/><span>Launch at login</span></label>
        <span class="muted small">${menuBar ? "Starts quietly in the menu bar so new catches are counted." : "Opens the app when you log in."}</span>
      </div>
      <div class="settings-row">
        <label class="toggle"><input id="auto-update" data-action="auto-update" type="checkbox"${attr(!!update?.auto, "checked")}${off}/><span>Check for app updates automatically</span></label>
        <span class="muted small">Asks GitHub for the latest version number at launch and every 6 hours. Sends nothing about you.</span>
      </div>
      <div class="settings-row">
        <button class="ghost" data-action="check-update"${off}>Check for app updates</button>
        <span class="muted small" id="update-line" aria-live="polite">${update ? updateLine(update) : ""}</span>
      </div>
    </section>
    <section><div class="head"><h2>Privacy</h2>
      <p class="muted">Ordinary messages are never stored, only counted. This controls what's kept for catches you'll see in the Hall of Fame. Text always steps down as it ages, never up.</p></div>
      <div class="levels">
        ${opt("full", "Full message", "Your whole message and the start of the AI's reply. Secrets, emails and links are still redacted.")}
        ${opt("snippet", "Snippet (recommended)", "Only the sentence that matched, up to 140 characters, redacted.")}
        ${opt("category", "Category only", "No text at all. Just “Made it up”, “Still broken”, and so on.")}
      </div>
      <div class="ttl">
        <label>Keep full text for <input id="full-days" type="number" min="${RETENTION_MIN_DAYS}" max="${RETENTION_MAX_DAYS}" value="${r.full_days}"/> days, then snippet</label>
        <label>Keep snippets for <input id="snippet-days" type="number" min="${RETENTION_MIN_DAYS}" max="${RETENTION_MAX_DAYS}" value="${r.snippet_days}"/> days, then category only</label>
        <button data-action="save-privacy">Save</button><span id="privacy-status" class="muted small" role="status" aria-live="polite"></span>
      </div>
      <p class="muted small">Pinned catches keep their text until you unpin them.</p>
    </section>`;
}
