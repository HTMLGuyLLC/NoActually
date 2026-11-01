// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { must } from "./dom";
import { memoryStorage, meta, overall, progress, row, source, stats } from "./test/fixtures";

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
  progress: vi.fn(),
  recentReportCards: vi.fn(),
  reportCardsStatus: vi.fn(),
  logError: vi.fn(),
  openLogFolder: vi.fn(),
  openShare: vi.fn(),
  syncNow: vi.fn(),
  setAutoUpdate: vi.fn(),
  installUpdate: vi.fn(),
  updateStatus: vi.fn(),
  getPopups: vi.fn(),
  setPopups: vi.fn(),
  rescoredNotice: vi.fn(),
  openNotices: vi.fn(),
  importExport: vi.fn(),
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
  state.setSettingsPane("sources");
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
  api.reportCardsStatus.mockResolvedValue(null);
  api.logError.mockResolvedValue(null);
  api.openLogFolder.mockResolvedValue(null);
  api.getPopups.mockResolvedValue(true);
  api.setPopups.mockResolvedValue(null);
  api.rescoredNotice.mockResolvedValue(null);
  api.openNotices.mockResolvedValue(null);
  vi.stubGlobal("localStorage", memoryStorage());
  await render();
});
afterEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
});

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
/**
 * Holds the clock still, so the next click can be put a set time after the last one. A second click on an
 * armed button sooner than 400 ms is a double-click and does nothing.
 */
const holdClock = () => vi.useFakeTimers({ toFake: ["Date"] });
const later = (ms = 500) => vi.setSystemTime(Date.now() + ms);
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
    root.insertAdjacentHTML("beforeend", '<button data-action="go-settings" data-id="sources">Choose sources</button>');
    await click('[data-action="go-settings"]');
    expect(state.state.settingsPane).toBe("sources");
  });

  it("opens Settings at Sources from Choose sources, with no Sources tab of its own", async () => {
    expect(root.querySelector('[data-action="tab"][data-id="sources"]')).toBeNull();
    root.insertAdjacentHTML("beforeend", '<button data-action="go-settings" data-id="sources">Choose sources</button>');
    await click('[data-action="go-settings"]');
    expect(state.state.tab).toBe("settings");
    expect(root.querySelector("section#sources")).not.toBeNull();
  });

  it("scrolls to the top when it opens a Settings section, whichever one", async () => {
    const scrollTo = vi.fn();
    vi.stubGlobal("scrollTo", scrollTo);
    for (const id of ["sources", "report-cards"]) {
      root.insertAdjacentHTML("beforeend", `<button data-action="go-settings" data-id="${id}">Go</button>`);
      scrollTo.mockClear();
      await click(`[data-action="go-settings"][data-id="${id}"]`);
      expect(scrollTo, id).toHaveBeenCalledWith({ top: 0 });
    }
    vi.unstubAllGlobals();
  });

  it("only opens a Settings section that exists", async () => {
    state.setTab("improve");
    root.insertAdjacentHTML("beforeend", '<button data-action="go-settings" data-id="nope">Go</button>');
    await click('[data-action="go-settings"][data-id="nope"]');
    expect(state.state.tab).toBe("improve");
  });

  it("opens a named Settings section from another tab", async () => {
    state.setTab("improve");
    root.insertAdjacentHTML("beforeend", '<button data-action="go-settings" data-id="privacy">Set up</button>');
    await click('[data-action="go-settings"]');
    expect(state.state.tab).toBe("settings");
    expect(state.state.settingsPane).toBe("privacy");
    expect(root.querySelector("section#privacy, .levels")).not.toBeNull();
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

  it("takes two clicks to forget a source, and says what goes and that it's final", async () => {
    holdClock();
    await toTab("settings");
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).not.toHaveBeenCalled();
    expect(btn('[data-action="forget"]').textContent).toBe("Click again to delete its scores, pins and report cards for good");
    later();
    await click('[data-action="forget"][data-id="codex"]');
    expect(api.forget).toHaveBeenCalledWith("codex");
  });

  it("ignores a second click within 400 ms, a double-click, and still takes a real one after", async () => {
    holdClock();
    await toTab("settings");
    const forget = '[data-action="forget"][data-id="codex"]';
    await click(forget);
    later(399);
    await click(forget);
    expect(api.forget).not.toHaveBeenCalled();
    expect(btn(forget).dataset.confirm).toBe("yes"); // still armed, still asking
    later(1);
    await click(forget);
    expect(api.forget).toHaveBeenCalledTimes(1);
  });

  it("guards dropping a goal the same way", async () => {
    holdClock();
    api.progress.mockResolvedValue(progress({ goal: null }));
    root.insertAdjacentHTML("beforeend", '<button id="drop" data-action="clear-goal">Drop</button>');
    const drop = btn("#drop");
    drop.click();
    expect(drop.textContent).toBe("Click again to drop this goal and its progress for good");
    drop.click(); // a double-click
    expect(drop.dataset.confirm).toBe("yes");
    expect(drop.textContent).toBe("Click again to drop this goal and its progress for good");
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
      expect(note.textContent).toBe("Click again to delete its scores, pins and report cards for good");
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
    expect(b.textContent).toBe("Click again to delete its scores, pins and report cards for good");
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

  describe("the first Allow", () => {
    const allowFirst = async (choice?: string) => {
      api.stats.mockResolvedValue(stats({ messages: 0 }));
      api.sources.mockResolvedValue([source()]);
      await render();
      await toTab("settings");
      if (choice) {
        const select = root.querySelector<HTMLSelectElement>('select[data-since="claude-code"]');
        if (select) select.value = choice;
      }
      api.sources.mockResolvedValue([source({ state: allowed })]);
      await click('[data-action="allow"][data-id="claude-code"]');
    };

    it("goes to the scorecard and says it is reading, not that all is set", async () => {
      await allowFirst("all");
      expect(root.querySelector('[data-action="tab"][data-id="score"][aria-current="page"]')).not.toBeNull();
      expect(root.textContent).toContain("Reading your history…");
      expect(root.textContent).not.toContain("You're all set.");
    });

    it("stays in Settings for From now on, where there is no history to read", async () => {
      await allowFirst("none");
      expect(state.state.tab).toBe("settings");
    });

    it("says all is set only when no read is under way", async () => {
      await allowFirst("all");
      state.setReading("off");
      await render();
      expect(root.textContent).toContain("You're all set.");
    });

    it("offers the way back to the scorecard when the read ends elsewhere", async () => {
      await allowFirst("all");
      await toTab("settings");
      state.setReading("done");
      await render();
      expect(root.textContent).toContain("Your history is read.");
      await click('#notice-slot [data-action="home"]');
      expect(state.state.reading).toBe("off");
      expect(state.state.tab).toBe("score");
      expect(root.querySelector("#notice-slot")).toBeNull();
    });

    it("stays where it is when something is already counted", async () => {
      await toTab("settings");
      await click('[data-action="allow"][data-id="claude-code"]');
      expect(state.state.tab).toBe("settings");
      expect(state.state.reading).toBe("off");
    });
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

  it("scrolls to the top on a section change", async () => {
    const scrollTo = vi.fn();
    vi.stubGlobal("scrollTo", scrollTo);
    await click('[data-action="view"][data-id="corrections"]');
    expect(scrollTo).toHaveBeenCalledWith({ top: 0 });
    vi.unstubAllGlobals();
  });

  it("keeps the pickers on an empty period when history exists, and widens from there", async () => {
    api.stats.mockImplementation((f?: { since?: string }) => Promise.resolve(f?.since === "1970-01-01" ? stats() : stats({ messages: 0 })));
    state.setFilter({ family: "Claude" });
    await render();
    expect(root.textContent).toContain("No chats with");
    expect(root.textContent).not.toContain("Let's see if AI could replace you.");
    expect(root.querySelector("#model-trigger")).not.toBeNull();
    expect(root.querySelector('[data-action="period"]')).not.toBeNull();
    expect(btn('[data-action="widen"]').textContent).toBe("Show all time and all AI");
    api.stats.mockResolvedValue(stats());
    await click('[data-action="widen"]');
    expect(state.state.period).toBe("all");
    expect(state.state.filter).toEqual({});
    expect(root.querySelector('[data-action="widen"]')).toBeNull();
  });

  it("shows first-run onboarding only when all time is empty too", async () => {
    api.stats.mockResolvedValue(stats({ messages: 0 }));
    await render();
    expect(root.textContent).toContain("You're all set.");
    expect(root.querySelector('[data-action="widen"]')).toBeNull();
  });

  it("keeps the header on the error page and offers the log folder", async () => {
    api.stats.mockRejectedValue(new Error("boom"));
    await render();
    expect(root.textContent).toContain("Something went wrong.");
    expect(root.querySelector('[data-action="tab"]')).not.toBeNull();
    await click('[data-action="open-log-folder"]');
    expect(api.openLogFolder).toHaveBeenCalled();
  });

  it("opens the false-positive form from a correction", async () => {
    await click('[data-action="view"][data-id="hall"]');
    const b = btn('[data-action="counted-wrong"]');
    expect(b.dataset.tip).toContain("Leave out");
    await click('[data-action="counted-wrong"]');
    expect(api.openShare).toHaveBeenCalledWith("https://github.com/HTMLGuyLLC/NoActually/issues/new?template=false_positive.yml");
  });

  it("says so when a check for new chats fails", async () => {
    await toTab("settings");
    api.syncNow.mockRejectedValue(new Error("offline"));
    await click('[data-action="sync-now"]:not(.sync-now)');
    expect(root.querySelector('.action-error[role="alert"]')?.textContent).toBe("Couldn't do that: offline");
  });

  it("puts a refused toggle back and says why", async () => {
    await toTab("settings");
    await click('[data-action="settings-pane"][data-id="general"]');
    api.setAutostart.mockRejectedValue(new Error("not allowed"));
    const box = btn("#autostart") as unknown as HTMLInputElement;
    const before = box.checked;
    box.click();
    await settle();
    expect(root.querySelector<HTMLInputElement>("#autostart")?.checked).toBe(before);
    expect(root.querySelector("#autostart")?.closest(".settings-row")?.querySelector(".action-error")?.textContent).toBe("Couldn't do that: not allowed");
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

  describe("saving privacy", () => {
    const open = async () => {
      await toTab("settings");
      await click('[data-action="settings-pane"][data-id="privacy"]');
    };
    const field = (id: string, value: string) => {
      const el = root.querySelector<HTMLInputElement>(id);
      if (el) el.value = value;
    };

    it("clamps, redraws the values as saved, and doesn't say it trimmed when it kept as much", async () => {
      await open();
      field("#full-days", "99999");
      await click('[data-action="save-privacy"]');
      expect(api.setRetention).toHaveBeenCalledWith({ level: "snippet", full_days: 3650, snippet_days: 365 });
      expect(root.querySelector("#privacy-status")?.textContent).toBe("Saved.");
      // The redraw reads the saved value back, not what was typed.
      api.retention.mockResolvedValue({ level: "snippet", full_days: 3650, snippet_days: 365 });
      await render();
      expect(root.querySelector<HTMLInputElement>("#full-days")?.value).toBe("3650");
    });

    it("asks once more, naming what goes, before keeping less", async () => {
      holdClock();
      await open();
      field("#full-days", "5");
      await click('[data-action="save-privacy"]');
      expect(api.setRetention).not.toHaveBeenCalled();
      expect(btn('[data-action="save-privacy"]').textContent).toBe("Click again to delete older text for good");
      later();
      await click('[data-action="save-privacy"]');
      expect(api.setRetention).toHaveBeenCalledWith({ level: "snippet", full_days: 5, snippet_days: 365 });
      expect(root.querySelector("#privacy-status")?.textContent).toBe("Saved. Older text was trimmed to match.");
    });

    it("counts the corrections that lose their text when nothing is kept but the category", async () => {
      holdClock();
      await open();
      const none = root.querySelector<HTMLInputElement>('input[name="level"][value="category"]');
      if (none) none.checked = true;
      await click('[data-action="save-privacy"]');
      expect(btn('[data-action="save-privacy"]').textContent).toMatch(/^Click again to delete older text \(\d+ corrections? in your Hall of fame\) for good$/);
    });

    it("shows a form error in place", async () => {
      await open();
      for (const r of root.querySelectorAll<HTMLInputElement>('input[name="level"]')) r.checked = false;
      await click('[data-action="save-privacy"]');
      expect(root.querySelector("#privacy-status")?.textContent).toBe("Pick what to keep first.");
      expect(api.setRetention).not.toHaveBeenCalled();
    });
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

describe("the status text's tooltip", () => {
  const text = () => {
    const b = root.querySelector<HTMLElement>(".sync-text");
    if (!b) throw new Error("no status text");
    return b;
  };

  const open = () => text().getAttribute("aria-expanded");

  it("starts closed, and a tap or a click toggles it", () => {
    expect(open()).toBe("false");
    text().click();
    expect(open()).toBe("true");
    text().click();
    expect(open()).toBe("false");
  });

  it("opens on Enter, even though focus alone already shows it", () => {
    text().focus();
    text().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    text().click(); // what the browser sends for Enter on a button
    expect(open()).toBe("true");
  });

  it("closes on Escape, on leaving it, and on a click elsewhere", () => {
    text().click();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(open()).toBe("false");
    text().click();
    text().dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    expect(open()).toBe("false");
    text().click();
    root.querySelector<HTMLElement>("h1, .wrap")?.click();
    expect(open()).toBe("false");
  });

  it("adds no listeners as it opens and closes", () => {
    const add = vi.spyOn(document, "addEventListener");
    for (let i = 0; i < 3; i++) text().click();
    expect(add).not.toHaveBeenCalled();
    add.mockRestore();
  });

  it("stays open through a repaint that changes the words", () => {
    text().click();
    state.setStatus({ syncing: false, last_sync: "2026-01-01T00:00:00Z", last_error: null, files_with_errors: 0, rebuilding: false });
    paintStatus();
    expect(open()).toBe("true");
  });

  it("is left alone by a repaint that changes nothing", () => {
    const pill = root.querySelector(".sync-pill");
    paintStatus();
    expect(root.querySelector(".sync-pill")).toBe(pill);
  });
});

describe("the log", () => {
  it("opens the log folder from General", async () => {
    state.setSettingsPane("general");
    state.setTab("settings");
    await render();
    await click('[data-action="open-log-folder"]');
    expect(api.openLogFolder).toHaveBeenCalledTimes(1);
  });

  it("writes a failed action to the log, and ignores the log failing too", async () => {
    const shout = vi.spyOn(console, "error").mockImplementation(() => {});
    api.logError.mockRejectedValueOnce("disk full");
    root.insertAdjacentHTML("beforeend", '<button id="bad" data-action="start-goal"></button>');
    await click("#bad");
    // The line carries the stack too.
    expect(api.logError).toHaveBeenCalledWith(expect.stringContaining("Pick a goal first"));
    shout.mockRestore();
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
    state.setStatus({ syncing: false, last_sync: null, last_error: "boom", files_with_errors: 0, rebuilding: false });
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

describe("the report card tip", () => {
  const TIP = "noactually.hint.report-cards";
  beforeEach(() => {
    vi.stubGlobal("localStorage", memoryStorage());
    api.progress.mockResolvedValue(progress());
    api.recentReportCards.mockResolvedValue([]);
    state.setTab("improve");
  });
  afterEach(() => vi.unstubAllGlobals());

  it("shows when a source that can carry a report card is allowed, and goes away for good when dismissed", async () => {
    await render();
    expect(root.querySelector(".card-hint")).not.toBeNull();
    await click('[data-action="dismiss-card-hint"]');
    expect(root.querySelector(".card-hint")).toBeNull();
    expect(localStorage.getItem(TIP)).toBe("1");
    await render();
    expect(root.querySelector(".card-hint")).toBeNull();
  });

  it("puts focus on the trends heading when the tip goes, instead of dropping it", async () => {
    await render();
    btn('[data-action="dismiss-card-hint"]').focus();
    await click('[data-action="dismiss-card-hint"]');
    const heading = root.querySelector("#trends-heading");
    expect(heading?.tagName).toBe("H2");
    expect(document.activeElement).toBe(heading);
  });

  it("stays away when no such source is allowed", async () => {
    api.sources.mockResolvedValue([source()]);
    await render();
    expect(root.querySelector(".card-hint")).toBeNull();
  });

  it("still draws Improve when the recent list can't be read", async () => {
    api.recentReportCards.mockRejectedValue(new Error("no such command"));
    await render();
    expect(root.querySelector(".improve-top")).not.toBeNull();
  });
});

describe("while the counts are rebuilt", () => {
  const rebuilding = () => state.setStatus({ syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: true });

  it("says so in the pill, and holds the scorecard with no numbers and nothing to share", async () => {
    rebuilding();
    await render();
    expect(root.querySelector(".sync-pill")?.textContent).toBe("Recounting your history…");
    expect(root.textContent).toContain("Recounting your history…");
    expect(root.querySelector(".card")).toBeNull();
    expect(root.querySelector('[data-action="open-share"], [data-action="share-prompts"], [data-action="share-corrections"]')).toBeNull();
    expect(api.stats).toHaveBeenCalledTimes(1); // the beforeEach draw only
  });

  it("brings the scorecard back when the rebuild is over", async () => {
    rebuilding();
    await render();
    state.setStatus({ syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false });
    await render();
    expect(root.querySelector(".card.hero")).not.toBeNull();
  });

  it("leaves the other tabs alone", async () => {
    rebuilding();
    await toTab("learn");
    expect(root.textContent).not.toContain("Recounting your history…".replace("Recounting your history…", "Recounting…x"));
    expect(root.querySelector("main#main")).not.toBeNull();
  });
});

describe("chats that couldn't be read", () => {
  it("shows how many in the pill, with a way to the log folder", async () => {
    state.setStatus({ syncing: false, last_sync: new Date().toISOString(), last_error: null, files_with_errors: 3, rebuilding: false });
    await render();
    const b = btn('.sync-pill [data-action="open-log-folder"]');
    expect(b.textContent).toBe("3 chats couldn't be read");
    await click('.sync-pill [data-action="open-log-folder"]');
    expect(api.openLogFolder).toHaveBeenCalledTimes(1);
  });

  it("says nothing when every chat was read", async () => {
    expect(root.querySelector('.sync-pill [data-action="open-log-folder"]')).toBeNull();
  });
});

describe("the update banner", () => {
  const update = (over = {}) => ({
    current: "0.5.4",
    auto: true,
    channel: "stable" as const,
    checking: false,
    installing: false,
    available: "0.6.0",
    last_check: null,
    last_error: null,
    ...over,
  });

  it("keeps a failed install's reason on the banner, even though the button was clicked on a banner that's gone", async () => {
    state.setUpdate(update());
    await render();
    api.installUpdate.mockImplementationOnce(async () => {
      // The banner is redrawn while the install runs (a status event), so the button is detached.
      const slot = root.querySelector("#update-slot");
      if (slot) slot.innerHTML = "";
      throw "disk full";
    });
    await click('[data-action="install-update"]');
    expect(root.querySelector("#update-slot")?.textContent).toContain("The last try failed: disk full");
    expect(root.querySelector('#update-slot [data-action="install-update"]')?.textContent).toBe("Try again");
  });

  it("shows the app's own reason for a failed install, beside an update that is still available", async () => {
    state.setUpdate(update({ last_error: "signature check failed" }));
    await render();
    expect(root.querySelector("#update-slot")?.textContent).toContain("The last try failed: signature check failed");
  });
});

describe("the rescored line", () => {
  const STAMP = "2026-11-01T10:00:00Z";

  it("shows once scores were recounted, and goes for good when dismissed", async () => {
    api.rescoredNotice.mockResolvedValue(STAMP);
    await render();
    expect(root.querySelector("#notice-slot")?.textContent).toContain("recounted your history");
    await click('[data-action="dismiss-rescored"]');
    expect(root.querySelector("#notice-slot")).toBeNull();
    await render();
    expect(root.querySelector("#notice-slot")).toBeNull();
  });

  it("comes back for a later rebuild", async () => {
    api.rescoredNotice.mockResolvedValue(STAMP);
    await render();
    await click('[data-action="dismiss-rescored"]');
    api.rescoredNotice.mockResolvedValue("2026-11-09T10:00:00Z");
    await render();
    expect(root.querySelector("#notice-slot")).not.toBeNull();
  });

  it("is quiet when the app can't say", async () => {
    api.rescoredNotice.mockRejectedValue("nope");
    await render();
    expect(root.querySelector("#notice-slot")).toBeNull();
  });
});

describe("General settings", () => {
  const general = async () => {
    state.setSettingsPane("general");
    await toTab("settings");
  };

  it("has a popups switch bound to the app's setting, on by default", async () => {
    await general();
    const box = root.querySelector<HTMLInputElement>("#popups");
    expect(box?.checked).toBe(true);
    if (!box) return;
    box.checked = false;
    box.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(api.setPopups).toHaveBeenCalledWith(false);
  });

  it("shows the switch off when the app says so, and puts it back when the app refuses", async () => {
    api.getPopups.mockResolvedValue(false);
    await general();
    const box = root.querySelector<HTMLInputElement>("#popups");
    expect(box?.checked).toBe(false);
    if (!box) return;
    api.setPopups.mockRejectedValueOnce("nope");
    box.checked = true;
    box.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(box.checked).toBe(false);
  });

  it("opens the licenses, and shows why when this build has none", async () => {
    await general();
    await click('[data-action="open-notices"]');
    expect(api.openNotices).toHaveBeenCalledTimes(1);
    api.openNotices.mockRejectedValueOnce("The notices file isn't part of this build.");
    await click('[data-action="open-notices"]');
    expect(root.textContent).toContain("The notices file isn't part of this build.");
  });
});

describe("moving around", () => {
  it("wraps each page in main, behind a skip link that moves focus there", async () => {
    const main = root.querySelector("main#main");
    expect(main).not.toBeNull();
    await click('[data-action="skip-to-main"]');
    expect(document.activeElement).toBe(main);
  });

  it("announces a move to another page, and not a redraw of the same one", async () => {
    document.body.insertAdjacentHTML("beforeend", '<div id="announce" role="status"></div>');
    await toTab("improve");
    expect(document.getElementById("announce")?.textContent).toBe("Improve");
    await render();
    await click('[data-action="tab"][data-id="settings"]');
    expect(document.getElementById("announce")?.textContent).toMatch(/^Settings, /);
    document.getElementById("announce")?.remove();
  });

  it("logs a page that failed to draw, with the stack", async () => {
    const shout = vi.spyOn(console, "error").mockImplementation(() => {});
    api.stats.mockRejectedValueOnce(new Error("database is locked"));
    await render();
    expect(api.logError).toHaveBeenCalledWith(expect.stringContaining("database is locked"));
    shout.mockRestore();
  });
});

describe("importing an export", () => {
  it("shows the app's plain reason when the file is refused", async () => {
    api.sources.mockResolvedValue([source({ id: "chatgpt-export", name: "ChatGPT (export)", kind: "import", roots: [] })]);
    api.importExport.mockRejectedValueOnce("That doesn't look like a ChatGPT export. Choose its conversations.json.");
    state.setTab("settings");
    state.setSettingsPane("sources");
    await render();
    await click('[data-action="import"]');
    expect(root.querySelector(".action-error")?.textContent).toBe("Couldn't do that: That doesn't look like a ChatGPT export. Choose its conversations.json.");
  });
});
