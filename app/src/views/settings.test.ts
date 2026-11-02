// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReportCardsStatus, ReportCardToolStatus, SourceInfo } from "../api";
import { emptyReportCards, type ReportCardsState } from "../state";
import { source } from "../test/fixtures";
import { settingsView } from "./settings";

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
  claude_code: tool(),
  codex: tool(),
  tips: false,
  tip_activity: { active: 0, tips: 0, blocked: false },
  reminders: true,
  translocated: false,
  ...over,
});

/** A source the app is allowed to read. */
const allowed = (id: string, over: Partial<SourceInfo> = {}): SourceInfo =>
  source({ id, state: { id, allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] }, ...over });
const BOTH = [allowed("claude-code"), allowed("codex")];

const view = (
  cards: Partial<ReportCardsState> = {},
  pane: "report-cards" | "general" | "privacy" = "report-cards",
  sources: SourceInfo[] = BOTH,
  demo = false,
  deleteAll = { busy: false, error: "" },
) =>
  settingsView({
    pane,
    sources,
    status: { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false },
    retention: { level: "snippet", full_days: 30, snippet_days: 365 },
    autostart: false,
    menuBar: { enabled: true, available: true },
    reportCards: { ...emptyReportCards(), ...cards },
    deleteAll,
    tipRules: "Be <kind> to them.",
    update: null,
    demo,
  }).value;

/** The markup as the page would hold it, to ask for things in instead of matching text. */
const dom = (markup: string): HTMLElement => {
  const el = document.createElement("div");
  el.innerHTML = markup;
  return el;
};

/** The visible words under `node`, text pieces apart by a space, so the copy can be checked as people read it. */
const words = (node: Node): string => {
  const out: string[] = [];
  const walk = (n: Node) => {
    if (n.nodeType === 3) out.push(n.nodeValue ?? "");
    else for (const child of n.childNodes) walk(child);
  };
  walk(node);
  return out
    .join(" ")
    .replace(/\s+/g, " ")
    .replace(/ ([.,:)])/g, "$1")
    .trim();
};

/** The row for one tool, by the name it shows. */
const row = (markup: string, name: string): HTMLElement => {
  const found = [...dom(markup).querySelectorAll<HTMLElement>(".source")].find((r) => r.querySelector("b")?.textContent === name);
  if (!found) throw new Error(`no row for ${name}`);
  return found;
};
const button = (el: ParentNode, sel = "button") => el.querySelector<HTMLButtonElement>(sel);

afterEach(() => vi.restoreAllMocks());

describe("Settings → Report cards", () => {
  it("is a section of its own, named in the tabs, that other views can link to", () => {
    const m = view({ status: status() });
    expect(m).toContain('<section id="report-cards">');
    expect(m).toContain('data-action="settings-pane" data-id="report-cards" data-tip="Report cards" aria-current="page"');
    expect(m).toContain('<h2 class="sr-only">Report cards</h2>');
    expect(view({ status: status() }, "general")).not.toContain('<section id="report-cards">');
  });

  it("says grades can vary and what a card costs", () => {
    const text = words(dom(view({ status: status() })));
    expect(text).toContain("can vary between runs, unlike your score");
    expect(text).toContain("Claude Haiku misses more tips");
    expect(text).toContain("0.5k to 2k output tokens per task graded");
  });

  it("says what it is, and how to use it", () => {
    const text = words(dom(view({ status: status() })));
    expect(text).toContain(
      "Get a letter grade (A–F) on how you asked for each task, right inside Claude Code or Codex, with advice on getting it right the first time.",
    );
    expect(text).toContain("How to use it: when you finish something, type /grade:me in Claude Code (or $grade-me in Codex).");
    expect(text).toContain("Delete report card and tip history");
  });

  it("says to move the app out of a temporary copy, and only then", () => {
    const move = "Move No, Actually... to your Applications folder and reopen it, so report cards keep working when the app is closed.";
    const moved = dom(view({ status: status({ translocated: true }) }));
    expect(moved.querySelector("#report-cards-move-app")?.textContent).toBe(move);
    expect(words(moved)).toContain(move);
    expect(view({ status: status() })).not.toContain("Applications folder");
    expect(view({}, "report-cards")).not.toContain("Applications folder");
  });

  it("offers Turn on next to a tool that is installed, allowed and off", () => {
    const r = row(view({ status: status() }), "Claude Code");
    const b = button(r);
    expect(b?.textContent).toBe("Turn on");
    expect(b?.dataset).toMatchObject({ action: "report-cards-tool", id: "claude_code", on: "false" });
    expect(b?.getAttribute("aria-label")).toBe("Turn on report cards in Claude Code");
    expect(r.classList.contains("on")).toBe(false);
    expect(words(r)).toBe("Claude Code Turn on");
  });

  it("shows a tool that is on with its latest report card, and Turn off", () => {
    const r = row(view({ status: status({ claude_code: tool({ on: true, last_card_day: "2026-10-29" }) }) }), "Claude Code");
    expect(r.classList.contains("on")).toBe(true);
    expect(words(r)).toBe("Claude Code On · last report card Oct 29 Turn off");
    const b = button(r);
    expect(b?.classList.contains("ghost")).toBe(true);
    expect(b?.dataset).toMatchObject({ action: "report-cards-tool", id: "claude_code", on: "true" });
    expect(b?.getAttribute("aria-label")).toBe("Turn off report cards in Claude Code");
  });

  it("tells a tool that is on but has no card yet what to type, in that tool's words", () => {
    const m = view({ status: status({ claude_code: tool({ on: true }), codex: tool({ on: true }) }) });
    expect(words(row(m, "Claude Code"))).toContain("No report cards yet. Type /grade:me after your next task.");
    expect(words(row(m, "Codex"))).toContain("No report cards yet. Type $grade-me after your next task.");
  });

  it("says Not found on this computer, and what does work, for a tool that isn't there", () => {
    const r = row(view({ status: status({ codex: tool({ installed_tool: false }) }) }), "Codex");
    expect(words(r)).toBe(
      "Codex Not found on this computer. Install Codex, then press Look again. Report cards work in Claude Code and Codex, including the Code tab in the Claude desktop app, not in regular Claude or ChatGPT chats. Look again",
    );
    expect(r.querySelectorAll(".src-main .muted.small")).toHaveLength(2);
    expect(r.textContent).not.toContain("Not installed");
    expect(button(r, "[data-action='report-cards-tool']")).toBeNull();
  });

  it("tells Claude Code that isn't found where it comes from, including the Claude desktop app on a Mac, and Codex only about Codex", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15");
    const m = view({ status: status({ claude_code: tool({ installed_tool: false }), codex: tool({ installed_tool: false }) }) });
    expect(words(row(m, "Claude Code"))).toBe(
      "Claude Code Not found on this computer. Install Claude Code, or open the Claude desktop app once, then press Look again. Report cards work in Claude Code and Codex, including the Code tab in the Claude desktop app, not in regular Claude or ChatGPT chats. Look again",
    );
    expect(words(row(m, "Codex"))).not.toContain("once");
    expect(words(row(m, "Codex"))).toContain("Install Codex, then press Look again.");
    expect(words(row(m, "Claude Code"))).not.toContain("Install Codex");
  });

  it("doesn't send anyone but a Mac to open the Claude desktop app once", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    const m = view({ status: status({ claude_code: tool({ installed_tool: false }) }) });
    expect(words(row(m, "Claude Code"))).toContain("Not found on this computer. Install Claude Code, then press Look again.");
    expect(words(row(m, "Claude Code"))).not.toContain("open the Claude desktop app once");
  });

  it("says when a tool is on but its source isn't being read, and only then", () => {
    const paused = "Paused: report cards won't run while No, Actually... isn't reading Claude Code.";
    const on = status({ claude_code: tool({ on: true }) });
    expect(words(row(view({ status: on }), "Claude Code"))).not.toContain("Paused");
    expect(words(row(view({ status: on }, "report-cards", []), "Claude Code"))).toContain(paused);
    const stopped = allowed("claude-code", {
      state: { id: "claude-code", allowed: false, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] },
    });
    expect(words(row(view({ status: on }, "report-cards", [stopped]), "Claude Code"))).toContain(paused);
    const pausedSource = allowed("claude-code", {
      state: { id: "claude-code", allowed: true, paused: true, backfill_since: null, granted_at: null, start_after: null, paths: [] },
    });
    expect(words(row(view({ status: on }, "report-cards", [pausedSource]), "Claude Code"))).toContain(paused);
    // Off, there is nothing to pause.
    expect(words(row(view({ status: status() }, "report-cards", []), "Claude Code"))).not.toContain("Paused");
  });

  it("says how many report cards couldn't be counted, and points to the log", () => {
    const one = row(view({ status: status({ claude_code: tool({ on: true, left_out: 1 }) }) }), "Claude Code");
    expect(words(one)).toContain("1 report card couldn't be counted. See the log for why.");
    const many = row(view({ status: status({ codex: tool({ left_out: 3 }) }) }), "Codex");
    expect(words(many)).toContain("3 report cards couldn't be counted. See the log for why.");
    expect(words(row(view({ status: status() }), "Claude Code"))).not.toContain("couldn't be counted");
  });

  it("has a Show log folder button in General, but not in the demo", () => {
    const b = button(dom(view({ status: status() }, "general")), "[data-action='open-log-folder']");
    expect(b?.textContent).toBe("Show log folder");
    expect(button(dom(view({ status: status() }, "general", BOTH, true)), "[data-action='open-log-folder']")).toBeNull();
  });

  it("offers to look again for a tool that wasn't found, since the quick search may have missed it", () => {
    const r = row(view({ status: status({ claude_code: tool({ installed_tool: false }) }) }), "Claude Code");
    const b = button(r);
    expect(b?.textContent).toBe("Look again");
    expect(b?.dataset).toMatchObject({ action: "report-cards-retry", id: "look" });
  });

  it("says why report cards can't be turned on here where Turn on would be, and offers Look again only where something can change", () => {
    const git = "Report cards need Git for Windows, which Claude Code uses to run commands. Install it, or set CLAUDE_CODE_GIT_BASH_PATH, then try again.";
    const nope = "Report cards for Codex aren't available on Windows yet.";
    const m = view({ status: status({ claude_code: tool({ unavailable: git }), codex: tool({ unavailable: nope }) }) });
    const claude = row(m, "Claude Code");
    expect(words(claude)).toBe(`Claude Code ${git} Look again`);
    expect(button(claude)?.dataset).toMatchObject({ action: "report-cards-retry", id: "look" });
    expect(button(claude, "[data-action='report-cards-tool']")).toBeNull();
    expect(words(row(m, "Codex"))).toBe(`Codex ${nope}`);
    expect(row(m, "Codex").querySelector("button")).toBeNull();
    // It comes before asking for the source: allowing it would not help.
    const noSources = view({ status: status({ codex: tool({ unavailable: nope }) }) }, "report-cards", []);
    expect(words(row(noSources, "Codex"))).toBe(`Codex ${nope}`);
    // A tool that is on can always be turned off, and a tool that isn't there is just not found.
    const on = view({ status: status({ claude_code: tool({ on: true, unavailable: git }) }) });
    expect(button(row(on, "Claude Code"))?.textContent).toBe("Turn off");
    expect(words(row(on, "Claude Code"))).not.toContain("Git");
    const missing = view({ status: status({ claude_code: tool({ installed_tool: false, unavailable: git }) }) });
    expect(words(row(missing, "Claude Code"))).toContain("Not found on this computer.");
    expect(words(row(missing, "Claude Code"))).not.toContain("Git");
  });

  it("asks for the tool's source to be allowed first, and takes you to Sources", () => {
    const m = view({ status: status() }, "report-cards", [allowed("codex")]);
    const claude = row(m, "Claude Code");
    expect(words(claude)).toBe("Claude Code Allow Claude Code in Sources first, so the app can read your report cards. Open Sources");
    const go = button(claude);
    expect(go?.dataset).toMatchObject({ action: "go-settings", id: "sources" });
    expect(button(claude, "[data-action='report-cards-tool']")).toBeNull();
    // Codex is allowed, so it just turns on.
    expect(button(row(m, "Codex"))?.textContent).toBe("Turn on");
  });

  it("counts only the tool's own source: Cowork doesn't stand in for Claude Code, and nothing allowed is nothing", () => {
    const cowork = view({ status: status() }, "report-cards", [allowed("claude-cowork"), allowed("codex")]);
    expect(words(row(cowork, "Claude Code"))).toContain("Allow Claude Code in Sources first");
    const none = view({ status: status() }, "report-cards", []);
    expect(words(row(none, "Codex"))).toContain("Allow Codex in Sources first, so the app can read your report cards.");
    const revoked = view({ status: status() }, "report-cards", [
      allowed("claude-code", {
        state: { id: "claude-code", allowed: false, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] },
      }),
    ]);
    expect(words(row(revoked, "Claude Code"))).toContain("Allow Claude Code in Sources first");
  });

  it("still offers Turn off for a tool that is on, whatever its source says", () => {
    const m = view({ status: status({ claude_code: tool({ on: true }) }) }, "report-cards", []);
    expect(button(row(m, "Claude Code"))?.textContent).toBe("Turn off");
    expect(words(row(m, "Claude Code"))).not.toContain("Sources");
  });

  it("shows the working state on the button and holds the others", () => {
    const m = view({ status: status({ claude_code: tool({ on: true }) }), busy: "codex" });
    expect(button(row(m, "Codex"))?.getAttribute("aria-label")).toBe("Turning on report cards in Codex");
    expect(button(row(m, "Codex"))?.textContent).toBe("Turning on…");
    expect(button(row(m, "Codex"))?.getAttribute("aria-disabled")).toBe("true");
    expect(button(row(m, "Claude Code"))?.getAttribute("aria-disabled")).toBe("true");
    expect(button(dom(m), '[data-action="report-cards-delete"]')?.getAttribute("aria-disabled")).toBe("true");
    const off = view({ status: status({ claude_code: tool({ on: true }) }), busy: "claude_code" });
    expect(button(row(off, "Claude Code"))?.textContent).toBe("Turning off…");
    expect(button(dom(view({ status: status(), busy: "delete" })), '[data-action="report-cards-delete"]')?.textContent).toBe("Deleting…");
    expect(view({ status: status({ claude_code: tool({ on: true }) }) })).not.toContain("aria-disabled");
  });

  it("shows an error beside the control that failed, and escapes it", () => {
    const text = "Couldn't reach GitHub to download the plugin. Check your connection and try again.";
    const m = view({ status: status(), error: { control: "codex", text } });
    expect(row(m, "Codex").querySelector(".action-error")?.textContent).toBe(text);
    expect(row(m, "Claude Code").querySelector(".action-error")).toBeNull();
    const hostile = view({ status: status(), error: { control: "delete", text: "<img src=x onerror=alert(1)>" } });
    expect(hostile).not.toContain("<img");
    expect(hostile).toContain("&lt;img");
    expect(dom(view({ status: status(), error: { control: "tips", text: "No." } })).querySelector(".action-error")?.textContent).toBe("No.");
    expect(dom(view({ status: status(), error: { control: "reminders", text: "Nope." } })).querySelector(".action-error")?.textContent).toBe("Nope.");
  });

  it("never marks an error or a note as an alert: they are drawn again on every redraw, and announced once by the app", () => {
    const m = view({
      status: status({ claude_code: tool({ problem: "Couldn't update the report card plugin. It will try again next launch." }) }),
      error: { control: "codex", text: "Something went wrong." },
      note: { control: "claude_code", text: "Done." },
    });
    expect(m).not.toContain("role=");
    expect(m).not.toContain("alert");
    expect(view({ statusError: "Something went wrong. Try again." })).not.toContain('role="alert"');
  });

  it("shows what a finished job says beside its tool, and what the launch check couldn't do on the Claude Code row", () => {
    const problem = "Couldn't update the report card plugin. It will try again next launch.";
    const m = view({
      status: status({ claude_code: tool({ on: true, problem }) }),
      note: { control: "codex", text: "Done. Start a new Codex chat, then type $grade-me." },
    });
    expect(row(m, "Claude Code").querySelector(".action-error")?.textContent).toBe(problem);
    expect(row(m, "Codex").querySelector(".action-note")?.textContent).toBe("Done. Start a new Codex chat, then type $grade-me.");
    expect(row(m, "Claude Code").querySelector(".action-note")).toBeNull();
    expect(row(m, "Codex").querySelector(".action-error")).toBeNull();
  });

  it("has a reminders box, on unless the app says it's off, that says it uses no tokens", () => {
    const on = dom(view({ status: status() }));
    const box = on.querySelector<HTMLInputElement>("#report-cards-reminders");
    expect(box?.checked).toBe(true);
    expect(box?.dataset.action).toBe("report-cards-reminders");
    expect(box?.getAttribute("aria-describedby")).toBe("report-cards-reminders-help");
    expect(on.querySelector("#report-cards-reminders-help")).not.toBeNull();
    expect(words(on)).toContain("Remind me after a task (uses no tokens)");
    expect(dom(view({ status: status({ reminders: false }) })).querySelector<HTMLInputElement>("#report-cards-reminders")?.checked).toBe(false);
  });

  it("ticks the tips box from the status and ties it to its help", () => {
    const off = dom(view({ status: status({ claude_code: tool({ on: true }) }) }));
    const box = off.querySelector<HTMLInputElement>("#report-cards-tips");
    expect(box?.checked).toBe(false);
    expect(box?.dataset.action).toBe("report-cards-tips");
    expect(box?.getAttribute("aria-describedby")).toBe("report-cards-tips-help");
    expect(off.querySelector("#report-cards-tips-help")).not.toBeNull();
    expect(dom(view({ status: status({ tips: true, claude_code: tool({ on: true }) }) })).querySelector<HTMLInputElement>("#report-cards-tips")?.checked).toBe(
      true,
    );
  });

  it("shows the exact rules, escaped, closed until asked", () => {
    const rules = dom(view({ status: status() })).querySelector("details.tip-rules");
    expect(rules?.querySelector("summary")?.textContent).toBe("See what your AI is told");
    expect(rules?.querySelector("pre")?.textContent).toBe("Be <kind> to them.");
    expect(rules?.hasAttribute("open")).toBe(false);
  });

  it("says how tips are doing only when they're on and able to work", () => {
    const on = { tips: true, claude_code: tool({ on: true }) };
    const said = (over: Partial<ReportCardsStatus>) => words(dom(view({ status: status(over) })));
    expect(said({ ...on, tip_activity: { active: 6, tips: 0, blocked: false } })).toContain("Active in 6 chats this week.");
    expect(said({ ...on, tip_activity: { active: 6, tips: 14, blocked: false } })).toContain("Active in 6 chats this week · 14 tips so far.");
    expect(said({ ...on, tip_activity: { active: 1, tips: 1, blocked: false } })).toContain("Active in 1 chat this week · 1 tip so far.");
    expect(said({ ...on, tip_activity: { active: 1, tips: 0, blocked: false } })).toContain("Active in 1 chat this week.");
    expect(said({ ...on, tip_activity: { active: 0, tips: 0, blocked: false } })).not.toContain("Active in");
    expect(said({ ...on, tip_activity: { active: 0, tips: 0, blocked: true } })).toContain(
      "Recent chats aren't getting the tip instructions. Start a new chat. On a work computer, ask your admin whether plugin hooks are allowed.",
    );
    expect(said({ tips: false, claude_code: tool({ on: true }), tip_activity: { active: 6, tips: 0, blocked: true } })).not.toMatch(
      /Active in|received the tip/,
    );
    expect(said({ tips: true, claude_code: tool({ on: false }), tip_activity: { active: 6, tips: 0, blocked: true } })).not.toMatch(
      /Active in|received the tip/,
    );
  });

  it("says tips are paused while Claude Code isn't being read", () => {
    const paused = "They're paused while No, Actually... isn't reading Claude Code.";
    const on = status({ tips: true, claude_code: tool({ on: true }) });
    expect(words(dom(view({ status: on }, "report-cards", [])))).toContain(paused);
    expect(words(dom(view({ status: on })))).not.toContain(paused);
    expect(words(dom(view({ status: status({ tips: false, claude_code: tool({ on: true }) }) }, "report-cards", [])))).not.toContain(paused);
  });

  it("names the tips box for Claude Code only, and says what it costs", () => {
    const text = words(dom(view({ status: status({ claude_code: tool({ on: true }) }) })));
    expect(text).toContain("Give me tips as I work (Claude Code only)");
    expect(text).toContain(
      "When a follow-up could have been in your first message, your AI adds a one-line tip, and No, Actually... keeps count. Adds about 480 tokens when a chat starts and again after a long chat is compacted, about 2.5% more tokens overall in testing.",
    );
    expect(text).not.toMatch(/1,000|per report card/);
  });

  it("holds the tips box until Claude Code is on, and says why", () => {
    const auto = (cards: Partial<ReportCardsState>) => dom(view(cards)).querySelector<HTMLInputElement>("#report-cards-tips");
    expect(auto({ status: status() })?.hasAttribute("aria-disabled")).toBe(true);
    expect(words(dom(view({ status: status() })))).toContain("Turn on report cards in Claude Code first.");
    expect(auto({ status: status({ codex: tool({ on: true }) }) })?.hasAttribute("aria-disabled")).toBe(true);
    expect(auto({ status: status({ claude_code: tool({ on: true }) }) })?.hasAttribute("aria-disabled")).toBe(false);
    expect(words(dom(view({ status: status({ claude_code: tool({ on: true }) }) })))).not.toContain("first.");
  });

  it("holds the tips box while any report card command runs", () => {
    const on = status({ claude_code: tool({ on: true }) });
    for (const busy of ["claude_code", "codex", "delete"] as const) {
      expect(
        dom(view({ status: on, busy }))
          .querySelector<HTMLInputElement>("#report-cards-tips")
          ?.hasAttribute("aria-disabled"),
        busy,
      ).toBe(true);
    }
  });

  it("never holds the tips box while it is ticked: turning it off always works", () => {
    const auto = (cards: Partial<ReportCardsState>) => dom(view(cards)).querySelector<HTMLInputElement>("#report-cards-tips");
    // Ticked, with Claude Code off (or not found): still there to untick.
    for (const claude of [tool(), tool({ installed_tool: false }), tool({ on: true, unavailable: "Report cards need Git for Windows." })]) {
      const box = auto({ status: status({ tips: true, claude_code: claude }) });
      expect(box?.checked).toBe(true);
      expect(box?.hasAttribute("aria-disabled"), JSON.stringify(claude)).toBe(false);
    }
    // Ticked while a command runs: the same.
    for (const busy of ["claude_code", "codex", "delete"] as const) {
      expect(auto({ status: status({ tips: true, claude_code: tool({ on: true }) }), busy })?.hasAttribute("aria-disabled"), busy).toBe(false);
      expect(auto({ status: status({ tips: true, claude_code: tool() }), busy })?.hasAttribute("aria-disabled"), `${busy}, Claude Code off`).toBe(false);
    }
    // Unticked, it still waits, as before. And until the app has answered it holds still, ticked or not.
    expect(auto({ status: status({ tips: false, claude_code: tool() }) })?.hasAttribute("aria-disabled")).toBe(true);
    expect(auto({ status: status({ tips: false, claude_code: tool({ on: true }) }), busy: "codex" })?.hasAttribute("aria-disabled")).toBe(true);
    expect(auto({})?.hasAttribute("aria-disabled")).toBe(true);
  });

  it("says what a ticked box does while Claude Code is off, instead of asking to turn it on first", () => {
    const help = (cards: Partial<ReportCardsState>) => dom(view(cards)).querySelector("#report-cards-tips-help")?.textContent;
    expect(help({ status: status({ tips: true, claude_code: tool() }) })).toContain("New chats won't get them until report cards are on in Claude Code.");
    expect(help({ status: status({ tips: true, claude_code: tool() }) })).not.toContain("first.");
    expect(help({ status: status({ tips: false, claude_code: tool() }) })).toContain("Turn on report cards in Claude Code first.");
    expect(help({ status: status({ tips: true, claude_code: tool({ on: true }) }) })).not.toContain("Claude Code");
  });

  it("says it is checking, and holds the boxes, until the app answers", () => {
    const m = view();
    expect(m).toContain('<p class="muted" role="status">Checking Claude Code and Codex…</p>');
    expect(m).not.toContain("report-cards-tool");
    expect(dom(m).querySelector<HTMLInputElement>("#report-cards-tips")?.hasAttribute("aria-disabled")).toBe(true);
    expect(dom(m).querySelector<HTMLInputElement>("#report-cards-reminders")?.disabled).toBe(true);
  });

  it("says why it couldn't check, and offers to try again", () => {
    const m = dom(view({ statusError: "Something went wrong. Try again." }));
    expect(m.querySelector(".action-error")?.textContent).toBe("Something went wrong. Try again.");
    expect(button(m, '[data-action="report-cards-retry"]')?.textContent).toBe("Try again");
    expect(words(m)).not.toContain("Checking");
  });

  it("shows what just happened to the history", () => {
    expect(dom(view({ status: status(), note: { control: "delete", text: "Deleted 3 report cards." } })).querySelector("#report-cards-note")?.textContent).toBe(
      "Deleted 3 report cards.",
    );
    expect(dom(view({ status: status(), note: { control: "codex", text: "Done." } })).querySelector("#report-cards-note")?.textContent).toBe("");
  });
});

describe("delete everything", () => {
  const privacy = (deleteAll = { busy: false, error: "" }) => dom(view({}, "privacy", BOTH, false, deleteAll));
  const del = (el: HTMLElement) => button(el, '[data-action="delete-everything"]');

  it("says what it does and that it can't be undone, in Privacy only", () => {
    const help = privacy().querySelector("#delete-everything-help")?.textContent;
    expect(help).toContain("can't be undone");
    expect(help).toContain("Launch at login");
    expect(del(privacy())?.textContent).toBe("Delete everything");
    expect(del(privacy())?.className).toBe("danger");
    expect(del(dom(view({}, "general")))).toBeFalsy();
  });

  it("shows it working", () => {
    const b = del(privacy({ busy: true, error: "" }));
    expect(b?.textContent).toBe("Deleting…");
    expect(b?.getAttribute("aria-disabled")).toBe("true");
  });

  it("shows why it stopped as an alert, as text", () => {
    const el = privacy({ busy: false, error: "<img src=x onerror=alert(1)>" });
    expect(el.querySelector('[role="alert"]')?.textContent).toBe("<img src=x onerror=alert(1)>");
    expect(el.querySelector("img")).toBeNull();
    expect(privacy().querySelector('[role="alert"]')).toBeNull();
  });
});
