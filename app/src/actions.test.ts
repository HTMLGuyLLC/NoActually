// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { meta, source, stats } from "./test/fixtures";

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
}));
vi.mock("./api", async (importOriginal) => ({ ...(await importOriginal<typeof import("./api")>()), api, demo: false }));

let render: () => Promise<void>;
let root: HTMLElement;
let scheduler: import("./scheduler").Scheduler;
let state: typeof import("./state");

beforeAll(async () => {
  document.body.innerHTML = `<div id="app"></div>`;
  const r = await import("./render");
  render = r.render;
  root = r.root;
  scheduler = r.scheduler;
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
  api.getMenuBar.mockResolvedValue(true);
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
    expect(pickStatFor("score", "briefs")).toBe("briefs");
    expect(pickStatFor("score", "catches")).toBe("catches");
    expect(pickStatFor("score", "hall")).toBe("catches");
  });

  it("shows both rates on Improve and Learn, whatever scorecard section was last open", async () => {
    const { pickStatFor } = await import("./actions");
    for (const tab of ["improve", "learn", "settings"] as const) expect(pickStatFor(tab, "briefs"), tab).toBe("both");
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
    expect(root.querySelector("h2")?.textContent).toBe("Sources");
    expect(btn('[aria-current="page"]').dataset.id).toBe("settings");
    expect(document.activeElement).toBe(btn('[data-action="tab"][data-id="settings"]'));
  });

  it("opens Settings at Sources from Choose sources, with no Sources tab of its own", async () => {
    expect(root.querySelector('[data-action="tab"][data-id="sources"]')).toBeNull();
    root.insertAdjacentHTML("beforeend", '<button data-action="go-sources">Choose sources</button>');
    await click('[data-action="go-sources"]');
    expect(state.state.tab).toBe("settings");
    expect(root.querySelector("section#sources h2")?.textContent).toBe("Sources");
  });

  it("takes two clicks to forget a source", async () => {
    await toTab("settings");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).not.toHaveBeenCalled();
    expect(btn('[data-action="forget"]').textContent).toBe("Click again to delete its data");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).toHaveBeenCalledWith("codex");
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
    await click('[data-action="view"][data-id="briefs"]');
    expect(root.querySelector('[data-action="view"][aria-current="page"]')?.textContent).toBe("Briefs");
    expect(root.textContent).toContain("Where steering went");
  });

  it("sends the logo home: scorecard overview", async () => {
    await click('[data-action="view"][data-id="briefs"]');
    await toTab("settings");
    await click('[data-action="home"]');
    expect(root.querySelector('[data-action="tab"][aria-current="page"]')?.textContent).toBe("Scorecard");
    expect(root.querySelector('[data-action="view"][aria-current="page"]')?.textContent).toBe("Overview");
  });

  it("pins and unpins a catch", async () => {
    await click('[data-action="view"][data-id="hall"]');
    await click('[data-action="pin"][data-id="1"]');
    expect(api.pin).toHaveBeenCalledWith(1, true);
  });

  it("saves privacy settings, clamped", async () => {
    await toTab("settings");
    const full = root.querySelector<HTMLInputElement>("#full-days");
    if (full) full.value = "99999";
    await click('[data-action="save-privacy"]');
    expect(api.setRetention).toHaveBeenCalledWith({ level: "snippet", full_days: 3650, snippet_days: 365 });
    expect(root.querySelector("#privacy-status")?.textContent).toBe("Saved. Older text was trimmed to match.");
  });

  it("un-ticks a toggle the backend refused", async () => {
    await toTab("settings");
    api.setAutostart.mockRejectedValueOnce("nope");
    const box = root.querySelector<HTMLInputElement>("#autostart");
    if (!box) throw new Error("no autostart box");
    box.checked = true;
    box.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(box.checked).toBe(false);
  });

  it("clears a filter", async () => {
    state.setFilter({ provider: "OpenAI" });
    await render();
    await click('[data-action="clear-filter"]');
    expect(state.state.filter).toEqual({});
    expect(api.stats).toHaveBeenLastCalledWith({ since: state.periodSince("28d") });
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
