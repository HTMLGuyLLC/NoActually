// Draws the current view into #app, keeping keyboard focus where it was.
import { api, demo, REPORT_CARD_TOOLS } from "./api";
import { captureFocus, must, restoreFocus } from "./dom";
import { ago, errorText, isFiltered, listNames, subjectText } from "./format";
import { isDismissed } from "./hints";
import { type Html, html } from "./html";
import { refreshCheck } from "./learn/checker";
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
  setReportCards,
  state,
} from "./state";
import { updateBanner, updateLine } from "./update";
import { coverage, headerView, statusPillView } from "./views/chrome";
import { emptyGlanceView, glanceView } from "./views/glance";
import { canGrade, improveView } from "./views/improve";
import { learnView } from "./views/learn";
import { modelPicker, onboardingView, scorecardView } from "./views/scorecard";
import { settingsView } from "./views/settings";
import { watchingNames } from "./views/sources";

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
  const s = await api.stats(since ? { ...state.filter, since } : state.filter);
  setCurrent(s);
  // The picker's counts cover the same window as the page. A filtered page can't supply them, so a
  // changed window asks for the unfiltered counts.
  const scope = since ?? "";
  if (!isFiltered(state.filter)) setAllModels(s.models, scope);
  else if (state.allModelsScope !== scope) setAllModels((await api.stats(since ? { since } : {})).models, scope);
  if (isGlance) {
    if (!s.messages) return emptyGlanceView(await watching());
    // The goal is all-AI, like Improve's; the glance still draws if it can't be read.
    const goal = await api.progress(state.filter).then(
      (p) => p.goal,
      () => null,
    );
    return glanceView(s, meta, goal);
  }

  // Opening Settings → Report cards reads its status. That can take a moment (it asks Claude Code), so
  // the section draws first, with what it has, and again once the answer is in.
  const onCards = state.tab === "settings" && state.settingsPane === "report-cards";
  if (onCards && !reportCardsShown) void readReportCards().then(scheduler.refresh);
  reportCardsShown = onCards;

  let body: Html;
  if (state.tab === "settings") {
    // Sources lead Settings: you set them up once, beside the other privacy controls.
    const [sources, retention, autostart, menuBar] = await Promise.all([
      api.sources(),
      api.retention(),
      api.getAutostart().catch(() => false),
      api.getMenuBar(),
    ]);
    body = settingsView({
      pane: state.settingsPane,
      sources,
      status: state.status,
      retention,
      autostart,
      menuBar,
      reportCards: state.reportCards,
      tipRules: meta.tip_rules,
      update: state.update,
      demo,
    });
  } else if (state.tab === "learn") {
    body = learnView({ meta, prompts: s.messages ? s.prompts : null, done: await api.learned(), lesson: state.lesson, draft: state.draft, demo });
  } else if (state.tab === "improve") {
    // The recent list and the setup tip are extras: Improve still draws without them.
    const [progress, recent, sources] = await Promise.all([
      api.progress(state.filter),
      api.recentReportCards(state.filter).catch(() => []),
      api.sources().catch(() => []),
    ]);
    // Reading it asks the AI tools, which can take seconds, so it never holds up a draw.
    const cards = state.reportCards.status;
    if (!cards && !cardsAsked) {
      cardsAsked = true;
      void readReportCards().then(render);
    }
    body = improveView({
      meta,
      stats: s,
      progress,
      range: state.range,
      subject: subjectText(state.filter, meta),
      picker: modelPicker({ filter: state.filter, meta }),
      filtered: isFiltered(state.filter),
      recent,
      canGrade: canGrade(meta, sources),
      hintDismissed: isDismissed("report-cards"),
      cardsOn: REPORT_CARD_TOOLS.find((t) => cards?.[t].on) ?? null,
    });
  } else if (s.messages) {
    body = scorecardView(s, {
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
  } else body = onboardingView(await watching());
  const banner = state.update ? updateBanner(state.update, state.laterFor) : "";
  return html`<div class="wrap">${headerView(state.tab, state.status, demo, coverage(state.current))}<div id="update-slot">${banner}</div>${body}</div>`;
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
  if (state.tab === "learn" && !isGlance) await refreshCheck(root);
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
