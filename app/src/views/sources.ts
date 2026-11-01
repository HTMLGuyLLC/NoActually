// Sources: which tools the app may read, and the controls to allow, pause, import or forget them.
import type { SourceInfo, SyncStatus } from "../api";
import { ago, localIsoDate, shortSourceName } from "../format";
import { type Html, html } from "../html";

/** The "how far back" choices offered before allowing a live source. */
export const SINCE_CHOICES = [
  ["all", "All history"],
  ["30", "Last 30 days"],
  ["7", "Last 7 days"],
  ["none", "From now on"],
] as const;

/** Turns a "how far back" choice into `allow_source` arguments: a local start day, or "from now on". */
export function allowArgs(choice: string, now: Date): { since: string | null; fromNow: boolean } {
  if (choice === "none") return { since: null, fromNow: true };
  const days = Number(choice);
  if (choice === "all" || !Number.isFinite(days) || days <= 0) return { since: null, fromNow: false };
  const start = new Date(now);
  start.setDate(start.getDate() - days);
  return { since: localIsoDate(start), fromNow: false };
}

/** Allowed, unpaused live sources: what the app is reading right now. A file you imported is read once, not watched. */
export const watchingNames = (list: SourceInfo[]): string[] =>
  list.filter((x) => x.kind !== "import" && x.state?.allowed && !x.state.paused).map((x) => shortSourceName(x.name));

function sourceRow(s: SourceInfo): Html {
  const st = s.state;
  // Every source repeats the same buttons, so each says whose it is.
  const who = (verb: string) => `${verb} ${s.name}`;
  const found = s.roots.filter(([, ok]) => ok).map(([p]) => p);
  const where =
    s.kind === "import" ? (st?.paths.length ? st.paths.join(", ") : "Pick your export file") : found.length ? found.join(", ") : "Not found on this computer";
  const label = st?.allowed ? (st.paused ? "Paused" : "Allowed") : "Not allowed";
  const actions =
    s.kind === "import"
      ? html`<button data-action="import" data-id="${s.id}" aria-label="${st?.paths.length ? who("Add another export for") : who("Import export for")}">${st?.paths.length ? "Add another export" : "Import export…"}</button>`
      : st?.allowed
        ? html`<button class="ghost" data-action="pause" data-id="${s.id}" data-paused="${String(st.paused)}" aria-label="${who(st.paused ? "Resume" : "Pause")}">${st.paused ? "Resume" : "Pause"}</button><button class="ghost" data-action="revoke" data-id="${s.id}" aria-label="${who("Stop reading")}">Stop reading</button>`
        : found.length
          ? html`<select data-since="${s.id}" aria-label="How far back to read ${s.name}">${SINCE_CHOICES.map(
              ([v, text]) => html`<option value="${v}">${text}</option>`,
            )}</select><button data-action="allow" data-id="${s.id}" aria-label="${who("Allow")}">Allow</button>`
          : "";
  const hint =
    s.kind === "import"
      ? s.id === "chatgpt-export"
        ? "Get it in ChatGPT: Settings → Data controls → Export data. It arrives as a zip by email. Unzip it, then choose conversations.json. If it has several, add each one."
        : "Get it on claude.ai: Settings → Privacy → Export data. It arrives as a zip by email. Unzip it, then choose conversations.json. If it has several, add each one."
      : "";
  return html`<div class="source${st?.allowed ? " on" : ""}">
      <div class="src-main"><b>${s.name}</b><span class="status">${label}</span>
        <p class="muted small mono">${where}</p>${hint ? html`<p class="muted small">${hint}</p>` : ""}</div>
      <div class="src-actions">${actions}${st ? html`<button class="danger" data-action="forget" data-id="${s.id}" aria-label="${who("Forget")}">Forget</button>` : ""}</div>
    </div>`;
}

/** `status` dates the last check for new chats; Check for new chats runs one now. */
export function sourcesView(list: SourceInfo[], status: SyncStatus): Html {
  return html`
    <section id="sources"><div class="head sources-head">
      <p class="muted">Nothing is read until you allow it. No, Actually... reads your messages and the AI's replies on this computer to score your prompts and count corrections. The app itself sends no chat content or stats. If you turn on report cards, your AI reads a short summary of that chat.</p>
      <div class="sync-check">
        <button class="ghost" data-action="sync-now">Check for new chats</button>
        <span class="muted small" id="sync-line">New chats last checked ${ago(status.last_sync)}.</span>
      </div></div>
      <div class="sources">${list.map(sourceRow)}</div>
      <p class="muted small">Stop reading keeps your scores. Forget also deletes what that source produced, pins included. Finished goal weeks and trophies stay, as totals.</p>
    </section>`;
}
