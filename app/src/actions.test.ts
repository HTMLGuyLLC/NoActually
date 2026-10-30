// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { must } from "./dom";
import { meta, overall, row, source, stats } from "./test/fixtures";

const allowed = { id: "claude-code", allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] };

const api = vi.hoisted(() => ({
  meta: vi.fn(),
  stats: vi.fn(),
  sources: vi.fn(),
  retention: vi.fn(),
  getAutostart: vi.fn(),
  allow: vi.fn(),
  forget: vi.fn(),
  pin: vi.fn(),
  setRetention: vi.fn(),
  setAutostart: vi.fn(),
  getMenuBar: vi.fn(),
  openMain: vi.fn(),
}));
vi.mock("./api", async (importOriginal) => ({ ...(await importOriginal<typeof import("./api")>()), api, demo: false }));

let render: () => Promise<void>;
let root: HTMLElement;
let scheduler: import("./scheduler").Scheduler;
let state: typeof import("./state");
let paintStatus: () => void;

beforeAll(async () => {
  document.body.innerHTML = `<div id="app"></div>`;
  const r = await import("./render");
  render = r.render;
  root = r.root;
  scheduler = r.scheduler;
  paintStatus = r.paintStatus;
  state = await import("./state");
  (await import("./actions")).bindActions(root);
});

beforeEach(async () => {
  state.resetState();
  api.meta.mockResolvedValue(meta);
  api.stats.mockResolvedValue(stats());
  api.sources.mockResolvedValue([source(), source({ id: "codex", name: "Codex", state: allowed })]);
  api.retention.mockResolvedValue({ level: "snippet", full_days: 30, snippet_days: 365 });
  api.getAutostart.mockResolvedValue(false);
  api.getMenuBar.mockResolvedValue({ enabled: true, available: true });
  api.allow.mockResolvedValue(null);
  api.forget.mockResolvedValue(null);
  api.pin.mockResolvedValue(null);
  api.setRetention.mockResolvedValue(null);
  api.setAutostart.mockResolvedValue(null);
  await render();
});
afterEach(() => vi.clearAllMocks());

const btn = (sel: string) => {
  const b = root.querySelector<HTMLButtonElement>(sel);
  if (!b) throw new Error(`no button ${sel}`);
  return b;
};
const settle = () => new Promise((r) => setTimeout(r, 0));
/** Clicks, then lets the async handler and any redraw settle. */
const click = async (sel: string) => {
  btn(sel).click();
  await settle();
};
const toTab = async (tab: string) => {
  await click(`[data-action="tab"][data-id="${tab}"]`);
};

describe("pickStatFor", () => {
  it("shows both rates on the overview, and the page's own numbers elsewhere", async () => {
    const { pickStatFor } = await import("./actions");
    expect(pickStatFor("score", "overview")).toBe("both");
    expect(pickStatFor("score", "prompts")).toBe("prompts");
    expect(pickStatFor("score", "corrections")).toBe("corrections");
    expect(pickStatFor("score", "hall")).toBe("corrections");
  });

  it("shows both rates on Improve and Learn, whatever scorecard section was last open", async () => {
    const { pickStatFor } = await import("./actions");
    for (const tab of ["improve", "learn", "settings"] as const) expect(pickStatFor(tab, "prompts"), tab).toBe("both");
  });
});

describe("parseGoalChoice", () => {
  const parse = async (raw: string) => (await import("./actions")).parseGoalChoice(raw);

  it("accepts each kind of goal with its target", async () => {
    expect(await parse('{"kind":"habit","signal":"why","target":40}')).toEqual({ kind: { kind: "habit", signal: "why" }, target: 40 });
    expect(await parse('{"kind":"steer","steer":"redo","target":0}')).toEqual({ kind: { kind: "steer", steer: "redo" }, target: 0 });
    expect(await parse('{"kind":"tier","tier":"coworker","target":0}')).toEqual({ kind: { kind: "tier", tier: "coworker" }, target: 0 });
  });

  it("drops fields it doesn't know", async () => {
    expect(await parse('{"kind":"tier","tier":"coworker","target":0,"extra":1}')).toEqual({ kind: { kind: "tier", tier: "coworker" }, target: 0 });
  });

  it("rejects junk, missing or non-numeric targets, and unknown kinds", async () => {
    for (const bad of [
      "",
      "not json",
      "null",
      "[]",
      "5",
      '"habit"',
      "{}",
      '{"kind":"habit","signal":"why"}',
      '{"kind":"habit","signal":"why","target":"40"}',
      '{"kind":"habit","signal":"why","target":null}',
      '{"kind":"habit","target":40}',
      '{"kind":"habit","signal":7,"target":40}',
      '{"kind":"steer","signal":"why","target":10}',
      '{"kind":"tier","tier":"x"}',
      '{"kind":"streak","tier":"x","target":1}',
    ]) {
      expect(await parse(bad), bad).toBeNull();
    }
  });

  it("rejects a target that isn't finite", async () => {
    // JSON can't carry NaN or Infinity, so a number too big for a double is the nearest thing.
    expect(await parse('{"kind":"habit","signal":"why","target":1e999}')).toBeNull();
  });
});

describe("delegated actions", () => {
  it("switches tabs, moving aria-current and keeping keyboard focus on the tab", async () => {
    btn('[data-action="tab"][data-id="settings"]').focus();
    await toTab("settings");
    expect(state.state.tab).toBe("settings");
    expect(root.querySelector("section#sources")).not.toBeNull();
    expect(btn('[aria-current="page"]').dataset.id).toBe("settings");
    expect(document.activeElement).toBe(btn('[data-action="tab"][data-id="settings"]'));
  });

  it("Choose sources opens the Sources sub-tab, even if another was open", async () => {
    state.setTab("settings");
    state.setSettingsPane("privacy");
    root.insertAdjacentHTML("beforeend", '<button data-action="go-sources">Choose sources</button>');
    await click('[data-action="go-sources"]');
    expect(state.state.settingsPane).toBe("sources");
  });

  it("opens Settings at Sources from Choose sources, with no Sources tab of its own", async () => {
    expect(root.querySelector('[data-action="tab"][data-id="sources"]')).toBeNull();
    root.insertAdjacentHTML("beforeend", '<button data-action="go-sources">Choose sources</button>');
    await click('[data-action="go-sources"]');
    expect(state.state.tab).toBe("settings");
    expect(root.querySelector("section#sources")).not.toBeNull();
  });

  it("opens the app on a named tab, and ignores a name that isn't a tab", async () => {
    root.insertAdjacentHTML(
      "beforeend",
      '<button id="gear" data-action="open-main" data-id="settings"></button><button id="bad" data-action="open-main" data-id="nope"></button>',
    );
    await click("#gear");
    await click("#bad");
    expect(api.openMain.mock.calls).toEqual([["settings"], [null]]);
  });

  it("flips an award's entries in place and keeps the place across a redraw", async () => {
    const { hallEntry } = await import("./test/fixtures");
    api.stats.mockResolvedValue(
      stats({
        categories: { just_wrong: 2 },
        hall_of_fame: [hallEntry({ id: 1, category: "just_wrong", human: "first" }), hallEntry({ id: 2, category: "just_wrong", human: "second" })],
      }),
    );
    state.setView("hall");
    await render();
    const shown = () => root.querySelector('[data-award="just_wrong"] .slide:not([aria-hidden])')?.textContent ?? "";
    expect(shown()).toContain("first");
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="1"]');
    expect(shown()).toContain("second");
    expect(root.querySelector('[data-award="just_wrong"] .slide-at')?.textContent).toBe("2");
    await render();
    expect(shown()).toContain("second");
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="1"]');
    expect(shown()).toContain("first"); // wraps around
  });

  it("stays on the entry you pinned when pinning moves it to the front", async () => {
    const { hallEntry } = await import("./test/fixtures");
    const entries = ["a1", "a2", "a3"].map((h, i) => hallEntry({ id: i + 1, category: "just_wrong", human: h }));
    api.stats.mockResolvedValue(stats({ categories: { just_wrong: 3 }, hall_of_fame: entries }));
    state.setView("hall");
    await render();
    const shown = () =>
      [...root.querySelectorAll('[data-award="just_wrong"] .slide')].find((el) => el.getAttribute("aria-hidden") !== "true")?.querySelector(".you-said")
        ?.textContent ?? "";
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="1"]');
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="1"]');
    expect(shown()).toContain("a3");
    // Rust sends the pinned one first after the pin.
    const third = entries[2] ?? hallEntry();
    api.stats.mockResolvedValue(stats({ categories: { just_wrong: 3 }, hall_of_fame: [{ ...third, pinned: true }, entries[0] ?? third, entries[1] ?? third] }));
    await click('[data-award="just_wrong"] .slide:not([aria-hidden]) [data-action="pin"]');
    expect(api.pin).toHaveBeenCalledWith(3, true);
    expect(shown()).toContain("a3");
    expect(root.querySelector('[data-award="just_wrong"] .slide-at')?.textContent).toBe("1");
  });

  it("loops through five entries both ways", async () => {
    const { hallEntry } = await import("./test/fixtures");
    const names = ["a1", "a2", "a3", "a4", "a5"];
    api.stats.mockResolvedValue(
      stats({ categories: { just_wrong: 5 }, hall_of_fame: names.map((h, i) => hallEntry({ id: i + 1, category: "just_wrong", human: h })) }),
    );
    state.setView("hall");
    await render();
    // What a person sees: the slides the stylesheet doesn't hide (every other one is aria-hidden="true").
    const shown = () => {
      const visible = [...root.querySelectorAll('[data-award="just_wrong"] .slide')].filter((el) => el.getAttribute("aria-hidden") !== "true");
      expect(visible).toHaveLength(1);
      return visible[0]?.querySelector(".you-said")?.textContent ?? "";
    };
    const next = () => click('[data-award="just_wrong"] [data-action="award-slide"][data-step="1"]');
    const seen: string[] = [];
    for (let i = 0; i < 7; i++) {
      seen.push(shown());
      await next();
    }
    expect(seen.map((t) => t.replace(/[“”]/g, ""))).toEqual(["a1", "a2", "a3", "a4", "a5", "a1", "a2"]);
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="-1"]');
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="-1"]');
    await click('[data-award="just_wrong"] [data-action="award-slide"][data-step="-1"]');
    expect(shown().replace(/[“”]/g, "")).toBe("a5");
  });

  it("takes two clicks to forget a source", async () => {
    await toTab("settings");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).not.toHaveBeenCalled();
    expect(btn('[data-action="forget"]').textContent).toBe("Click again to delete its data");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).toHaveBeenCalledWith("codex");
  });

  it("announces the confirm prompt and disarms it after a few seconds", async () => {
    await toTab("settings");
    vi.useFakeTimers();
    try {
      const b = btn('[data-action="forget"][data-id="codex"]');
      const label = b.textContent;
      b.click();
      vi.advanceTimersByTime(1);
      const note = must("#confirm-note");
      expect(note.getAttribute("role")).toBe("status");
      expect(note.textContent).toBe("Click again to delete its data");
      vi.advanceTimersByTime(5000);
      expect(b.textContent).toBe(label);
      expect(b.dataset.confirm).toBeUndefined();
      expect(note.textContent).toBe("");
      b.click(); // armed again, not confirmed
      expect(api.forget).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("disarms a confirm when focus leaves the button", async () => {
    await toTab("settings");
    const b = btn('[data-action="forget"][data-id="codex"]');
    const label = b.textContent;
    b.click();
    expect(b.textContent).toBe("Click again to delete its data");
    b.dispatchEvent(new FocusEvent("blur"));
    expect(b.textContent).toBe(label);
    expect(root.querySelector('[data-confirm="yes"]')).toBeNull();
  });

  it("keeps the picker's counts to the scorecard's period", async () => {
    const model = (n: number) => row("Anthropic", "Claude Opus", "5", n);
    api.stats.mockImplementation(async (f: { provider?: string; since?: string }) => stats({ models: [model(f.since === state.periodSince("7d") ? 3 : 30)] }));
    await render();
    expect(state.state.allModels[0]?.corrections).toBe(30);
    state.setFilter({ provider: "Anthropic" });
    state.setPeriod("7d");
    await render();
    expect(state.state.allModels[0]?.corrections).toBe(3);
  });

  it("allows a source with the chosen start, then redraws", async () => {
    await toTab("settings");
    const select = root.querySelector<HTMLSelectElement>('select[data-since="claude-code"]');
    if (select) select.value = "none";
    await click('[data-action="allow"][data-id="claude-code"]');
    expect(api.allow).toHaveBeenCalledWith("claude-code", null, true);
  });

  it("puts a failed Allow back and says why, instead of leaving it on Allowing…", async () => {
    await toTab("settings");
    api.allow.mockRejectedValueOnce("permission denied");
    await click('[data-action="allow"][data-id="claude-code"]');
    const b = btn('[data-action="allow"]');
    expect(b.textContent).toBe("Allow");
    expect(b.disabled).toBe(false);
    expect(root.querySelector('.action-error[role="alert"]')?.textContent).toBe("Couldn't do that: permission denied");
  });

  it("switches scorecard sections", async () => {
    expect(root.querySelector('[data-action="view"][aria-current="page"]')?.textContent).toBe("Overview");
    await click('[data-action="view"][data-id="prompts"]');
    expect(root.querySelector('[data-action="view"][aria-current="page"]')?.textContent).toBe("Prompting");
    expect(root.textContent).toContain("Why you followed up");
  });

  it("sends the logo home: scorecard overview", async () => {
    await click('[data-action="view"][data-id="prompts"]');
    await toTab("settings");
    await click('[data-action="home"]');
    expect(root.querySelector('[data-action="tab"][aria-current="page"]')?.textContent).toBe("Scorecard");
    expect(root.querySelector('[data-action="view"][aria-current="page"]')?.textContent).toBe("Overview");
  });

  it("pins and unpins a correction", async () => {
    await click('[data-action="view"][data-id="hall"]');
    await click('[data-action="pin"][data-id="1"]');
    expect(api.pin).toHaveBeenCalledWith(1, true);
  });

  it("saves privacy settings, clamped", async () => {
    await toTab("settings");
    await click('[data-action="settings-pane"][data-id="privacy"]');
    const full = root.querySelector<HTMLInputElement>("#full-days");
    if (full) full.value = "99999";
    await click('[data-action="save-privacy"]');
    expect(api.setRetention).toHaveBeenCalledWith({ level: "snippet", full_days: 3650, snippet_days: 365 });
    expect(root.querySelector("#privacy-status")?.textContent).toBe("Saved. Older text was trimmed to match.");
  });

  it("un-ticks a toggle the backend refused", async () => {
    await toTab("settings");
    await click('[data-action="settings-pane"][data-id="general"]');
    api.setAutostart.mockRejectedValueOnce("nope");
    const box = root.querySelector<HTMLInputElement>("#autostart");
    if (!box) throw new Error("no autostart box");
    box.checked = true;
    box.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(box.checked).toBe(false);
  });

  it("holds a background refresh while the picker is open and applies it when it closes", async () => {
    await click('[data-action="model-picker"]');
    expect(document.querySelector(".picker-pop")).not.toBeNull();
    const before = api.stats.mock.calls.length;
    scheduler.refresh();
    await settle();
    expect(api.stats.mock.calls.length).toBe(before);
    document.querySelector(".pick-search")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await settle();
    expect(api.stats.mock.calls.length).toBe(before + 1);
    expect(document.activeElement?.id).toBe("model-trigger");
  });

  it("holds a background refresh while the share dialog is open and re-focuses the opener on close", async () => {
    await click('[data-action="open-share"]');
    const dlg = document.querySelector("dialog.share-dialog");
    expect(dlg).not.toBeNull();
    const before = api.stats.mock.calls.length;
    scheduler.refresh();
    await settle();
    expect(api.stats.mock.calls.length).toBe(before);
    dlg?.querySelector<HTMLButtonElement>('[data-sd="close"]')?.click();
    await settle();
    expect(api.stats.mock.calls.length).toBe(before + 1);
    expect(document.activeElement?.getAttribute("data-action")).toBe("open-share");
  });

  it("shows an error page with a retry when loading fails", async () => {
    api.stats.mockRejectedValueOnce("database is locked");
    await render();
    expect(root.textContent).toContain("database is locked");
    await click('[data-action="retry"]');
    expect(root.querySelector(".card")).not.toBeNull();
  });
});

describe("sync pill repaint", () => {
  const active = () => document.activeElement?.className;

  it("keeps focus on the same control inside the new pill", () => {
    for (const cls of ["sync-now", "sync-text"]) {
      root.querySelector<HTMLElement>(`.${cls}`)?.focus();
      expect(active(), cls).toBe(cls);
      paintStatus();
      expect(active(), cls).toBe(cls);
    }
  });

  it("falls back to the first control when that one is gone", () => {
    root.querySelector<HTMLElement>(".sync-now")?.focus();
    state.setStatus({ syncing: false, last_sync: null, last_error: "boom", files_with_errors: 0 });
    paintStatus();
    expect(active()).toBe("sync-bad");
  });

  it("leaves focus alone when it wasn't in the pill", () => {
    const tab = btn('[data-action="tab"][data-id="improve"]');
    tab.focus();
    paintStatus();
    expect(document.activeElement).toBe(tab);
  });
});

describe("verdict scrawl", () => {
  const still = (sel: string) => root.querySelector(`${sel} .no`)?.classList.contains("still");

  it("replays the corrections verdict only when the correction tier changes", async () => {
    await render();
    expect(still(".card.mini.prompts"), "prompts").toBe(true);
    expect(still(".card.mini.corrections"), "corrections").toBe(true);
    // Another tier for corrections only: the overall verdict and the prompts verdict stay still.
    api.stats.mockResolvedValue(stats({ overall: overall({ correction_tier: "autopilot" }) }));
    await render();
    expect(still(".card.hero"), "hero").toBe(true);
    expect(still(".card.mini.prompts"), "prompts").toBe(true);
    expect(still(".card.mini.corrections"), "corrections").toBe(false);
  });
});
