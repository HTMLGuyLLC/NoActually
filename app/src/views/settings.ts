// Settings: sources, launch and updates, report cards, and what's kept for the Hall of fame, as sub-tabs.
import {
  isLevel,
  type Level,
  type MenuBar,
  REPORT_CARD_TOOLS,
  type ReportCardTool,
  type ReportCardToolStatus,
  type Retention,
  type SourceInfo,
  type SyncStatus,
  type UpdateStatus,
} from "../api";
import { fmtDay, plural } from "../format";
import { EMPTY, type Html, html, raw } from "../html";
import { TOOL_WORDS } from "../report-cards";
import { type ReportCardsState, SETTINGS_PANES, type SettingsPane } from "../state";
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
  reportCards: ReportCardsState;
  /** What your AI is told when tips are on. */
  tipRules: string;
  update: UpdateStatus | null;
  demo: boolean;
};

const attr = (on: boolean, name: string) => (on ? raw(` ${name}`) : "");

const PANE_TITLES: Record<SettingsPane, string> = { sources: "Sources", general: "General", "report-cards": "Report cards", privacy: "Privacy" };
const PANE_ICONS: Record<SettingsPane, Html> = {
  // Folders stacked: where the history comes from.
  sources: svg("M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"),
  // Sliders.
  general: svg("M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1M15 4v4M9 10v4M17 16v4"),
  // A clipboard with a check.
  "report-cards": svg("M9 4h6v3H9zM15 5h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2M9 14l2 2 4-4"),
  // A padlock.
  privacy: svg("M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3"),
};

/** Settings: Sources, General, Report cards and Privacy as sub-tabs, one showing at a time. */
export function settingsView(ctx: SettingsContext): Html {
  const nav = sectionNav({
    label: "Settings sections",
    action: "settings-pane",
    items: SETTINGS_PANES,
    current: ctx.pane,
    titles: PANE_TITLES,
    icons: PANE_ICONS,
  });
  const body = {
    sources: () => sourcesView(ctx.sources, ctx.status),
    general: () => generalView(ctx),
    "report-cards": () => reportCardsView(ctx.reportCards, ctx.sources, ctx.tipRules),
    privacy: () => privacyView(ctx),
  }[ctx.pane]();
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
        <span class="muted small" id="auto-update-help">Asks GitHub for the latest version number a minute after launch and every 6 hours. Sends no account, stats or chat data. GitHub sees your IP address, as with any download.</span>
      </div>
      <div class="settings-row">
        <label class="toggle" for="update-channel">Update channel</label>
        <select id="update-channel" data-action="update-channel" aria-describedby="update-channel-help"${off}>
          <option value="stable"${attr(update?.channel !== "beta", "selected")}>Stable</option>
          <option value="beta"${attr(update?.channel === "beta", "selected")}>Beta</option>
        </select>
        <span class="muted small" id="update-channel-help">Stable gets full releases. Beta gets new versions first, and they may have bugs. Going back to Stable keeps your version until a newer full release comes out.</span>
      </div>
      <div class="settings-row">
        <button class="ghost" data-action="check-update"${off}>Check for app updates</button>
        <span class="muted small" id="update-line" aria-live="polite">${update ? updateLine(update) : ""}</span>
      </div>
      ${
        demo
          ? ""
          : html`<div class="settings-row">
        <button class="ghost" data-action="open-log-folder">Show log folder</button>
        <span class="muted small">Where the app writes what went wrong. Handy to attach when you ask for help.</span>
      </div>`
      }
    </section>`;
}

/** What a tool that wasn't found says: where it might come from, and to press Look again once it's there. On a Mac, Claude Code also comes with the Claude desktop app, which downloads it (those folders are searched only there). */
const notFound = (tool: ReportCardTool): string =>
  tool === "claude_code" && navigator.userAgent.includes("Macintosh")
    ? "Not found on this computer. Install Claude Code, or open the Claude desktop app once, then press Look again."
    : `Not found on this computer. Install ${TOOL_WORDS[tool].name}, then press Look again.`;

/** Report cards that were written but couldn't be counted. */
const leftOut = (n: number): Html =>
  n > 0 ? html`<span class="muted small">${plural(n, "report card")} couldn't be counted. See the log for why.</span>` : EMPTY;

/** The text under a tool's name: not found, or on and how it's going. Off says nothing. */
function toolState(tool: ReportCardTool, st: ReportCardToolStatus, reading: boolean): Html {
  if (!st.installed_tool) {
    return html`<span class="muted small">${notFound(tool)}</span><span class="muted small">Report cards work in Claude Code and Codex, including the Code tab in the Claude desktop app, not in regular Claude or ChatGPT chats.</span>`;
  }
  if (!st.on) return leftOut(st.left_out);
  const { name, command } = TOOL_WORDS[tool];
  // Report cards read the chat, so they can't run while its source is stopped or paused.
  const paused = reading ? EMPTY : html`<span class="muted small">Paused: report cards won't run while No, Actually... isn't reading ${name}.</span>`;
  const on = st.last_card_day
    ? html`<span class="muted small"><span class="status">On</span> · last report card ${fmtDay(st.last_card_day)}</span>`
    : html`<span class="muted small"><span class="status">On</span></span><span class="muted small">No report cards yet. Type ${command} after your next task.</span>`;
  return html`${on}${paused}${leftOut(st.left_out)}`;
}

/** What a failure or a finished job says beside its control. Not an alert: these are drawn again on every redraw, and the app announces each once. */
const said = (kind: "error" | "note", text: string | undefined): Html | "" =>
  text ? html`<span class="${kind === "error" ? "action-error" : "action-note muted small"}">${text}</span>` : "";

function toolRow(tool: ReportCardTool, st: ReportCardToolStatus, rc: ReportCardsState, allowed: boolean, reading: boolean): Html {
  const { name } = TOOL_WORDS[tool];
  const working = rc.busy === tool;
  const verb = st.on ? "Turn off" : "Turn on";
  const doing = st.on ? "Turning off" : "Turning on";
  const label = working ? `${doing}…` : verb;
  // Not found: it may only be somewhere the quick search doesn't look, so offer to search harder.
  // Not allowed: a report card is read from the chat, so the source has to be allowed first.
  // Can't be turned on here (Windows without Git, Codex on Windows): the row says why where Turn on would be.
  // Git can be installed, so Claude Code's row can look again; Codex on Windows has nothing to wait for.
  const blocked = st.installed_tool && !st.on ? st.unavailable : null;
  const needsSource = st.installed_tool && !st.on && !blocked && !allowed;
  const lead = blocked
    ? html`<span class="muted small">${blocked}</span>`
    : needsSource
      ? html`<span class="muted small">Allow ${name} in Sources first, so the app can read your report cards.</span>`
      : "";
  const lookAgain = html`<button class="ghost" data-action="report-cards-retry" data-id="look">Look again</button>`;
  // aria-disabled, not disabled: the button keeps keyboard focus through the redraws, and its label says what's happening.
  const button = !st.installed_tool
    ? lookAgain
    : blocked
      ? tool === "claude_code"
        ? lookAgain
        : ""
      : needsSource
        ? html`<button class="ghost" data-action="go-settings" data-id="sources">Open Sources</button>`
        : html`<button class="${st.on ? "ghost" : ""}" data-action="report-cards-tool" data-id="${tool}" data-on="${String(st.on)}" aria-label="${working ? doing : verb} report cards in ${name}"${attr(rc.busy !== null, 'aria-disabled="true"')}>${label}</button>`;
  const error = said("error", rc.error?.control === tool ? rc.error.text : (st.problem ?? undefined));
  const note = said("note", rc.note?.control === tool ? rc.note.text : undefined);
  return html`<div class="source${st.on ? " on" : ""}">
      <div class="src-main"><b>${name}</b>${toolState(tool, st, reading)}${lead}</div>
      <div class="src-actions">${button}</div>${error}${note}
    </div>`;
}

/** An app opened straight from the download runs from a temporary copy that is gone once it closes, and report cards need the real one. */
const MOVE_APP = "Move No, Actually... to your Applications folder and reopen it, so report cards keep working when the app is closed.";

function reportCardsView(rc: ReportCardsState, sources: SourceInfo[], tipRules: string): Html {
  const { status } = rc;
  const allowed = (tool: ReportCardTool) => sources.some((s) => s.id === TOOL_WORDS[tool].source && s.state?.allowed);
  // Reading means allowed and not paused; a stopped or paused source stops report cards too.
  const reading = (tool: ReportCardTool) => sources.some((s) => s.id === TOOL_WORDS[tool].source && s.state?.allowed && !s.state?.paused);
  const tools = status
    ? html`<div class="sources">${REPORT_CARD_TOOLS.map((t) => toolRow(t, status[t], rc, allowed(t), reading(t)))}</div>`
    : rc.statusError
      ? html`<p><span class="action-error">${rc.statusError}</span> <button class="ghost" data-action="report-cards-retry">Try again</button></p>`
      : html`<p class="muted" role="status">Checking Claude Code and Codex…</p>`;
  const err = (control: NonNullable<ReportCardsState["error"]>["control"]) => said("error", rc.error?.control === control ? rc.error.text : undefined);
  // Tips are Claude Code's, so turning them on waits for that to be on, and for any command to finish.
  // Turning them off never waits: a box that is ticked can always be unticked, whatever Claude Code is doing.
  const claudeOn = !!status?.claude_code.on;
  const tipsOn = !!status?.tips;
  const tipsLocked = !status || (!tipsOn && (!claudeOn || rc.busy !== null));
  const tipsHint =
    (!status || claudeOn
      ? ""
      : tipsOn
        ? " New chats won't get them until report cards are on in Claude Code."
        : " Turn on report cards in Claude Code first.") +
    (tipsOn && claudeOn && !reading("claude_code") ? " They're paused while No, Actually... isn't reading Claude Code." : "");
  const activity = status?.tip_activity;
  const tipsState =
    !tipsOn || !claudeOn || !activity
      ? ""
      : activity.blocked
        ? html`<p class="action-error">Recent chats aren't getting the tip instructions. Start a new chat. On a work computer, ask your admin whether plugin hooks are allowed.</p>`
        : activity.active
          ? html`<p class="muted small">Active in ${plural(activity.active, "chat")} this week${activity.tips > 0 ? ` · ${plural(activity.tips, "tip")} so far` : ""}.</p>`
          : "";
  const deleting = rc.busy === "delete";
  return html`
    <section id="report-cards">
      <div class="head"><p class="muted">Get a letter grade (A–F) on how you asked for each task, right inside Claude Code or Codex, with advice on getting it right the first time.</p></div>
      ${status?.translocated ? html`<p class="action-error" id="report-cards-move-app">${MOVE_APP}</p>` : ""}
      ${tools}
      <p class="muted how-to"><b>How to use it:</b> when you finish something, type <span class="mono">${TOOL_WORDS.claude_code.command}</span> in Claude Code (or <span class="mono">${TOOL_WORDS.codex.command}</span> in Codex).</p>
      <p class="muted small" id="report-cards-cost">Grades are your AI's judgment, so they can vary between runs, unlike your score. Claude Haiku misses more tips than larger models. Each report card uses your own tokens: roughly 0.5k to 2k output tokens per task graded.</p>
      <div class="settings-row">
        <label class="toggle"><input id="report-cards-tips" data-action="report-cards-tips" type="checkbox" aria-describedby="report-cards-tips-help"${attr(tipsOn, "checked")}${attr(tipsLocked, 'aria-disabled="true"')}/><span>Give me tips as I work (Claude Code only)</span></label>
        <span class="muted small" id="report-cards-tips-help">When a follow-up could have been in your first message, your AI adds a one-line tip, and No, Actually... keeps count. Adds about 480 tokens to each chat. Applies to new or reopened chats. To stop sooner, tell your AI “no more tips.”${tipsHint}</span>${err("tips")}
        <details class="tip-rules"><summary>See what your AI is told</summary><pre>${tipRules}</pre></details>
        ${tipsState}
      </div>
      <div class="settings-row">
        <label class="toggle"><input id="report-cards-reminders" data-action="report-cards-reminders" type="checkbox" aria-describedby="report-cards-reminders-help"${attr(status?.reminders ?? true, "checked")}${attr(!status, "disabled")}/><span>Remind me after a task (uses no tokens)</span></label>
        <span class="muted small" id="report-cards-reminders-help">A one-line note after a task, like “Want a report card on that?”</span>${err("reminders")}
      </div>
      <div class="settings-row">
        <button class="danger" data-action="report-cards-delete"${attr(rc.busy !== null, 'aria-disabled="true"')}>${deleting ? "Deleting…" : "Delete report card and tip history"}</button>${err("delete")}
        <span class="muted small" id="report-cards-note">${rc.note?.control === "delete" ? rc.note.text : ""}</span>
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
