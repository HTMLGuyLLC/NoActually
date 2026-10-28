// What every control does. Views mark controls with data-action="…" (and data-id for the thing
// acted on); one delegated listener per event type on the root dispatches through the maps below,
// so nothing is re-bound on each redraw.
import { api, type HallEntry, type Meta, type Stats } from "./api";
import { data, say } from "./dom";
import { errorText, filterLabel } from "./format";
import { tierName } from "./meta";
import { buildPickItems, sameFilter } from "./picker/picker";
import { closePicker, openPicker, pickerOpen } from "./picker/popover";
import { paintUpdate, render, scheduler } from "./render";
import { drawCatchCard, drawShareCard } from "./share/cards";
import { openShareDialog, type Post } from "./share/dialog";
import { catchShareText, shareText } from "./share/share";
import { isTab, setAutoUpdate, setFilter, setLaterFor, setTab, state } from "./state";
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
  return { title: "Share your score", text: shareText(s, who), image: () => drawShareCard(s, who, tierName(meta, s.tier)) };
};

const catchPost = (e: HallEntry): Post => ({
  title: "Share this catch",
  text: catchShareText(e),
  image: () => drawCatchCard(e),
  quotesChat: !!(e.human || e.ai),
});

/** The data the scorecard was drawn from; its buttons only exist once both are loaded. */
function drawn(): { s: Stats; meta: Meta } {
  const { current: s, meta } = state;
  if (!s || !meta) throw new Error("Nothing drawn yet");
  return { s, meta };
}

function togglePicker(trigger: HTMLButtonElement) {
  if (pickerOpen()) return closePicker();
  openPicker(trigger, {
    items: buildPickItems(state.allModels),
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
  // The button itself confirms the copy, so the quick glance never grows taller.
  "copy-share": async (b) => {
    const { s, meta } = drawn();
    const ok = await navigator.clipboard.writeText(shareText(s, filterLabel(state.filter, meta))).then(
      () => true,
      () => false,
    );
    b.textContent = ok ? "Copied ✓" : "Couldn't copy";
    say(b.parentElement?.querySelector("#copy-status") ?? null, ok ? "Copied the share text." : "Couldn't copy the share text.");
    setTimeout(() => {
      b.textContent = "Copy text";
    }, 1800);
  },
  "open-share": (b) => {
    const { s, meta } = drawn();
    openShareDialog(scorePost(s, meta), b, scheduler.flush);
  },
  "share-catch": (b) => {
    const id = Number(data(b, "id"));
    const e = drawn().s.hall_of_fame.find((h) => h.id === id);
    if (e) openShareDialog(catchPost(e), b, scheduler.flush);
  },
  "open-main": () => api.openMain(),
  "go-sources": () => {
    setTab("sources");
    return render();
  },
  tab: (b) => {
    const tab = data(b, "id");
    if (!isTab(tab)) throw new Error(`Unknown tab: ${tab}`);
    setTab(tab);
    return render();
  },
  "model-picker": (b) => togglePicker(b),
  "clear-filter": () => {
    setFilter({});
    return render();
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
    if (b.dataset.confirm !== "yes") {
      b.dataset.confirm = "yes";
      b.textContent = "Click again to delete its data";
      return;
    }
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

type InputHandler = (el: HTMLInputElement) => unknown;

/** Undoes a checkbox the backend refused, so it never shows a setting that isn't saved. */
const toggle = (save: (on: boolean) => Promise<unknown>, saved?: (on: boolean) => void) => async (box: HTMLInputElement) => {
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
  root.addEventListener("change", (e) => {
    const el = actionTarget(e);
    const name = el?.dataset.action;
    if (!(el instanceof HTMLInputElement) || !name || !has(changeActions, name)) return;
    Promise.resolve(changeActions[name](el)).catch(report);
  });
}
