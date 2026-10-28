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

describe("delegated actions", () => {
  it("switches tabs, moving aria-current and keeping keyboard focus on the tab", async () => {
    btn('[data-action="tab"][data-id="sources"]').focus();
    await toTab("sources");
    expect(state.state.tab).toBe("sources");
    expect(root.querySelector("h2")?.textContent).toBe("Sources");
    expect(btn('[aria-current="page"]').dataset.id).toBe("sources");
    expect(document.activeElement).toBe(btn('[data-action="tab"][data-id="sources"]'));
  });

  it("takes two clicks to forget a source", async () => {
    await toTab("sources");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).not.toHaveBeenCalled();
    expect(btn('[data-action="forget"]').textContent).toBe("Click again to delete its data");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).toHaveBeenCalledWith("codex");
  });

  it("allows a source with the chosen start, then redraws", async () => {
    await toTab("sources");
    const select = root.querySelector<HTMLSelectElement>('select[data-since="claude-code"]');
    if (select) select.value = "none";
    await click('[data-action="allow"][data-id="claude-code"]');
    expect(api.allow).toHaveBeenCalledWith("claude-code", null, true);
  });

  it("puts a failed Allow back and says why, instead of leaving it on Allowing…", async () => {
    await toTab("sources");
    api.allow.mockRejectedValueOnce("permission denied");
    await click('[data-action="allow"][data-id="claude-code"]');
    const b = btn('[data-action="allow"]');
    expect(b.textContent).toBe("Allow");
    expect(b.disabled).toBe(false);
    expect(root.querySelector('.action-error[role="alert"]')?.textContent).toBe("Couldn't do that: permission denied");
  });

  it("pins and unpins a catch", async () => {
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
    expect(api.stats).toHaveBeenLastCalledWith({});
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
