// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReportCardsStatus, ReportCardToolStatus, SourceInfo } from "./api";
import { must } from "./dom";
import { CODEX_TRUST, confirmLines, doneLine, offLine } from "./report-cards";
import { meta, source, stats } from "./test/fixtures";

const api = vi.hoisted(() => ({
  meta: vi.fn(),
  stats: vi.fn(),
  sources: vi.fn(),
  retention: vi.fn(),
  getAutostart: vi.fn(),
  getMenuBar: vi.fn(),
  reportCardsStatus: vi.fn(),
  reportCardsTurnOn: vi.fn(),
  reportCardsTurnOff: vi.fn(),
  reportCardsSetTips: vi.fn(),
  reportCardsSetReminders: vi.fn(),
  reportCardsDeleteHistory: vi.fn(),
}));
vi.mock("./api", async (importOriginal) => ({ ...(await importOriginal<typeof import("./api")>()), api, demo: false }));

let render: () => Promise<void>;
let root: HTMLElement;
let state: typeof import("./state");

beforeAll(async () => {
  document.body.innerHTML = `<div id="app"></div>`;
  const r = await import("./render");
  render = r.render;
  root = r.root;
  state = await import("./state");
  (await import("./actions")).bindActions(root);
});

const tool = (over: Partial<ReportCardToolStatus> = {}): ReportCardToolStatus => ({
  installed_tool: true,
  on: false,
  last_card_day: null,
  problem: null,
  left_out: 0,
  unavailable: null,
  ...over,
});
const status = (over: Partial<ReportCardsStatus> = {}): ReportCardsStatus => ({
  claude_code: tool({ on: true, last_card_day: "2026-10-29" }),
  codex: tool(),
  tip_activity: { active: 0, tips: 0, blocked: false },
  tips: false,
  reminders: true,
  translocated: false,
  ...over,
});
/** A source the app is allowed to read. */
const allowed = (id: string): SourceInfo =>
  source({ id, state: { id, allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] } });

beforeEach(async () => {
  state.resetState();
  for (const d of document.querySelectorAll("dialog")) d.remove();
  api.meta.mockResolvedValue(meta);
  api.stats.mockResolvedValue(stats());
  api.sources.mockResolvedValue([allowed("claude-code"), allowed("codex")]);
  api.retention.mockResolvedValue({ level: "snippet", full_days: 30, snippet_days: 365 });
  api.getAutostart.mockResolvedValue(false);
  api.getMenuBar.mockResolvedValue({ enabled: true, available: true });
  api.reportCardsStatus.mockResolvedValue(status());
  api.reportCardsTurnOn.mockResolvedValue(null);
  api.reportCardsTurnOff.mockResolvedValue(null);
  api.reportCardsSetTips.mockImplementation(async (on: boolean) => {
    api.reportCardsStatus.mockResolvedValue(status({ tips: on }));
    return null;
  });
  api.reportCardsSetReminders.mockResolvedValue(null);
  api.reportCardsDeleteHistory.mockResolvedValue(3);
  await render();
});
afterEach(() => {
  vi.clearAllMocks();
  document.getElementById("confirm-note")?.remove();
});

const settle = () => new Promise((r) => setTimeout(r, 0));
const btn = (sel: string, within: ParentNode = root) => must<HTMLButtonElement>(sel, within);
const click = async (sel: string, within: ParentNode = root) => {
  btn(sel, within).click();
  await settle();
};
/** Opens Settings → Report cards and lets the status come in. */
const open = async () => {
  await click('[data-action="tab"][data-id="settings"]');
  await click('[data-action="settings-pane"][data-id="report-cards"]');
  await settle();
};
const section = () => must("#report-cards").textContent ?? "";
const tools = (id: string) => `[data-action="report-cards-tool"][data-id="${id}"]`;
const dialog = () => must<HTMLDialogElement>("dialog.confirm-dialog");
/** What the hidden status line has announced. */
const announced = () => document.getElementById("confirm-note")?.textContent ?? "";
/** A promise the test settles by hand, to look at the screen while a command is running. */
const pending = <T>() => {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

describe("opening the section", () => {
  it("reads where things stand once, and shows it", async () => {
    await open();
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(1);
    expect(section()).toContain("Claude Code");
    expect(section()).toContain("On · last report card Oct 29");
    expect(section()).toContain("Codex");
    expect(btn(tools("codex")).textContent).toBe("Turn on");
    expect(btn(tools("claude_code")).textContent).toBe("Turn off");
  });

  it("shows the section first, then fills it in when the answer arrives", async () => {
    const answer = pending<ReportCardsStatus>();
    api.reportCardsStatus.mockReturnValueOnce(answer.promise);
    await open();
    expect(section()).toContain("Checking Claude Code and Codex…");
    expect(root.querySelector(tools("codex"))).toBeNull();
    answer.resolve(status());
    await settle();
    expect(section()).not.toContain("Checking");
    expect(root.querySelector(tools("codex"))).not.toBeNull();
  });

  it("doesn't ask again on every redraw, only when the section is opened again", async () => {
    await open();
    await render();
    await render();
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(1);
    await click('[data-action="settings-pane"][data-id="general"]');
    await click('[data-action="settings-pane"][data-id="report-cards"]');
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(2);
    await click('[data-action="tab"][data-id="score"]');
    await click('[data-action="tab"][data-id="settings"]');
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(3);
  });

  it("doesn't ask at all while another section is showing", async () => {
    await click('[data-action="tab"][data-id="settings"]');
    expect(api.reportCardsStatus).not.toHaveBeenCalled();
  });

  it("says when it couldn't check, and tries again on request", async () => {
    api.reportCardsStatus.mockRejectedValueOnce("Something went wrong. Try again.");
    await open();
    expect(must("#report-cards .action-error").textContent).toBe("Something went wrong. Try again.");
    await click('[data-action="report-cards-retry"]');
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(2);
    expect(section()).not.toContain("Something went wrong");
    expect(section()).toContain("On · last report card Oct 29");
  });
});

describe("a tool that wasn't found", () => {
  it("is searched for harder when the person asks, and found", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ codex: tool({ installed_tool: false }) }));
    await open();
    expect(section()).toContain("Not found on this computer.");
    expect(api.reportCardsStatus).toHaveBeenLastCalledWith(false);
    api.reportCardsStatus.mockResolvedValueOnce(status());
    await click('[data-action="report-cards-retry"][data-id="look"]');
    expect(api.reportCardsStatus).toHaveBeenLastCalledWith(true);
    expect(section()).not.toContain("Not found on this computer.");
    expect(btn(tools("codex")).textContent).toBe("Turn on");
  });
});

describe("a tool whose source isn't allowed", () => {
  it("says to allow it first, and the button goes to Sources", async () => {
    api.sources.mockResolvedValue([allowed("codex")]);
    api.reportCardsStatus.mockResolvedValueOnce(status({ claude_code: tool() }));
    await open();
    expect(section()).toContain("Allow Claude Code in Sources first, so the app can read your report cards.");
    expect(root.querySelector(tools("claude_code"))).toBeNull();
    await click('[data-action="go-settings"][data-id="sources"]');
    expect(state.state.tab).toBe("settings");
    expect(state.state.settingsPane).toBe("sources");
    expect(root.querySelector("#report-cards")).toBeNull();
  });
});

describe("turning on", () => {
  it("asks first, with the plan's words, and does nothing if you cancel", async () => {
    await open();
    await click(tools("codex"));
    const d = dialog();
    expect(d.open).toBe(true);
    expect(must("h2", d).textContent).toBe("Turn on report cards in Codex?");
    expect([...d.querySelectorAll("li")].map((li) => li.textContent)).toEqual(confirmLines("codex"));
    expect(d.textContent).toContain(CODEX_TRUST);
    expect(document.activeElement?.textContent).toBe("Turn on");
    await click('[data-confirm-dialog="no"]', d);
    expect(document.querySelector("dialog")).toBeNull();
    expect(api.reportCardsTurnOn).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(root.querySelector(tools("codex")));
  });

  it("is named by its heading and described by its list, so a screen reader reads what will happen", async () => {
    await open();
    await click(tools("codex"));
    const d = dialog();
    expect(d.getAttribute("aria-labelledby")).toBe("confirm-title");
    expect(d.getAttribute("aria-describedby")).toBe("confirm-list");
    expect(must("#confirm-list", d).tagName).toBe("UL");
    expect(must("#confirm-list", d).querySelectorAll("li")).toHaveLength(confirmLines("codex").length);
  });

  it("Esc cancels too", async () => {
    await open();
    await click(tools("codex"));
    dialog().dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(document.querySelector("dialog")).toBeNull();
    expect(api.reportCardsTurnOn).not.toHaveBeenCalled();
  });

  it("applies a redraw that waited for it when it closes, however it closes", async () => {
    const { scheduler } = await import("./render");
    for (const close of ['[data-confirm-dialog="no"]', '[data-confirm-dialog="yes"]']) {
      await open();
      await click(tools("codex"));
      scheduler.refresh(); // a background update arrives while the screen is open
      expect(scheduler.deferred).toBe(true);
      await click(close, dialog());
      expect(scheduler.deferred, close).toBe(false);
      await settle();
    }
  });

  it("has the Claude Code words, which say where the plugin comes from and what is read", () => {
    expect(confirmLines("claude_code")).toEqual([
      "Adds a small No, Actually... plugin to Claude Code, downloaded from GitHub. It updates when the app does.",
      "A small check runs on this computer after each reply. It uses no AI and sends nothing.",
      "Report cards are written by your AI, so they use your tokens: only when you ask, plus a little in each chat if you turn on tips.",
      "Shows a one-line reminder when a task ends (no tokens). You can turn that off.",
      "When you ask for a report card, your AI reads a short summary of the chat that No, Actually... makes on your computer. No, Actually... itself sends nothing anywhere.",
    ]);
  });

  it("has the Codex words: files, not a download, no tips yet, and the one thing Codex asks", () => {
    const lines = confirmLines("codex");
    expect(lines[0]).toBe("Adds two small files Codex uses to run report cards. Your Codex settings are backed up first.");
    expect(lines).toContain("Report cards are written by your AI, so they use your tokens: only when you ask. Tips are Claude Code only for now.");
    expect(lines.at(-1)).toBe("Codex will ask you to trust this now, and again after app updates. Say yes.");
    expect(lines.at(-1)).toBe(CODEX_TRUST);
    expect(lines.join(" ")).not.toMatch(/GitHub|say thanks if/);
    expect(lines).toHaveLength(6);
  });

  it("shows it working, then reads again and shows it on", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ claude_code: tool() }));
    await open();
    const work = pending<null>();
    api.reportCardsTurnOn.mockReturnValueOnce(work.promise);
    await click(tools("claude_code"));
    await click('[data-confirm-dialog="yes"]', dialog());
    expect(api.reportCardsTurnOn).toHaveBeenCalledWith("claude_code");
    const b = btn(tools("claude_code"));
    expect(b.textContent).toBe("Turning on…");
    expect(b.getAttribute("aria-disabled")).toBe("true");
    expect(root.ownerDocument.activeElement).toBe(b); // still where the keyboard was
    api.reportCardsStatus.mockResolvedValueOnce(status({ claude_code: tool({ on: true }) }));
    work.resolve(null);
    await settle();
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(2);
    expect(btn(tools("claude_code")).textContent).toBe("Turn off");
    expect(section()).toContain("No report cards yet. Type /grade:me after your next task.");
    expect(document.activeElement).toBe(root.querySelector(tools("claude_code")));
  });

  it("says what to do next, on screen beside the tool and once to a screen reader", async () => {
    await open();
    await click(tools("codex"));
    await click('[data-confirm-dialog="yes"]', dialog());
    const text = "Done. Start a new Codex chat, finish a task, then type $grade-me.";
    expect(text).toBe(doneLine("codex"));
    expect(must(tools("codex")).closest(".source")?.querySelector(".action-note")?.textContent).toBe(text);
    expect(announced()).toBe(text);
    await click(tools("claude_code")); // Turn off: the line for Codex goes
    expect(section()).not.toContain("Done. Start a new Codex chat");
    expect(doneLine("claude_code")).toBe("Done. Start a new Claude Code chat, finish a task, then type /grade:me.");
  });

  it("ignores a second click while something is running", async () => {
    api.reportCardsStatus.mockResolvedValue(status({ claude_code: tool(), codex: tool() }));
    await open();
    const work = pending<null>();
    api.reportCardsTurnOn.mockReturnValueOnce(work.promise);
    await click(tools("claude_code"));
    await click('[data-confirm-dialog="yes"]', dialog());
    await click(tools("codex"));
    await click(tools("claude_code"));
    expect(document.querySelector("dialog")).toBeNull();
    expect(api.reportCardsTurnOn).toHaveBeenCalledTimes(1);
    work.resolve(null);
    await settle();
  });

  it("asks the app to turn on the tool that was picked", async () => {
    await open();
    await click(tools("codex"));
    await click('[data-confirm-dialog="yes"]', dialog());
    expect(api.reportCardsTurnOn).toHaveBeenCalledWith("codex");
  });

  it("shows a failure in plain words beside the button, reads again anyway, and clears it on the next try", async () => {
    await open();
    const why = "Couldn't reach GitHub to download the plugin. Check your connection and try again.";
    api.reportCardsTurnOn.mockRejectedValueOnce(why);
    await click(tools("codex"));
    await click('[data-confirm-dialog="yes"]', dialog());
    const shown = must("#report-cards .action-error");
    expect(shown.textContent).toBe(why);
    expect(shown.closest(".source")?.textContent).toContain("Codex");
    expect(announced()).toBe(why);
    expect(root.querySelector('#report-cards [role="alert"]')).toBeNull();
    expect(btn(tools("codex")).textContent).toBe("Turn on");
    expect(btn(tools("codex")).getAttribute("aria-disabled")).toBeNull();
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(2);
    await click(tools("codex"));
    await click('[data-confirm-dialog="yes"]', dialog());
    expect(root.querySelector("#report-cards .action-error")).toBeNull();
  });
});

describe("turning off", () => {
  it("just does it, then reads again", async () => {
    await open();
    api.reportCardsStatus.mockResolvedValueOnce(status({ claude_code: tool() }));
    await click(tools("claude_code"));
    expect(document.querySelector("dialog")).toBeNull();
    expect(api.reportCardsTurnOff).toHaveBeenCalledWith("claude_code");
    expect(api.reportCardsStatus).toHaveBeenCalledTimes(2);
    expect(btn(tools("claude_code")).textContent).toBe("Turn on");
    expect(section()).not.toContain("On · last report card");
    expect(must(tools("claude_code")).closest(".source")?.querySelector(".action-note")?.textContent).toBe("Report cards are off in Claude Code.");
    expect(announced()).toBe(offLine("claude_code"));
  });

  it("tells you to stop tips in chats already open, only when Claude Code had them on", async () => {
    const note = () => must(tools("claude_code")).closest(".source")?.querySelector(".action-note")?.textContent;
    api.reportCardsStatus.mockResolvedValue(status({ tips: true }));
    await open();
    api.reportCardsStatus.mockResolvedValueOnce(status({ claude_code: tool(), tips: false }));
    await click(tools("claude_code"));
    expect(note()).toBe("Report cards are off in Claude Code. Chats already open keep their tips. Tell your AI “no more tips” in those.");
    expect(offLine("claude_code", true)).toBe(note());
    expect(offLine("codex", true)).toBe("Report cards are off in Codex.");
    expect(offLine("claude_code")).toBe("Report cards are off in Claude Code.");
  });

  it("shows why it couldn't", async () => {
    await open();
    api.reportCardsTurnOff.mockRejectedValueOnce("Claude Code couldn't remove the plugin. Try again in a moment.");
    await click(tools("claude_code"));
    expect(must("#report-cards .action-error").textContent).toBe("Claude Code couldn't remove the plugin. Try again in a moment.");
    expect(announced()).toBe("Claude Code couldn't remove the plugin. Try again in a moment.");
    expect(btn(tools("claude_code")).textContent).toBe("Turn off");
  });
});

describe("tips", () => {
  const box = () => must<HTMLInputElement>("#report-cards-tips");
  const tick = async (on: boolean) => {
    box().checked = on;
    box().dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
  };

  it("shows the saved choice, and the token note without a number", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ tips: true }));
    await open();
    expect(box().checked).toBe(true);
    expect(section()).toContain("Give me tips as I work (Claude Code only)");
    expect(section()).toContain(
      "When a follow-up could have been in your first message, your AI adds a one-line tip, and No, Actually... keeps count. Adds about 480 tokens when a chat starts and again after a long chat is compacted, about 2.5% more tokens overall in testing.",
    );
  });

  it("reads the status again after a change, so the state line can't go stale", async () => {
    await open();
    const reads = api.reportCardsStatus.mock.calls.length;
    await tick(true);
    expect(api.reportCardsStatus.mock.calls.length).toBe(reads + 1);
  });

  it("stays reachable before Claude Code is on, and says why it won't tick", async () => {
    api.reportCardsStatus.mockResolvedValue(status({ claude_code: tool({ on: false }) }));
    await open();
    expect(box().disabled).toBe(false);
    expect(box().getAttribute("aria-disabled")).toBe("true");
    await tick(true);
    expect(api.reportCardsSetTips).not.toHaveBeenCalled();
    expect(box().checked).toBe(false);
    expect(announced()).toBe("Turn on report cards in Claude Code first.");
  });

  it("turns on and off, keeping the box where the keyboard was", async () => {
    await open();
    box().focus();
    await tick(true);
    expect(api.reportCardsSetTips).toHaveBeenLastCalledWith(true);
    expect(box().checked).toBe(true);
    expect(document.activeElement).toBe(box());
    await tick(false);
    expect(api.reportCardsSetTips).toHaveBeenLastCalledWith(false);
    expect(box().checked).toBe(false);
  });

  it("puts the box back and says why, once, when the app can't", async () => {
    await open();
    api.reportCardsSetTips.mockRejectedValueOnce("Couldn't turn that on. Try again.");
    await tick(true);
    expect(box().checked).toBe(false);
    expect(must("#report-cards-tips").closest(".settings-row")?.querySelector(".action-error")?.textContent).toBe("Couldn't turn that on. Try again.");
    expect(announced()).toBe("Couldn't turn that on. Try again.");
    await tick(true);
    expect(box().checked).toBe(true);
    expect(root.querySelector("#report-cards .action-error")).toBeNull();
  });

  it("waits for Claude Code to be on", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ claude_code: tool() }));
    await open();
    expect(box().getAttribute("aria-disabled")).toBe("true");
    expect(section()).toContain("Turn on report cards in Claude Code first.");
  });

  it("can be turned off when Claude Code is off", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ tips: true, claude_code: tool() }));
    api.reportCardsSetTips.mockImplementationOnce(async () => {
      api.reportCardsStatus.mockResolvedValue(status({ claude_code: tool() }));
      return null;
    });
    await open();
    expect(box().checked).toBe(true);
    expect(box().getAttribute("aria-disabled")).toBeNull();
    await tick(false);
    expect(api.reportCardsSetTips).toHaveBeenLastCalledWith(false);
    expect(box().checked).toBe(false);
    expect(box().getAttribute("aria-disabled")).toBe("true");
    expect(section()).toContain("Turn on report cards in Claude Code first.");
  });

  it("can be turned off while a report card command runs", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ tips: true }));
    await open();
    const work = pending<null>();
    api.reportCardsTurnOff.mockReturnValueOnce(work.promise);
    await click(tools("claude_code"));
    expect(box().getAttribute("aria-disabled")).toBeNull();
    await tick(false);
    expect(api.reportCardsSetTips).toHaveBeenLastCalledWith(false);
    work.resolve(null);
    await settle();
    expect(box().checked).toBe(false);
  });

  it("holds still while a report card command runs, and comes back after", async () => {
    await open();
    const work = pending<null>();
    api.reportCardsTurnOn.mockReturnValueOnce(work.promise);
    await click(tools("codex"));
    await click('[data-confirm-dialog="yes"]', dialog());
    expect(box().getAttribute("aria-disabled")).toBe("true");
    work.resolve(null);
    await settle();
    expect(box().disabled).toBe(false);
  });
});

describe("reminders", () => {
  const box = () => must<HTMLInputElement>("#report-cards-reminders");
  const tick = async (on: boolean) => {
    box().checked = on;
    box().dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
  };

  it("are on by default, and say they use no tokens", async () => {
    await open();
    expect(box().checked).toBe(true);
    expect(section()).toContain("Remind me after a task (uses no tokens)");
  });

  it("show the saved choice", async () => {
    api.reportCardsStatus.mockResolvedValueOnce(status({ reminders: false }));
    await open();
    expect(box().checked).toBe(false);
  });

  it("turn off and on, keeping the box where the keyboard was", async () => {
    await open();
    box().focus();
    await tick(false);
    expect(api.reportCardsSetReminders).toHaveBeenLastCalledWith(false);
    expect(box().checked).toBe(false);
    expect(document.activeElement).toBe(box());
    await tick(true);
    expect(api.reportCardsSetReminders).toHaveBeenLastCalledWith(true);
    expect(box().checked).toBe(true);
    expect(api.reportCardsSetTips).not.toHaveBeenCalled();
  });

  it("go back and say why, once, when the app can't", async () => {
    await open();
    api.reportCardsSetReminders.mockRejectedValueOnce("Couldn't turn that off. Try again.");
    await tick(false);
    expect(box().checked).toBe(true);
    expect(box().closest(".settings-row")?.querySelector(".action-error")?.textContent).toBe("Couldn't turn that off. Try again.");
    expect(announced()).toBe("Couldn't turn that off. Try again.");
  });
});

describe("deleting the history", () => {
  const del = '[data-action="report-cards-delete"]';
  // A second click within 400 ms is a double-click and does nothing: hold the clock and move it on.
  beforeEach(() => vi.useFakeTimers({ toFake: ["Date"] }));
  afterEach(() => vi.useRealTimers());
  const again = async () => {
    vi.setSystemTime(Date.now() + 500);
    await click(del);
  };

  it("takes a second click, like Forget", async () => {
    await open();
    await click(del);
    expect(api.reportCardsDeleteHistory).not.toHaveBeenCalled();
    expect(btn(del).textContent).toBe("Click again to delete every report card and tip for good");
    await again();
    expect(api.reportCardsDeleteHistory).toHaveBeenCalledTimes(1);
    expect(must("#report-cards-note").textContent).toBe("Deleted your report card and tip history.");
    expect(btn(del).textContent).toBe("Delete report card and tip history");
  });

  it("says so when there was nothing to delete", async () => {
    await open();
    api.reportCardsDeleteHistory.mockResolvedValueOnce(0);
    await click(del);
    await again();
    expect(must("#report-cards-note").textContent).toBe("There was no history to delete.");
    api.reportCardsDeleteHistory.mockResolvedValueOnce(1);
    await click(del);
    await again();
    expect(must("#report-cards-note").textContent).toBe("Deleted your report card and tip history.");
  });

  it("shows a failure beside the button and no success note", async () => {
    await open();
    api.reportCardsDeleteHistory.mockRejectedValueOnce("Couldn't delete the report card history. Try again.");
    await click(del);
    await again();
    expect(must("#report-cards .action-error").textContent).toBe("Couldn't delete the report card history. Try again.");
    expect(must("#report-cards-note").textContent).toBe("");
  });

  it("shows it working", async () => {
    await open();
    const work = pending<number>();
    api.reportCardsDeleteHistory.mockReturnValueOnce(work.promise);
    await click(del);
    await again();
    expect(btn(del).textContent).toBe("Deleting…");
    expect(btn(del).getAttribute("aria-disabled")).toBe("true");
    work.resolve(2);
    await settle();
    expect(btn(del).textContent).toBe("Delete report card and tip history");
  });

  it("ignores a double-click", async () => {
    await open();
    await click(del);
    await click(del);
    expect(api.reportCardsDeleteHistory).not.toHaveBeenCalled();
    await again();
    expect(api.reportCardsDeleteHistory).toHaveBeenCalledTimes(1);
  });
});
