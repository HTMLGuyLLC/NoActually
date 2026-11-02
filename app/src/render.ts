// Draws the current view into #app, keeping keyboard focus where it was.
import { api, demo, type Meta, REPORT_CARD_TOOLS, type Stats } from "./api";
import { captureFocus, must, restoreFocus, say } from "./dom";
import { ago, errorText, isFiltered, listNames, subjectText } from "./format";
import { isDismissed, rescoredUnseen } from "./hints";
import { type Html, html } from "./html";
import { refreshCheck } from "./learn/checker";
import { report } from "./log";
import { closePicker } from "./picker/popover";
import { createScheduler, isUserBusy } from "./scheduler";
import {
  isGlance,
  periodSince,
  setAllModels,
  setCurrent,
  setLastCorrectionTier,
  setLastPromptTier,
  setLastVerdict,
  setMeta,
  setReading,
  setReportCards,
  state,
} from "./state";
import { readingDoneNotice, rescoredNotice, updateBanner, updateLine } from "./update";
import { coverage, headerView, statusPillView, TAB_TITLES } from "./views/chrome";
import { emptyGlanceView, glanceErrorView, glanceRebuildingView, glanceView } from "./views/glance";
import { canGrade, improveView } from "./views/improve";
import { learnView } from "./views/learn";
import { modelPicker, noChatsView, onboardingView, rebuildingView, scorecardView, VIEW_TITLES } from "./views/scorecard";
import { PANE_TITLES, settingsView } from "./views/settings";
import { isImportOnly, watchingNames } from "./views/sources";

/** A read that is only an extra: a failure (or a missing answer) is the fallback, never an error page. */
const soft = async <T>(read: () => Promise<T>, fallback: T): Promise<T> => {
  try {
    return await read();
  } catch {
    return fallback;
  }
};

export const root: HTMLElement = must("#app");

const watching = async () => listNames(watchingNames(await api.sources()));

/**
 * Asks the app where report cards stand and keeps the answer. A failure is shown in the section, not thrown.
 * `look` also searches for a tool the quick search didn't find (the "Look again" button).
 */
export async function readReportCards(look = false): Promise<void> {
  try {
    setReportCards({ status: await api.reportCardsStatus(look), statusError: "" });
  } catch (e) {
    setReportCards({ statusError: errorText(e) });
  }
}

/** Whether Improve has already asked for the report card status once. */
let cardsAsked = false;

/** Whether the last draw showed Settings → Report cards, so opening it is told from redrawing it. */
let reportCardsShown = false;

async function view(): Promise<Html> {
  const meta = state.meta ?? setMeta(await api.meta());
  // Only the scorecard's counts follow its period; everything else reads all of history.
  const scorecard = !isGlance && state.tab === "score";
  const since = scorecard ? periodSince(state.period) : null;
  // While the counts are rebuilt they are partly empty: show a holding page, never half a score.
  if (state.status.rebuilding && (isGlance || state.tab === "score")) return isGlance ? glanceRebuildingView() : holding();
  const s = await api.stats(since ? { ...state.filter, since } : state.filter);
  setCurrent(s);
  // The picker's counts cover the same window as the page. A filtered page can't supply them, so a
  // changed window asks for the unfiltered counts.
  const scope = since ?? "";
  if (!isFiltered(state.filter)) setAllModels(s.models, scope);
  else if (state.allModelsScope !== scope) setAllModels((await api.stats(since ? { since } : {})).models, scope);
  if (isGlance) return glanceBody(s, meta);

  // Opening Settings → Report cards reads its status. That can take a moment (it asks Claude Code), so
  // the section draws first, with what it has, and again once the answer is in.
  const onCards = state.tab === "settings" && state.settingsPane === "report-cards";
  if (onCards && !reportCardsShown) void readReportCards().then(scheduler.refresh);
  reportCardsShown = onCards;

  if (state.tab === "settings") return page(await settingsBody(meta));
  if (state.tab === "learn") return page(await learnBody(meta, s));
  if (state.tab === "improve") return page(await improveBody(meta, s));
  return page(s.messages ? await scorecardBody(meta, s) : await emptyBody(meta));
}

/** The small window under the menu bar icon. */
async function glanceBody(s: Stats, meta: Meta): Promise<Html> {
  if (!s.messages) return emptyGlanceView(await watching(), meta.min_prompts);
  // The goal is all-AI, like Improve's; the glance still draws if it can't be read.
  const goal = await soft(async () => (await api.progress(state.filter)).goal, null);
  return glanceView(s, meta, goal, isImportOnly(await soft(() => api.sources(), [])));
}

async function settingsBody(meta: Meta): Promise<Html> {
  // Sources lead Settings: you set them up once, beside the other privacy controls.
  const [sources, retention, autostart, menuBar, popups] = await Promise.all([
    api.sources(),
    api.retention(),
    soft(() => api.getAutostart(), false),
    api.getMenuBar(),
    soft(() => api.getPopups(), true),
  ]);
  return settingsView({
    pane: state.settingsPane,
    sources,
    status: state.status,
    retention,
    autostart,
    menuBar,
    reportCards: state.reportCards,
    deleteAll: state.deleteAll,
    tipRules: meta.tip_rules,
    update: state.update,
    popups,
    demo,
  });
}

async function learnBody(meta: Meta, s: Stats): Promise<Html> {
  return learnView({ meta, prompts: s.messages ? s.prompts : null, done: await api.learned(), lesson: state.lesson, draft: state.draft, demo });
}

async function improveBody(meta: Meta, s: Stats): Promise<Html> {
  // The recent list and the setup tip are extras: Improve still draws without them.
  const [progress, recent, sources] = await Promise.all([
    api.progress(state.filter),
    soft(() => api.recentReportCards(state.filter), []),
    soft(() => api.sources(), []),
  ]);
  // Reading it asks the AI tools, which can take seconds, so it never holds up a draw.
  const cards = state.reportCards.status;
  if (!cards && !cardsAsked) {
    cardsAsked = true;
    void readReportCards().then(render);
  }
  return improveView({
    meta,
    stats: s,
    progress,
    range: state.range,
    subject: subjectText(state.filter, meta),
    picker: modelPicker({ filter: state.filter, meta }),
    filtered: isFiltered(state.filter),
    recent,
    canGrade: canGrade(meta, sources),
    noSource: sources.length > 0 && !sources.some((x) => x.state?.allowed),
    importOnly: isImportOnly(sources),
    hintDismissed: isDismissed("report-cards"),
    cardsOn: REPORT_CARD_TOOLS.find((t) => cards?.[t].on) ?? null,
  });
}

async function scorecardBody(meta: Meta, s: Stats): Promise<Html> {
  setReading("off");
  // Nothing new will come to someone who only imported an export: say to add a newer one.
  const importOnly = s.overall.to_rank > 0 && isImportOnly(await soft(() => api.sources(), []));
  const body = scorecardView(s, {
    importOnly,
    meta,
    filter: state.filter,
    view: state.view,
    period: state.period,
    still: s.overall.verdict === state.lastVerdict,
    promptsStill: s.prompts.tier === state.lastPromptTier,
    correctionsStill: s.overall.correction_tier === state.lastCorrectionTier,
    awardSlide: state.awardSlide,
  });
  setLastVerdict(s.overall.verdict);
  setLastPromptTier(s.prompts.tier);
  setLastCorrectionTier(s.overall.correction_tier);
  return body;
}

/** The scorecard with no chats in this period or for this model: widen it if there is history, else onboarding. */
async function emptyBody(meta: Meta): Promise<Html> {
  const all = await api.stats({ since: periodSince("all") });
  // Nothing in this period or for this model, but there is history: keep the pickers so it can be widened.
  if (all.messages) return noChatsView({ filter: state.filter, meta, period: state.period, view: state.view, lastDay: all.last_day });
  return onboardingView(await watching(), state.reading === "reading" || state.status.syncing);
}

/** The holding page for the scorecard, with the frame around it. */
function holding(): Html {
  return html`<div class="wrap">${headerView(state.tab, state.status, demo)}<main id="main" tabindex="-1">${rebuildingView()}</main></div>`;
}

/** The frame around a page: skip link, header, the update and notice lines, and the page itself in `main`. */
async function page(body: Html): Promise<Html> {
  const banner = state.update ? updateBanner(state.update, state.laterFor) : "";
  const stamp = await soft(() => api.rescoredNotice(), null);
  const notice = state.reading === "done" && state.tab !== "score" ? readingDoneNotice() : rescoredUnseen(stamp) && stamp ? rescoredNotice(stamp) : "";
  return html`<div class="wrap"><button class="skip-link" data-action="skip-to-main">Skip to content</button>${headerView(state.tab, state.status, demo, coverage(state.current))}<div id="update-slot">${banner}</div>${notice ? html`<div id="notice-slot">${notice}</div>` : ""}<main id="main" tabindex="-1">${body}</main></div>`;
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
    report(e);
    // The glance has no header or log folder to offer: its own small page, with a retry and the full app.
    markup = isGlance
      ? glanceErrorView(errorText(e))
      : html`<div class="wrap">${headerView(state.tab, state.status, demo, coverage(state.current))}<main id="main" tabindex="-1"><section class="empty"><h1 class="q">Something went wrong.</h1><p class="muted mono small">${errorText(e)}</p><div class="src-actions"><button data-action="retry">Try again</button>${demo ? "" : html`<button class="ghost" data-action="open-log-folder">Show log folder</button>`}</div></section></main></div>`;
  }
  root.innerHTML = markup.value;
  applyBarWidths(root);
  restoreFocus(root, focus);
  announcePage();
  if (state.tab === "learn" && !isGlance) await refreshCheck(root);
}

/** The page last announced, so a background redraw says nothing and only a move to another page does. */
let announced = "";

/** Says the page's name in a live region when it changes (the tabs swap content in place, which a screen reader can't see). */
function announcePage(): void {
  if (isGlance) return;
  const name =
    state.tab === "score"
      ? `Scorecard, ${VIEW_TITLES[state.view]}`
      : state.tab === "settings"
        ? `Settings, ${PANE_TITLES[state.settingsPane]}`
        : TAB_TITLES[state.tab];
  if (name === announced) return;
  const first = announced === "";
  announced = name;
  // Not at launch: the window's own title is read then.
  if (!first) say(document.getElementById("announce"), name);
}

export const scheduler = createScheduler({ isBusy: () => isUserBusy(root), draw });
export const render = scheduler.render;

/** What can hold focus in the sync pill. */
const PILL_CONTROLS = [".sync-now", ".sync-bad", ".sync-text"];

/** Redraws only the sync status (pill and Settings line), so a check starting never interrupts anything. */
export function paintStatus(): void {
  const pill = root.querySelector(".sync-pill");
  if (pill) {
    // The pill itself isn't focusable: put focus back on the same kind of control inside the new one.
    const active = document.activeElement;
    const hadFocus = pill.contains(active);
    const kind = PILL_CONTROLS.find((c) => active?.matches(c));
    const markup = statusPillView(state.status, demo, coverage(state.current)).value;
    // Compared as the browser would write it, since it doesn't give back the markup it was given.
    const next = document.createElement("template");
    next.innerHTML = markup;
    // Nothing changed: leave it, so an open tooltip and the focus aren't disturbed.
    if (next.content.firstElementChild?.outerHTML !== pill.outerHTML) {
      const open = pill.querySelector(".sync-text")?.getAttribute("aria-expanded") === "true";
      pill.outerHTML = markup;
      if (open) root.querySelector(".sync-text")?.setAttribute("aria-expanded", "true");
    }
    if (hadFocus) {
      const next = root.querySelector(".sync-pill");
      const same = kind ? next?.querySelector<HTMLElement>(kind) : null;
      (same ?? next?.querySelector<HTMLElement>("button, [tabindex]"))?.focus();
    }
  }
  const line = root.querySelector("#sync-line");
  if (line) line.textContent = `New chats last checked ${ago(state.status.last_sync)}.`;
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
  const channel = root.querySelector<HTMLSelectElement>("#update-channel");
  if (channel && document.activeElement !== channel) channel.value = u.channel;
}
