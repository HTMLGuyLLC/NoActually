// What every control does. Views mark controls with data-action="…" (and data-id for the thing
// acted on); one delegated listener per event type on the root dispatches through the maps below,
// so nothing is re-bound on each redraw.
import { api, type BriefTier, type GoalKind, type HallEntry, type Meta, type Stats } from "./api";
import { data, say } from "./dom";
import { errorText, filterLabel } from "./format";
import { scheduleCheck } from "./learn/checker";
import { lessonById } from "./learn/lessons";
import { briefTier, catchTier, tierName } from "./meta";
import type { PickStat } from "./picker/picker";
import { buildPickItems, sameFilter } from "./picker/picker";
import { closePicker, openPicker, pickerOpen } from "./picker/popover";
import { paintUpdate, render, scheduler } from "./render";
import { drawBriefCard, drawCatchCard, drawCatchesCard, drawShareCard } from "./share/cards";
import { openShareDialog, type Post } from "./share/dialog";
import { briefShareText, catchesShareText, catchShareText, shareText } from "./share/share";
import {
  isPeriod,
  isRange,
  isScoreView,
  isTab,
  PERIOD_WHEN,
  setAutoUpdate,
  setDraft,
  setFilter,
  setLaterFor,
  setLesson,
  setPeriod,
  setRange,
  setTab,
  setView,
  state,
} from "./state";
import { judgeQuiz } from "./views/learn";
import { parseRetention } from "./views/settings";
import { allowArgs } from "./views/sources";

/**
 * Runs a button's async work: disables it (optionally relabelled, "Allowing…"), and always puts it
 * back. A failure shows inline next to the button instead of vanishing. Resolves true on success.
 */
export async function run(btn: HTMLButtonElement, work: () => Promise<unknown>, pending?: string): Promise<boolean> {
  const label = btn.textContent;
  const slot = btn.parentElement;
  slot?.querySelector(":scope > .action-error")?.remove();
  btn.disabled = true;
  if (pending) btn.textContent = pending;
  try {
    await work();
    return true;
  } catch (e) {
    const err = document.createElement("span");
    err.className = "action-error";
    err.setAttribute("role", "alert");
    err.textContent = `Couldn't do that: ${errorText(e)}`;
    slot?.append(err);
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

const briefsPost = (s: Stats, tier: BriefTier): Post => ({
  title: "Share your briefs",
  text: briefShareText(s.briefs, tier, when()),
  image: (shape) => drawBriefCard(s.briefs, tier, shape, when()),
});

const catchesPost = (s: Stats, tier: BriefTier): Post => ({
  title: "Share your catches",
  text: catchesShareText(s.overall, tier, when()),
  image: (shape) => drawCatchesCard(s.overall, tier, shape, when()),
});

const catchPost = (e: HallEntry): Post => ({
  title: "Share this catch",
  text: catchShareText(e),
  image: (shape) => drawCatchCard(e, shape),
  quotesChat: !!(e.human || e.ai),
});

/** The period the drawn numbers cover: the scorecard's picker, or the default 28 days elsewhere. */
const when = () => PERIOD_WHEN[state.tab === "score" ? state.period : "28d"];

/** True on the second click. The first relabels the button and holds off redraws until then. */
function confirmed(b: HTMLButtonElement, ask: string): boolean {
  if (b.dataset.confirm === "yes") return true;
  b.dataset.confirm = "yes";
  b.textContent = ask;
  return false;
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
  return view === "briefs" ? "briefs" : "catches";
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
  "share-briefs": (b) => {
    const { s, meta } = drawn();
    const tier = briefTier(meta, s.briefs.tier);
    if (tier) openShareDialog(briefsPost(s, tier), b, scheduler.flush);
  },
  "share-catches": (b) => {
    const { s, meta } = drawn();
    const tier = catchTier(meta, s.overall.catch_tier);
    if (tier) openShareDialog(catchesPost(s, tier), b, scheduler.flush);
  },
  "share-catch": (b) => {
    const id = Number(data(b, "id"));
    const e = drawn().s.hall_of_fame.find((h) => h.id === id);
    if (e) openShareDialog(catchPost(e), b, scheduler.flush);
  },
  "open-main": (b) => {
    const tab = b.dataset.id;
    return api.openMain(isTab(tab) ? tab : null);
  },
  home: () => {
    setTab("score");
    setView("overview");
    return render();
  },
  "go-sources": async () => {
    setTab("settings");
    await render();
    document.getElementById("sources")?.scrollIntoView({ block: "start" });
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
    say(b.parentElement?.querySelector("#draft-status") ?? null, ok ? "Copied your brief." : "Couldn't copy.");
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
    if (!confirmed(b, "Click again to drop it")) return;
    return thenRender(b, () => api.clearGoal());
  },
  pin: (b) => thenRender(b, () => api.pin(Number(data(b, "id")), data(b, "pinned") !== "true")),
  allow: (b) => {
    const id = data(b, "id");
    const choice = b.parentElement?.querySelector<HTMLSelectElement>(`select[data-since="${CSS.escape(id)}"]`)?.value ?? "all";
    const { since, fromNow } = allowArgs(choice, new Date());
    return thenRender(b, () => api.allow(id, since, fromNow), "Allowing…");
  },
  revoke: (b) => thenRender(b, () => api.revoke(data(b, "id"))),
  pause: (b) => thenRender(b, () => api.pause(data(b, "id"), data(b, "paused") !== "true")),
  import: (b) => thenRender(b, () => api.importExport(data(b, "id"))),
  // Forget deletes data, so it takes a second click to confirm.
  forget: (b) => {
    if (!confirmed(b, "Click again to delete its data")) return;
    return thenRender(b, () => api.forget(data(b, "id")));
  },
  "save-privacy": async (b) => {
    const form = b.closest("section") ?? document;
    const out = form.querySelector("#privacy-status");
    const value = (sel: string) => form.querySelector<HTMLInputElement>(sel)?.value ?? "";
    try {
      const retention = parseRetention({
        level: form.querySelector<HTMLInputElement>('input[name="level"]:checked')?.value ?? null,
        fullDays: value("#full-days"),
        snippetDays: value("#snippet-days"),
      });
      await api.setRetention(retention);
      say(out, "Saved. Older text was trimmed to match.");
    } catch (e) {
      say(out, errorText(e));
    }
  },
  "check-update": () => api.checkForUpdates(),
  "sync-now": () => api.syncNow(),
  "install-update": (b) => run(b, () => api.installUpdate()),
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

/** Undoes a checkbox the backend refused, so it never shows a setting that isn't saved. */
const toggle = (save: (on: boolean) => Promise<unknown>, saved?: (on: boolean) => void) => async (box: HTMLInputElement | HTMLSelectElement) => {
  if (!(box instanceof HTMLInputElement)) return;
  try {
    await save(box.checked);
    saved?.(box.checked);
  } catch {
    box.checked = !box.checked;
  }
};

export const changeActions = {
  level: (r) => {
    for (const l of r.closest(".levels")?.querySelectorAll(".level") ?? []) l.classList.toggle("on", l.contains(r));
  },
  autostart: toggle((on) => api.setAutostart(on)),
  "auto-update": toggle((on) => api.setAutoUpdate(on), setAutoUpdate),
  // The "Launch at login" hint depends on it, so the page redraws.
  "menu-bar": toggle(
    (on) => api.setMenuBar(on),
    () => void render(),
  ),
  quiz: (r) => r instanceof HTMLInputElement && judgeQuiz(r),
  "period-pick": (sel) => {
    if (!isPeriod(sel.value)) return;
    setPeriod(sel.value);
    return render();
  },
} satisfies Record<string, InputHandler>;

const has = <T extends object>(map: T, key: string): key is Extract<keyof T, string> => Object.keys(map).includes(key);

const actionTarget = (e: Event) => (e.target instanceof Element ? e.target.closest<HTMLElement>("[data-action]") : null);

const report = (e: unknown) => console.error("Action failed:", e);

/** Wires every control under `root`, once. */
export function bindActions(root: HTMLElement): void {
  root.addEventListener("click", (e) => {
    const el = actionTarget(e);
    const name = el?.dataset.action;
    if (!(el instanceof HTMLButtonElement) || !name || !has(clickActions, name)) return;
    try {
      Promise.resolve(clickActions[name](el)).catch(report);
    } catch (err) {
      report(err);
    }
  });
  // Typing in the brief checker: kept in memory and re-graded when typing pauses.
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
