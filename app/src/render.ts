// Draws the current view into #app, keeping keyboard focus where it was.
import { api, demo } from "./api";
import { captureFocus, must, restoreFocus } from "./dom";
import { ago, errorText, isFiltered, listNames } from "./format";
import { type Html, html } from "./html";
import { closePicker } from "./picker/popover";
import { createScheduler, isUserBusy } from "./scheduler";
import { isGlance, setAllModels, setCurrent, setLastVerdict, setMeta, state } from "./state";
import { updateBanner, updateLine } from "./update";
import { headerView, statusPillView } from "./views/chrome";
import { emptyGlanceView, glanceView } from "./views/glance";
import { onboardingView, scorecardView } from "./views/scorecard";
import { settingsView } from "./views/settings";
import { sourcesView, watchingNames } from "./views/sources";

export const root: HTMLElement = must("#app");

const watching = async () => listNames(watchingNames(await api.sources()));

async function view(): Promise<Html> {
  const meta = state.meta ?? setMeta(await api.meta());
  const s = await api.stats(state.filter);
  setCurrent(s);
  if (!isFiltered(state.filter)) setAllModels(s.models);
  if (isGlance) return s.messages ? glanceView(s, meta) : emptyGlanceView(await watching());

  let body: Html;
  if (state.tab === "sources") body = sourcesView(await api.sources());
  else if (state.tab === "settings") {
    const [retention, autostart] = await Promise.all([api.retention(), api.getAutostart().catch(() => false)]);
    body = settingsView({ retention, autostart, update: state.update, status: state.status, demo });
  } else if (s.messages) {
    body = scorecardView(s, { meta, filter: state.filter, still: s.verdict === state.lastVerdict });
    setLastVerdict(s.verdict);
  } else body = onboardingView(await watching());
  const banner = state.update ? updateBanner(state.update, state.laterFor) : "";
  return html`<div class="wrap">${headerView(state.tab, state.status, demo)}<div id="update-slot">${banner}</div>${body}</div>`;
}

/** Bar widths are set through CSSOM, not inline style attributes, so the CSP needs no 'unsafe-inline'. */
function applyBarWidths(el: ParentNode) {
  for (const bar of el.querySelectorAll<HTMLElement>("[data-w]")) bar.style.setProperty("--w", `${bar.dataset.w}%`);
}

async function draw(): Promise<void> {
  closePicker(false);
  const focus = captureFocus(root);
  let markup: Html;
  try {
    markup = await view();
  } catch (e) {
    markup = html`<div class="wrap"><section class="empty"><h1 class="q">Something went wrong.</h1><p class="muted mono small">${errorText(e)}</p><button data-action="retry">Try again</button></section></div>`;
  }
  root.innerHTML = markup.value;
  applyBarWidths(root);
  restoreFocus(root, focus);
}

export const scheduler = createScheduler({ isBusy: () => isUserBusy(root), draw });
export const render = scheduler.render;

/** Redraws only the sync status (pill and Settings line), so a check starting never interrupts anything. */
export function paintStatus(): void {
  const pill = root.querySelector(".sync-pill");
  if (pill) {
    const hadFocus = pill.contains(document.activeElement);
    pill.outerHTML = statusPillView(state.status, demo).value;
    if (hadFocus) root.querySelector<HTMLElement>(".sync-pill")?.focus();
  }
  const line = root.querySelector("#sync-line");
  if (line) line.textContent = `Last checked ${ago(state.status.last_sync)}.`;
}

/** Redraws only the update banner and the Settings line, so a check never interrupts anything. */
export function paintUpdate(): void {
  const u = state.update;
  if (!u) return;
  const slot = root.querySelector("#update-slot");
  if (slot) slot.innerHTML = updateBanner(u, state.laterFor).value;
  const line = root.querySelector("#update-line");
  if (line) line.textContent = updateLine(u);
  const box = root.querySelector<HTMLInputElement>("#auto-update");
  if (box && document.activeElement !== box) box.checked = u.auto;
}
