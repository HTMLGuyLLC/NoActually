// What every control does. Views mark controls with data-action="…" (and data-id for the thing
// acted on); one delegated listener per event type on the root dispatches through the maps below,
// so nothing is re-bound on each redraw.
import { api, type GoalKind, type HallEntry, isLevel, type Meta, type PromptTier, REPORT_CARD_TOOLS, type ReportCardTool, type Stats } from "./api";
import { data, say } from "./dom";
import { errorText, filterLabel, plural } from "./format";
import { dismiss, dismissRescored } from "./hints";
import { scheduleCheck } from "./learn/checker";
import { lessonById } from "./learn/lessons";
import { report } from "./log";
import { correctionTier, promptTier, tierName } from "./meta";
import type { PickStat } from "./picker/picker";
import { buildPickItems, sameFilter } from "./picker/picker";
import { closePicker, openPicker, pickerOpen } from "./picker/popover";
import { paintUpdate, readReportCards, render, scheduler } from "./render";
import { confirmTurnOn, doneLine, offLine } from "./report-cards";
import { drawCorrectionCard, drawCorrectionsCard, drawPromptCard, drawShareCard } from "./share/cards";
import { openShareDialog, type Post } from "./share/dialog";
import { correctionShareText, correctionsShareText, promptShareText, SHARE_URL, shareText } from "./share/share";
import {
  isPeriod,
  isRange,
  isScoreView,
  isSettingsPane,
  isTab,
  PERIOD_WHEN,
  type ReportCardsBusy,
  type ReportCardsState,
  setAutoUpdate,
  setAwardSlide,
  setDeleteAll,
  setDraft,
  setFilter,
  setLaterFor,
  setLesson,
  setPeriod,
  setRange,
  setReading,
  setReportCards,
  setSettingsPane,
  setTab,
  setUpdateChannel,
  setUpdateError,
  setView,
  state,
} from "./state";
import { judgeQuiz } from "./views/learn";
import { goneByCount, parseRetention, tightens, usesFullDays, usesSnippetDays } from "./views/settings";
import { allowArgs } from "./views/sources";

/** Removes the note a failed action left in `slot`. */
const clearError = (slot: Element | null) => slot?.querySelector(":scope > .action-error")?.remove();

/** Shows why something failed, in `slot`, as an alert. */
function showError(slot: Element | null, e: unknown): void {
  clearError(slot);
  const err = document.createElement("span");
  err.className = "action-error";
  err.setAttribute("role", "alert");
  err.textContent = `Couldn't do that: ${errorText(e)}`;
  slot?.append(err);
}

/**
 * Runs a button's async work: disables it (optionally relabelled, "Allowing…"), and always puts it
 * back. A failure shows inline next to the button instead of vanishing. Resolves true on success.
 */
export async function run(btn: HTMLButtonElement, work: () => Promise<unknown>, pending?: string): Promise<boolean> {
  const label = btn.textContent;
  clearError(btn.parentElement);
  btn.disabled = true;
  if (pending) btn.textContent = pending;
  try {
    await work();
    return true;
  } catch (e) {
    showError(btn.parentElement, e);
    return false;
  } finally {
    btn.disabled = false;
    btn.textContent = label;
  }
}

/** Runs `work` for the button, then redraws on success. */
const thenRender = async (btn: HTMLButtonElement, work: () => Promise<unknown>, pending?: string) => {
  if (await run(btn, work, pending)) await render();
};

const scorePost = (s: Stats, meta: Meta): Post => {
  const who = filterLabel(state.filter, meta);
  const tier = tierName(meta, s.overall.tier);
  const w = when();
  return { title: "Share your score", text: shareText(s, who, tier, w), image: (shape) => drawShareCard(s, who, tier, shape, w) };
};

const promptsPost = (s: Stats, tier: PromptTier): Post => ({
  title: "Share your prompts",
  text: promptShareText(s.prompts, tier, when()),
  image: (shape) => drawPromptCard(s.prompts, tier, shape, when()),
});

const correctionsPost = (s: Stats, tier: PromptTier): Post => ({
  title: "Share your corrections",
  text: correctionsShareText(s.overall, tier, when()),
  image: (shape) => drawCorrectionsCard(s.overall, tier, shape, when()),
});

const correctionPost = (e: HallEntry): Post => ({
  title: "Share this correction",
  text: correctionShareText(e),
  image: (shape, edited) => drawCorrectionCard(edited ?? e, shape),
  ...(e.human || e.ai ? { quotes: e } : {}),
});

/** The period the drawn numbers cover: the scorecard's picker, or the default 28 days elsewhere. */
const when = () => PERIOD_WHEN[state.tab === "score" ? state.period : "28d"];

/** How long a first click stays armed before the button goes back to normal, unless focus is still on it (a screen reader needs the time). */
const CONFIRM_MS = 5000;
/** The longest a confirm stays armed, even under keyboard focus. Redraws wait on it. */
const CONFIRM_CAP_MS = 30_000;
/** A second click sooner than this is a double-click, not a decision: it does nothing. */
const CONFIRM_GUARD_MS = 400;
const armed = new WeakMap<HTMLButtonElement, (flush: boolean) => void>();
const armedAt = new WeakMap<HTMLButtonElement, number>();

/** A hidden status line that reads the "Click again…" prompt aloud; the button's own text change isn't announced. */
function confirmNote(): HTMLElement {
  let el = document.getElementById("confirm-note");
  if (!el) {
    el = document.createElement("div");
    el.id = "confirm-note";
    el.className = "sr-only";
    el.setAttribute("role", "status");
    document.body.appendChild(el);
  }
  return el;
}

/** True on the second click. The first relabels the button and holds off redraws until then; it disarms
 * itself after a few seconds (longer under keyboard focus), or when focus leaves or Escape is pressed, restoring the label. A second click within
 * `CONFIRM_GUARD_MS` of the first (a double-click) is ignored. */
function confirmed(b: HTMLButtonElement, ask: string): boolean {
  const disarm = armed.get(b);
  if (disarm) {
    if (Date.now() - (armedAt.get(b) ?? 0) < CONFIRM_GUARD_MS) return false;
    disarm(false);
    return true;
  }
  const label = b.textContent;
  const note = confirmNote();
  const off = (flush: boolean) => {
    clearTimeout(timer);
    clearTimeout(announce);
    b.removeEventListener("blur", onBlur);
    b.removeEventListener("keydown", onKey);
    armed.delete(b);
    delete b.dataset.confirm;
    b.textContent = label;
    say(note, "");
    if (flush) scheduler.flush(); // apply any redraw that waited on the armed button
  };
  const onBlur = () => off(true);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") off(true);
  };
  // Keyboard focus keeps it armed so a screen reader can finish, up to a cap; a mouse click doesn't.
  const expire = () => {
    const reading = b.matches(":focus-visible") && Date.now() - (armedAt.get(b) ?? 0) < CONFIRM_CAP_MS;
    if (reading) timer = setTimeout(expire, CONFIRM_MS);
    else off(true);
  };
  let timer = setTimeout(expire, CONFIRM_MS);
  // Set after the region exists for a tick, so a screen reader notices the change.
  const announce = setTimeout(() => say(note, ask), 0);
  b.addEventListener("blur", onBlur);
  b.addEventListener("keydown", onKey);
  armed.set(b, off);
  armedAt.set(b, Date.now());
  b.dataset.confirm = "yes";
  b.textContent = ask;
  return false;
}

/** The form for a correction that was counted wrong. */
export const COUNTED_WRONG_URL = "https://github.com/HTMLGuyLLC/NoActually/issues/new?template=false_positive.yml";

/** What changed in each version, including rules updates that recount history. */
export const CHANGELOG_URL = `${SHARE_URL}/blob/main/CHANGELOG.md`;

/** A report cards tool named by a button, checked before it's sent to the app. */
function reportCardTool(id: string): ReportCardTool {
  const tool = REPORT_CARD_TOOLS.find((t) => t === id);
  if (!tool) throw new Error(`Unknown tool: ${id}`);
  return tool;
}

/**
 * Runs one report cards command with its button showing it's working, then reads where things stand again
 * (whether or not it worked) before the controls come back. A failure shows beside the control, in the app's words;
 * so does what `said` makes of a success. Either is announced once, here, and not on every redraw.
 */
async function reportCardsDo<T>(control: ReportCardsBusy, work: () => Promise<T>, said: (result: T) => string): Promise<void> {
  setReportCards({ busy: control, error: null, note: null });
  await render();
  let text = "";
  let error: ReportCardsState["error"] = null;
  try {
    text = said(await work());
  } catch (e) {
    error = { control, text: errorText(e) };
  }
  await readReportCards();
  setReportCards({ busy: null, error, note: text ? { control, text } : null });
  await render();
  say(confirmNote(), error?.text ?? text);
}

/** The data the scorecard was drawn from; its buttons only exist once both are loaded. */
function drawn(): { s: Stats; meta: Meta } {
  const { current: s, meta } = state;
  if (!s || !meta) throw new Error("Nothing drawn yet");
  return { s, meta };
}

/** The picker shows the numbers the page is about. */
export function pickStatFor(tab: typeof state.tab, view: typeof state.view): PickStat {
  if (tab !== "score" || view === "overview") return "both";
  return view === "prompts" ? "prompts" : "corrections";
}

function togglePicker(trigger: HTMLButtonElement) {
  if (pickerOpen()) return closePicker();
  openPicker(trigger, {
    items: buildPickItems(state.allModels),
    stat: pickStatFor(state.tab, state.view),
    isSelected: (i) => sameFilter(i.filter, state.filter),
    onChoose: (i) => {
      setFilter(i.filter);
      void render();
    },
    onClose: (flush) => {
      if (flush) scheduler.flush(); // apply any update that arrived while the picker was open
    },
  });
}

type Handler = (el: HTMLButtonElement) => unknown;

export const clickActions = {
  "open-share": (b) => {
    const { s, meta } = drawn();
    openShareDialog(scorePost(s, meta), b, scheduler.flush);
  },
  "share-prompts": (b) => {
    const { s, meta } = drawn();
    const tier = promptTier(meta, s.prompts.tier);
    if (tier) openShareDialog(promptsPost(s, tier), b, scheduler.flush);
  },
  "share-corrections": (b) => {
    const { s, meta } = drawn();
    const tier = correctionTier(meta, s.overall.correction_tier);
    if (tier) openShareDialog(correctionsPost(s, tier), b, scheduler.flush);
  },
  "share-correction": (b) => {
    const id = Number(data(b, "id"));
    const e = drawn().s.hall_of_fame.find((h) => h.id === id);
    if (e) openShareDialog(correctionPost(e), b, scheduler.flush);
  },
  "open-main": (b) => {
    const tab = b.dataset.id;
    return api.openMain(isTab(tab) ? tab : null);
  },
  home: () => {
    setTab("score");
    setView("overview");
    setReading("off");
    return render();
  },
  "open-changelog": (b) => run(b, () => api.openShare(CHANGELOG_URL)),
  // The "rescored" line is gone for good once dismissed; only a later rebuild brings it back.
  "dismiss-rescored": (b) => {
    dismissRescored(data(b, "id"));
    return render();
  },
  // The skip link: focus the page's content, past the header. (An anchor would change the hash, which reloads the window.)
  "skip-to-main": () => {
    const main = document.getElementById("main");
    main?.focus();
    main?.scrollIntoView?.({ block: "start" });
  },
  "open-notices": (b) => run(b, () => api.openNotices()),
  // Opens a Settings section from another tab (Improve's "Set up", the scorecard's "Choose sources").
  "go-settings": (b) => {
    const pane = data(b, "id");
    if (!isSettingsPane(pane)) throw new Error(`Unknown settings section: ${pane}`);
    setTab("settings");
    setSettingsPane(pane);
    return render().then(() => window.scrollTo({ top: 0 }));
  },
  // The tip is gone, so focus goes to the heading above where it was.
  "dismiss-card-hint": async () => {
    dismiss("report-cards");
    await render();
    const heading = document.querySelector<HTMLElement>("#trends-heading");
    if (heading) {
      heading.tabIndex = -1;
      heading.focus();
    }
  },
  "settings-pane": (b) => {
    const pane = data(b, "id");
    if (!isSettingsPane(pane)) throw new Error(`Unknown settings section: ${pane}`);
    setSettingsPane(pane);
    return render();
  },
  tab: (b) => {
    const tab = data(b, "id");
    if (!isTab(tab)) throw new Error(`Unknown tab: ${tab}`);
    setTab(tab);
    return render();
  },
  "model-picker": (b) => togglePicker(b),
  view: (b) => {
    const view = data(b, "id");
    if (!isScoreView(view)) throw new Error(`Unknown view: ${view}`);
    setView(view);
    return render().then(() => window.scrollTo({ top: 0 }));
  },
  // From "No chats in this period": back to all time, and all AI when a model was picked.
  widen: () => {
    setPeriod("all");
    setFilter({});
    return render();
  },
  period: (b) => {
    const p = data(b, "id");
    if (!isPeriod(p)) throw new Error(`Unknown period: ${p}`);
    setPeriod(p);
    return render();
  },
  range: (b) => {
    const r = data(b, "id");
    if (!isRange(r)) throw new Error(`Unknown range: ${r}`);
    setRange(r);
    return render();
  },
  lesson: (b) => {
    const id = data(b, "id");
    setLesson(lessonById(id) ? id : null);
    return render().then(() => window.scrollTo({ top: 0 }));
  },
  "open-lesson": (b) => {
    const id = data(b, "id");
    setTab("learn");
    setLesson(lessonById(id) ? id : null);
    return render().then(() => window.scrollTo({ top: 0 }));
  },
  // Flips an award's entry in place: no redraw, so the page doesn't move.
  "award-slide": (b) => {
    const award = b.closest<HTMLElement>("[data-award]");
    const slides = award ? [...award.querySelectorAll<HTMLElement>(".slide")] : [];
    if (!award || slides.length < 2) return;
    const now = slides.findIndex((el) => !el.hasAttribute("aria-hidden"));
    const next = (now + Number(data(b, "step")) + slides.length) % slides.length;
    slides.forEach((el, n) => {
      // The same markup the view draws: aria-hidden="true" plus inert on every slide but one.
      if (n === next) el.removeAttribute("aria-hidden");
      else el.setAttribute("aria-hidden", "true");
      el.toggleAttribute("inert", n !== next);
    });
    const at = award.querySelector(".slide-at");
    if (at) at.textContent = String(next + 1);
    setAwardSlide(data(b, "id"), Number(slides[next]?.dataset.entry));
  },
  "lesson-done": (b) =>
    thenRender(b, async () => {
      const done = data(b, "done") === "true";
      await api.setLearned(data(b, "id"), done);
      // Finishing a lesson goes back to the list, where the track's progress shows it.
      if (done) setLesson(null);
    }),
  "try-template": (b) => {
    const l = lessonById(data(b, "id"));
    if (!l) return;
    setDraft(l.template);
    setLesson(null);
    return render().then(() => {
      const box = document.querySelector<HTMLTextAreaElement>("#draft");
      box?.focus();
      box?.scrollIntoView({ block: "center" });
    });
  },
  "copy-draft": async (b) => {
    const ok = await navigator.clipboard.writeText(state.draft).then(
      () => true,
      () => false,
    );
    say(b.parentElement?.querySelector("#draft-status") ?? null, ok ? "Copied your prompt." : "Couldn't copy.");
  },
  "clear-draft": () => {
    setDraft("");
    const box = document.querySelector<HTMLTextAreaElement>("#draft");
    if (box) {
      box.value = "";
      box.focus();
    }
    scheduleCheck(document, 0);
  },
  // Switching or dropping a goal loses this week's progress on it, so each takes a second click.
  "start-goal": (b) => {
    if (b.dataset.replaces === "yes" && !confirmed(b, "Click again to switch")) return;
    const raw = b.dataset.goal ?? b.closest(".goal-pick")?.querySelector<HTMLSelectElement>("#goal-pick")?.value ?? "";
    const g = parseGoalChoice(raw);
    if (!g) throw new Error("Pick a goal first");
    return thenRender(b, () => api.setGoal(g.kind, g.target));
  },
  "clear-goal": (b) => {
    if (!confirmed(b, "Click again to drop this goal and its progress for good")) return;
    return thenRender(b, () => api.clearGoal());
  },
  pin: (b) => {
    const id = Number(data(b, "id"));
    // Pinning moves the entry to the front of its award; stay on it rather than on its old spot.
    const award = b.closest<HTMLElement>("[data-award]")?.dataset.award;
    if (award != null) setAwardSlide(award, id);
    return thenRender(b, () => api.pin(id, data(b, "pinned") !== "true"));
  },
  allow: (b) => {
    const id = data(b, "id");
    const choice = b.parentElement?.querySelector<HTMLSelectElement>(`select[data-since="${CSS.escape(id)}"]`)?.value ?? "all";
    const { since, fromNow } = allowArgs(choice, new Date());
    // The first Allow with a history to read goes to the scorecard, which says it is reading and then fills in.
    const first = !fromNow && !state.current?.messages;
    return thenRender(
      b,
      async () => {
        await api.allow(id, since, fromNow);
        if (!first) return;
        setReading("reading");
        setTab("score");
        setView("overview");
      },
      "Allowing…",
    );
  },
  revoke: (b) => thenRender(b, () => api.revoke(data(b, "id"))),
  pause: (b) => thenRender(b, () => api.pause(data(b, "id"), data(b, "paused") !== "true")),
  import: (b) => {
    // Like the first Allow: the first import with no history before it goes to the scorecard. A cancelled picker returns null and changes nothing.
    const first = !state.current?.messages;
    return thenRender(b, async () => {
      const picked = await api.importExport(data(b, "id"));
      if (picked === null || !first) return;
      setReading("reading");
      setTab("score");
      setView("overview");
    });
  },
  // Forget deletes data for good, so it takes a second click to confirm, and says what goes.
  forget: (b) => {
    if (!confirmed(b, "Click again to delete its scores, pins and report cards for good")) return;
    return thenRender(b, () => api.forget(data(b, "id")));
  },
  // Turn on asks first; Turn off just does it (History stays until deleted).
  "report-cards-tool": async (b) => {
    const tool = reportCardTool(data(b, "id"));
    if (state.reportCards.busy !== null) return;
    if (data(b, "on") === "true") {
      // Read before the turn-off: what the status says afterwards no longer has the tips on.
      const tipsWereOn = !!state.reportCards.status?.tips;
      return reportCardsDo(
        tool,
        () => api.reportCardsTurnOff(tool),
        () => offLine(tool, tipsWereOn),
      );
    }
    if (!(await confirmTurnOn(tool, b, scheduler.flush))) return;
    return reportCardsDo(
      tool,
      () => api.reportCardsTurnOn(tool),
      () => doneLine(tool),
    );
  },
  // After a failed check, or "Look again" for a tool that wasn't found (which may take a few seconds).
  "report-cards-retry": async (b) => {
    if (b.dataset.id === "look") {
      await run(b, () => readReportCards(true), "Looking…");
    } else {
      setReportCards({ statusError: "" });
      await render(); // "Checking…"
      await readReportCards();
    }
    await render();
  },
  // Deleting takes a second click, like Forget.
  "report-cards-delete": (b) => {
    if (state.reportCards.busy !== null || !confirmed(b, "Click again to delete every report card and tip for good")) return;
    return reportCardsDo(
      "delete",
      () => api.reportCardsDeleteHistory(),
      (n) => (n ? "Deleted your report card and tip history." : "There was no history to delete."),
    );
  },
  // On success the app quits, so the button stays on "Deleting…". A failure is kept in the state, because a redraw may replace the button meanwhile.
  "delete-everything": async (b) => {
    if (state.deleteAll.busy || !confirmed(b, "Click again to delete everything. This can't be undone")) return;
    setDeleteAll({ busy: true, error: "" });
    await render();
    try {
      await api.deleteEverything();
    } catch (e) {
      setDeleteAll({ busy: false, error: errorText(e) });
      await render();
      say(confirmNote(), errorText(e));
    }
  },
  "save-privacy": async (b) => {
    const form = b.closest("section") ?? document;
    const out = form.querySelector("#privacy-status");
    const value = (sel: string) => form.querySelector<HTMLInputElement>(sel)?.value ?? "";
    try {
      const typed = {
        level: form.querySelector<HTMLInputElement>('input[name="level"]:checked')?.value ?? null,
        fullDays: value("#full-days"),
        snippetDays: value("#snippet-days"),
      };
      const retention = parseRetention(typed);
      const saved = await api.retention();
      const cuts = tightens(saved, retention);
      // Keeping less deletes older text for good: say how much, and ask once more.
      if (cuts) {
        const n = goneByCount(state.current, retention);
        if (!confirmed(b, `Click again to delete older text${n === null ? "" : ` (${plural(n, "correction")} in your Hall of fame)`} for good`)) return;
      }
      await api.setRetention(retention);
      // Draw the values as saved (days are kept between 1 and 3650), then say what happened.
      await render();
      say(document.querySelector("#privacy-status"), cuts ? "Saved. Older text was trimmed to match." : "Saved.");
    } catch (e) {
      say(out, errorText(e));
    }
  },
  "check-update": (b) => run(b, () => api.checkForUpdates()),
  // The pill's button holds an icon, which `run` would wipe, so this shows its failure by hand.
  "sync-now": async (b) => {
    clearError(b.parentElement);
    try {
      await api.syncNow();
    } catch (e) {
      showError(b.parentElement, e);
      say(confirmNote(), `Couldn't check for new chats: ${errorText(e)}`);
    }
  },
  // Opens the false-positive form in the browser (github.com is on the app's allowed-link list).
  "counted-wrong": (b) => run(b, () => api.openShare(COUNTED_WRONG_URL)),
  // Opens or closes the tooltip for touch and keyboard. Closing happens in `bindActions` (blur, Escape, a click elsewhere).
  "sync-tip": (b) => b.setAttribute("aria-expanded", String(b.getAttribute("aria-expanded") !== "true")),
  "open-log-folder": (b) => run(b, () => api.openLogFolder()),
  // A failure is kept in the state and drawn in the banner, which a redraw may have replaced since the click.
  "install-update": async (b) => {
    let failure: unknown = null;
    const ok = await run(b, async () => {
      try {
        await api.installUpdate();
      } catch (e) {
        failure = e;
        throw e;
      }
    });
    if (ok) return;
    setUpdateError(errorText(failure));
    paintUpdate();
  },
  "later-update": () => {
    setLaterFor(state.update?.available ?? null);
    paintUpdate();
  },
  retry: () => render(),
} satisfies Record<string, Handler>;

/** A goal choice from the Improve form, checked before it's sent. */
export function parseGoalChoice(raw: string): { kind: GoalKind; target: number } | null {
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof v !== "object" || v === null) return null;
  const o = v as Record<string, unknown>;
  const target = typeof o.target === "number" ? o.target : Number.NaN;
  if (!Number.isFinite(target)) return null;
  if (o.kind === "habit" && typeof o.signal === "string") return { kind: { kind: "habit", signal: o.signal }, target };
  if (o.kind === "steer" && typeof o.steer === "string") return { kind: { kind: "steer", steer: o.steer }, target };
  if (o.kind === "tier" && typeof o.tier === "string") return { kind: { kind: "tier", tier: o.tier }, target };
  return null;
}

type InputHandler = (el: HTMLInputElement | HTMLSelectElement) => unknown;

/** The row a Settings control sits in, where a refusal is shown. */
const rowOf = (el: HTMLElement) => el.closest(".settings-row") ?? el.parentElement;

/** Undoes a checkbox the backend refused, so it never shows a setting that isn't saved, and says why. */
const toggle = (save: (on: boolean) => Promise<unknown>, saved?: (on: boolean) => void) => async (box: HTMLInputElement | HTMLSelectElement) => {
  if (!(box instanceof HTMLInputElement)) return;
  clearError(rowOf(box));
  try {
    await save(box.checked);
    saved?.(box.checked);
  } catch (e) {
    box.checked = !box.checked;
    showError(rowOf(box), e);
  }
};

/** Saves one of the report cards checkboxes. The box is ticked already; if the app refuses, the redraw puts it back and the reason is announced. */
async function saveCardsSetting(box: HTMLInputElement | HTMLSelectElement, key: "tips" | "reminders", save: (on: boolean) => Promise<unknown>): Promise<void> {
  if (!(box instanceof HTMLInputElement)) return;
  let error: ReportCardsState["error"] = null;
  try {
    await save(box.checked);
  } catch (e) {
    error = { control: key, text: errorText(e) };
  }
  // Read the status after the save: a Turn on or off can finish meanwhile, and its newer status wins.
  const { status } = state.reportCards;
  setReportCards({ error, status: status && !error ? { ...status, [key]: box.checked } : status });
  // Whether tips are reaching your chats depends on when they went on.
  if (key === "tips" && !error) await readReportCards();
  await render();
  if (error) say(confirmNote(), error.text);
}

export const changeActions = {
  level: (r) => {
    for (const l of r.closest(".levels")?.querySelectorAll(".level") ?? []) l.classList.toggle("on", l.contains(r));
    // Only the day fields this level uses are shown.
    if (isLevel(r.value)) {
      const form = r.closest("section");
      form?.querySelector<HTMLElement>("#full-days-row")?.toggleAttribute("hidden", !usesFullDays(r.value));
      form?.querySelector<HTMLElement>("#snippet-days-row")?.toggleAttribute("hidden", !usesSnippetDays(r.value));
    }
  },
  autostart: toggle((on) => api.setAutostart(on)),
  "auto-update": toggle((on) => api.setAutoUpdate(on), setAutoUpdate),
  // The "Launch at login" hint depends on it, so the page redraws.
  popups: toggle((on) => api.setPopups(on)),
  "menu-bar": toggle(
    (on) => api.setMenuBar(on),
    () => void render(),
  ),
  // The box is ticked already; if the app refuses, the redraw puts it back and says why.
  "report-cards-tips": async (box) => {
    // aria-disabled, not disabled: the box stays reachable, and this says why it won't tick.
    if (box instanceof HTMLInputElement && box.getAttribute("aria-disabled") === "true") {
      box.checked = false;
      say(
        confirmNote(),
        state.reportCards.status?.claude_code.on ? "Wait for the change in progress to finish." : "Turn on report cards in Claude Code first.",
      );
      return;
    }
    await saveCardsSetting(box, "tips", (on) => api.reportCardsSetTips(on));
  },
  "report-cards-reminders": (box) => saveCardsSetting(box, "reminders", (on) => api.reportCardsSetReminders(on)),
  quiz: (r) => r instanceof HTMLInputElement && judgeQuiz(r),
  "update-channel": async (sel) => {
    if (sel.value !== "stable" && sel.value !== "beta") return;
    const before = state.update?.channel ?? "stable";
    clearError(rowOf(sel));
    try {
      await api.setUpdateChannel(sel.value);
      setUpdateChannel(sel.value);
    } catch (e) {
      sel.value = before;
      showError(rowOf(sel), e);
    }
  },
  "period-pick": (sel) => {
    if (!isPeriod(sel.value)) return;
    setPeriod(sel.value);
    return render();
  },
} satisfies Record<string, InputHandler>;

const has = <T extends object>(map: T, key: string): key is Extract<keyof T, string> => Object.keys(map).includes(key);

const actionTarget = (e: Event) => (e.target instanceof Element ? e.target.closest<HTMLElement>("[data-action]") : null);

const SYNC_TIP = '.sync-text[aria-expanded="true"]';

/** Closes the status tooltip, except the one the click is on. */
function closeSyncTip(except: Element | null = null) {
  for (const b of document.querySelectorAll(SYNC_TIP)) if (b !== except) b.setAttribute("aria-expanded", "false");
}

/** Wires every control under `root`, once. */
export function bindActions(root: HTMLElement): void {
  root.addEventListener("click", (e) => {
    const el = actionTarget(e);
    closeSyncTip(el);
    const name = el?.dataset.action;
    if (!(el instanceof HTMLButtonElement) || !name || !has(clickActions, name)) return;
    try {
      Promise.resolve(clickActions[name](el)).catch(report);
    } catch (err) {
      report(err);
    }
  });
  root.addEventListener("focusout", (e) => {
    if (e.target instanceof Element && e.target.matches(SYNC_TIP)) e.target.setAttribute("aria-expanded", "false");
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeSyncTip();
  });
  // Typing in the prompt checker: kept in memory and re-graded when typing pauses.
  root.addEventListener("input", (e) => {
    const el = e.target;
    if (!(el instanceof HTMLTextAreaElement) || el.dataset.action !== "draft") return;
    setDraft(el.value);
    scheduleCheck(root);
  });
  root.addEventListener("change", (e) => {
    const el = actionTarget(e);
    const name = el?.dataset.action;
    if (!(el instanceof HTMLInputElement || el instanceof HTMLSelectElement) || !name || !has(changeActions, name)) return;
    Promise.resolve(changeActions[name](el)).catch(report);
  });
}
