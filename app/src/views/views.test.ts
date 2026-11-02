import { describe, expect, it } from "vitest";
import { emptyReportCards } from "../state";
import { HOSTILE, hallEntry, meta, overall, row, source, stats } from "../test/fixtures";
import { headerView, statusPillView } from "./chrome";
import { emptyGlanceView, glanceErrorView, glanceRebuildingView, glanceView } from "./glance";
import { chartSummary, chartView, hallView, noChatsView, onboardingView, rebuildingView, scorecardView } from "./scorecard";
import { goneByCount, parseRetention, settingsView, tightens } from "./settings";
import { allowArgs, sourcesView, watchingNames } from "./sources";

const ctx = { meta, filter: {}, still: false };
/** No hostile markup made it through as markup. */
const safe = (markup: string) => {
  expect(markup).not.toContain("<img");
  expect(markup).not.toContain('onerror="');
};

describe("verdict text for screen readers", () => {
  it("matches the Hmm. on screen", () => {
    const m = scorecardView(stats({ overall: overall({ verdict: "hmm" }) }), ctx).value;
    expect(m).toContain("Hmm. ");
    expect(m).not.toContain("Not yet.");
  });
});

describe("scorecardView", () => {
  it("opens on an overview with both verdicts and the section tabs", () => {
    const m = scorecardView(stats(), ctx).value;
    expect(m).toContain('aria-label="Scorecard sections"');
    expect(m).toMatch(
      /data-id="overview" data-tip="Overview" aria-current="page"><svg class="tab-icon"[^>]*aria-hidden="true">.*?<\/svg><span class="tab-name">Overview<\/span><\/button>/,
    );
    expect(m).toContain('data-id="prompts" data-tip="Prompts"');
    // Prompts come before corrections (they are 70% of the score): in the tabs, the overview cards and the score's parts.
    expect(m.indexOf('data-id="prompts"')).toBeLessThan(m.indexOf('data-id="corrections"'));
    expect(m.indexOf("Do you make it guess?")).toBeLessThan(m.indexOf("Do you correct it?"));
    expect(m.indexOf("Your prompts<small")).toBeLessThan(m.indexOf("Who corrects whom<small"));
    expect(m).toContain("Could ");
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain("Do you correct it?");
    expect(m).toContain('data-action="view" data-id="corrections"');
    expect(m).not.toContain("Hall of fame ·");
  });

  it("puts exactly one model picker in every section", () => {
    for (const view of ["overview", "corrections", "prompts", "hall"] as const) {
      const m = scorecardView(stats(), { ...ctx, view }).value;
      expect(m.split('id="model-trigger"').length - 1, view).toBe(1);
    }
    expect(scorecardView(stats(), { ...ctx, view: "prompts" }).value).toContain("Why you followed up");
    expect(scorecardView(stats(), { ...ctx, view: "hall" }).value).toContain('data-action="pin"');
  });

  it("draws the ladder from Rust's data on the overview", () => {
    const m = scorecardView(stats(), ctx).value;
    expect(m).toContain(">No.</div>");
    expect(m).toContain(
      '<li class="on" aria-current="true"><span>Sharp-eyed</span><span><b class="at">86</b><span class="sr-only">, you\'re at 86. Range</span> · 80–89</span></li>',
    );
    expect(m).toContain("<span>Irreplaceable</span><span>90+</span>");
    expect(m).not.toContain("style=");
  });

  it("draws the breakdowns from Rust's data in the corrections section", () => {
    const m = scorecardView(stats(), { ...ctx, view: "corrections" }).value;
    expect(m).toContain("Do you correct");
    expect(m).toContain("Flat-out wrong"); // category title from meta
    expect(m).toContain('data-w="100"'); // the top category's bar; width set through CSSOM
    expect(m).not.toContain("style=");
  });

  it("answers the corrections section in a word, with the correction tiers and a share", () => {
    const m = scorecardView(stats(), { ...ctx, view: "corrections" }).value;
    expect(m).toContain('aria-hidden="true">Religiously.</div>');
    expect(m).toContain('<ol class="ladder" aria-label="Correction tiers">');
    expect(m).toContain('<span>Religiously</span><span><b class="at">95</b>');
    expect(m).toContain("<span>Sometimes</span><span>50–74</span>");
    expect(m).toContain('<p class="tier-blurb"><b>Hawk-eyed.</b> You catch AI slipping nearly every time.</p>');
    expect(m).toMatch(/data-action="share-corrections"><svg[^>]*>.*?<\/svg>Share your score<\/button>/);
    const unranked = scorecardView(stats({ overall: overall({ judgment: null, correction_tier: null, to_rank: 3 }) }), { ...ctx, view: "corrections" }).value;
    expect(unranked).toContain(">Hmm.</div>");
    expect(unranked).not.toContain("share-corrections");
  });

  it("asks the question in one short line, naming the model", () => {
    const m = scorecardView(stats(), { ...ctx, view: "corrections" }).value;
    expect(m).toContain('<h1 class="q">Do you correct AI?</h1>');
    // The sentence carries both counts, so there's no separate you-vs-it block.
    expect(m).not.toContain("You corrected AI</span>");
    const opus = scorecardView(stats(), { ...ctx, view: "corrections", filter: { provider: "Anthropic", family: "Claude Opus" } }).value;
    expect(opus).toContain('<h1 class="q">Do you correct Claude Opus?</h1>');
  });

  it("says so when there are no corrections in the window yet", () => {
    const m = scorecardView(
      stats({ corrections: 0, overall: overall({ corrections: 0, judgment: 0, correction_tier: "autopilot", next_correction_tier: "spot_checker" }) }),
      {
        ...ctx,
        view: "corrections",
      },
    ).value;
    expect(m).toContain('aria-hidden="true">Rarely.</div>');
    expect(m).toContain("so there's no proof yet that you check its work.");
  });

  it("shows Hmm. and the next tier by name and floor when Rust says hmm", () => {
    const m = scorecardView(stats({ overall: overall({ verdict: "hmm", score: 60, tier: "rubber_stamp", next_tier: "coworker" }) }), ctx).value;
    expect(m).toContain("no maybe");
    expect(m).toContain(">Hmm.</div>");
    expect(m).toContain('<b class="at">60</b>');
  });

  it("keeps the scrawl still when the verdict didn't change", () => {
    expect(scorecardView(stats(), { ...ctx, still: true }).value).toContain('class="no still"');
  });

  it("keeps each verdict's scrawl on its own flag", () => {
    const count = (c: object) => scorecardView(stats(), { ...ctx, ...c }).value.match(/class="no still"/g)?.length ?? 0;
    expect(count({ still: true })).toBe(1);
    expect(count({ promptsStill: true })).toBe(1);
    expect(count({ correctionsStill: true })).toBe(1);
    expect(count({ still: true, promptsStill: true, correctionsStill: true })).toBe(3);
  });

  it("names a filter only in the model picker, with no chip to repeat it", () => {
    const m = scorecardView(stats(), { ...ctx, filter: { provider: "Anthropic", family: "Claude Opus", version: "5" } }).value;
    expect(m).not.toContain("Showing");
    expect(m).not.toContain("clear-filter");
    expect(m).toContain('id="model-trigger"');
  });

  it("escapes every string that comes from chats or model names", () => {
    const s = stats({
      models: [row(HOSTILE, HOSTILE, HOSTILE, 3)],
      categories: { [HOSTILE]: 2 },
      hall_of_fame: [hallEntry({ human: HOSTILE, ai: HOSTILE, model: HOSTILE, label: HOSTILE, day: HOSTILE })],
      first_day: HOSTILE,
    });
    safe(scorecardView(s, { ...ctx, filter: { provider: HOSTILE } }).value);
  });
});

describe("overview hero", () => {
  const hero = (over: Parameters<typeof overall>[0] = {}, extra: Parameters<typeof stats>[0] = {}) =>
    scorecardView(stats({ overall: overall(over), ...extra }), ctx).value;

  it("shows the score, the tier and why, with a share button once scored", () => {
    const m = hero();
    expect(m).not.toContain('class="eyebrow"'); // the picker and the sentence already say the window
    expect(m).toContain(
      "Score <b>86</b>. <b>83%</b> of your prompts were clear the first time. You corrected it <b>41 times</b> in the last 28 days. It corrected you 2 times.",
    );
    expect(m).toContain("Your tier is Sharp-eyed, score 86.");
    expect(m).not.toContain("Irreplaceable at 90. You&#39;re at 86."); // the row says it
    expect(m).toContain('data-action="open-share"');
    // Each overview card shares its own answer next to its See button, with the share icon.
    expect(m).toContain('data-action="share-prompts"><svg class="btn-icon"');
    expect(m).toContain('data-action="share-corrections"><svg class="btn-icon"');
    expect(m).toContain('data-action="open-share"><svg class="btn-icon" viewBox="0 0 24 24" aria-hidden="true">');
  });

  it("answers Do you correct it? in a word on the overview, with its tier", () => {
    const m = hero({ correction_tier: "reviewer", next_correction_tier: "hawk_eyed", judgment: 88 }, { corrections: 41, humbled: 2 });
    expect(m).toContain('aria-hidden="true">Often.</div>');
    expect(m).toContain("You corrected it <b>41 times</b> in the last 28 days. It corrected you 2 times.");
    expect(m).toContain('<p class="tier-blurb"><b>Reviewer.</b> You often catch it slipping.</p>');
    expect(m).toContain("Next: Religiously, at 90. You&#39;re at 88.");
  });

  it("shows the two parts as bars with their weights from meta", () => {
    const m = hero();
    expect(m).toContain('aria-label="What the score is made of"');
    expect(m).toContain("Your prompts<small");
    expect(m).toContain("prompts clear the first time · 70% of the score");
    expect(m).toContain('data-w="83"');
    expect(m).toContain("Who corrects whom<small");
    expect(m).toContain("you check its work, and it rarely corrects you");
    expect(m).toContain("· 30% of the score");
    expect(m).toContain('data-w="95"');
    expect(hero({}, {}).split('class="part"')).toHaveLength(3);
  });

  it("marks the top tier's row with your score", () => {
    expect(hero({ tier: "irreplaceable", next_tier: null, score: 93 })).toContain('<span>Irreplaceable</span><span><b class="at">93</b>');
  });

  it("asks for more tasks, and offers nothing to share, until there is a score", () => {
    const m = hero({ score: null, direction: null, judgment: null, tier: null, next_tier: null, verdict: "hmm", to_rank: 7 });
    expect(m).toContain("<b>7 more tasks</b> with more than one message to get a score");
    expect(m).toContain("7 more tasks to get a score");
    expect(m).toContain("Not scored yet.");
    expect(m).toContain('data-w="0"');
    expect(m).not.toContain('data-action="open-share"');
    expect(m).not.toMatch(/<li class="on" aria-current="true"/); // no tier on the ladder
  });

  it("says task, not tasks, for one", () => {
    expect(hero({ score: null, tier: null, next_tier: null, verdict: "hmm", to_rank: 1 })).toContain("<b>1 more task</b> with");
  });

  it("keeps the corrections ladder free of fine print", () => {
    const m = scorecardView(stats(), { ...ctx, view: "corrections" }).value;
    expect(m).not.toContain("corrections prove you check its work");
  });

  it("escapes names from meta and unknown ids", () => {
    const bad = { ...meta, tiers: meta.tiers.map((t) => ({ ...t, name: HOSTILE })) };
    safe(scorecardView(stats(), { ...ctx, meta: bad }).value);
    safe(scorecardView(stats({ overall: overall({ tier: HOSTILE, next_tier: HOSTILE }) }), ctx).value);
  });
});

describe("hallView", () => {
  it("uses the award name, never the label, and the source name when no model was recorded", () => {
    const m = hallView([hallEntry({ label: "confirmed", model: "", source_name: "ChatGPT (export)" })]).value;
    expect(m).not.toContain("Made it up"); // the award names the kind
    expect(m).not.toContain("confirmed");
    expect(m).toContain('<div class="meta">');
    expect(m).toContain("<b>ChatGPT:</b>");
  });

  it("marks pinned corrections and says how to unpin", () => {
    const m = hallView([hallEntry({ pinned: true })]).value;
    expect(m).toContain(">Pinned<");
    expect(m).not.toContain("forever");
    expect(m).toContain('data-pinned="true"');
    expect(m).toContain(">Unpin<");
  });
});

describe("hall entries", () => {
  it("offers a link to report a wrong count, with the redaction reminder", () => {
    const m = hallView([hallEntry()]).value;
    expect(m).toContain('data-action="counted-wrong"');
    expect(m).toContain("Leave out names, secrets");
    expect(m).toContain(">Pin it<");
  });

  it("keeps the report link after the actions", () => {
    const m = hallView([hallEntry()]).value;
    expect(m.indexOf(">Pin it<")).toBeLessThan(m.indexOf("Counted wrong?"));
  });
});

describe("noChatsView", () => {
  it("names the period and offers all time", () => {
    const m = noChatsView({ meta, filter: {}, period: "28d" }).value;
    expect(m).toContain("No chats in the last 28 days.");
    expect(m).toContain(">Show all time<");
    expect(m).toContain('data-action="period"');
    expect(m).toContain('id="model-trigger"');
  });

  it("offers all AI when a model is picked", () => {
    expect(noChatsView({ meta, filter: { family: "Claude" }, period: "all" }).value).toContain(">Show all AI<");
  });
});

describe("chartView", () => {
  it("labels the busiest week once without coloring it as the good one", () => {
    const m = chartView(stats(), meta).value;
    expect(m).not.toContain("hot");
    expect(m.match(/class="peak-label">Most: /g)).toHaveLength(1);
    expect(m).toContain('aria-label="Corrections per week"');
    expect(m).toContain('aria-describedby="chart-summary"');
  });

  it("gives each week's bar its breakdown: corrections by kind, most first, and the times it corrected you", () => {
    const m = chartView(stats(), meta).value;
    expect(m).toContain('data-tip="Week of Oct 19\n16 corrections · 200 messages\n\nFlat-out wrong 10\nMade it up 6\n\nIt corrected you 1 time"');
    expect(m).not.toContain("<title>");
  });

  it("says when there are no weeks", () => {
    expect(chartView(stats({ weekly: [] }), meta).value).toContain("No weeks yet.");
  });

  it("summarises the chart in words for screen readers", () => {
    const t = chartSummary(stats().weekly);
    expect(t).toMatch(/^2 weeks from .+\. Most corrections: 16, the week of .+\.$/);
    expect(chartSummary(stats().weekly.slice(0, 1))).toMatch(/^1 week from /);
    expect(chartSummary([])).toBe("No weeks yet.");
  });
});

describe("onboarding and glance", () => {
  it("welcomes a new person, or says what's being watched", () => {
    expect(onboardingView("").value).toContain("Choose sources");
    const m = onboardingView(HOSTILE).value;
    expect(m).toContain("You're all set.");
    safe(m);
  });

  it("shows the score and tier by name in the glance", () => {
    const m = glanceView(stats(), meta).value;
    expect(m).toContain("Score 86 · Sharp-eyed");
    expect(glanceView(stats({ overall: overall({ tier: "irreplaceable", next_tier: null, score: 93 }) }), meta).value).toContain(
      "Score 93 · Irreplaceable</span>",
    );
  });

  it("asks for more tasks in the glance until there is a score", () => {
    const o = overall({ score: null, direction: null, judgment: null, tier: null, next_tier: null, verdict: "hmm", to_rank: 4 });
    const m = glanceView(stats({ overall: o }), meta).value;
    expect(m).toContain("4 more tasks to get a score");
    expect(m).toContain('<span class="g-word maybe">Hmm.</span>');
  });

  it("has an empty glance before the first correction", () => {
    expect(emptyGlanceView("").value).toContain("Set up");
    safe(emptyGlanceView(HOSTILE).value);
  });
});

describe("chrome", () => {
  it("marks the current tab for assistive tech", () => {
    const m = headerView("settings", { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false }, false).value;
    expect(m).toContain('data-id="settings" data-tip="Settings" aria-current="page"');
    expect(m.match(/aria-current/g)).toHaveLength(1);
  });

  it("gives every tab an icon and makes the logo a way home", () => {
    const m = headerView("settings", { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false }, false).value;
    expect(m.match(/class="tab-icon"/g)).toHaveLength(4);
    expect(m).toContain('<button class="home" data-action="home"');
  });

  it("escapes a sync error in the retry pill", () => {
    const m = statusPillView({ syncing: false, last_sync: null, last_error: HOSTILE, files_with_errors: 0, rebuilding: false }, false).value;
    expect(m).toContain("Couldn't check. Try again");
    safe(m);
  });

  it("says Demo data in the demo, and Checking while syncing", () => {
    expect(statusPillView({ syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false }, true).value).toContain("Demo data");
    expect(statusPillView({ syncing: true, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false }, false).value).toContain("Checking…");
  });
});

const status = { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false };

describe("sourcesView", () => {
  it("checks for new chats from Sources, and leaves app updates to General", () => {
    const m = sourcesView([source()], status).value;
    expect(m).toContain('data-action="sync-now">Check for new chats</button>');
    expect(m).toContain('id="sync-line">New chats last checked');
    const g = settingsView({
      pane: "general",
      sources: [],
      status,
      retention: { level: "snippet", full_days: 30, snippet_days: 365 },
      autostart: false,
      menuBar: { enabled: true, available: true },
      reportCards: emptyReportCards(),
      deleteAll: { busy: false, error: "" },
      tipRules: "",
      update: null,
      demo: false,
    }).value;
    expect(g).toContain(">Check for app updates</button>");
    expect(g).not.toContain("sync-now");
  });

  it("offers Allow with a how-far-back choice for a found, unallowed source", () => {
    const m = sourcesView([source()], status).value;
    expect(m).toContain('data-action="allow" data-id="claude-code" aria-label="Allow Claude Code"');
    expect(m).toContain('<option value="none">From now on</option>');
    expect(m).not.toContain('data-action="forget"');
  });

  it("offers pause, stop and forget once allowed", () => {
    const state = { id: "claude-code", allowed: true, paused: true, backfill_since: null, granted_at: null, start_after: null, paths: [] };
    const m = sourcesView([source({ state })], status).value;
    expect(m).toContain(">Resume<");
    expect(m).toContain('data-paused="true"');
    expect(m).toContain('data-action="forget" data-id="claude-code" aria-label="Forget Claude Code"');
    expect(m).toContain('aria-label="Resume Claude Code"');
    expect(m).toContain('aria-label="Stop reading Claude Code"');
  });

  it("explains how to get an export", () => {
    expect(sourcesView([source({ id: "chatgpt-export", kind: "import", roots: [] })], status).value).toContain(
      "Settings → Data controls → Export data. It arrives as a zip by email. Unzip it, then choose conversations.json.",
    );
  });

  it("escapes names and paths", () => {
    safe(sourcesView([source({ name: HOSTILE, roots: [[HOSTILE, true]] })], status).value);
  });

  it("lists what's being read right now", () => {
    const on = { id: "x", allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] };
    const list = [
      source({ name: "ChatGPT (export)", kind: "import", state: on }),
      source({ name: "Claude Code", state: on }),
      source({ name: "Codex", state: { ...on, paused: true } }),
      source(),
    ];
    // A file you imported is read once, never "watched".
    expect(watchingNames(list)).toEqual(["Claude Code"]);
  });
});

describe("allowArgs", () => {
  const now = new Date(2026, 9, 27, 0, 30); // just after local midnight
  it("reads all history, the last N local days, or from now on", () => {
    expect(allowArgs("all", now)).toEqual({ since: null, fromNow: false });
    expect(allowArgs("7", now)).toEqual({ since: "2026-10-20", fromNow: false });
    expect(allowArgs("30", now)).toEqual({ since: "2026-09-27", fromNow: false });
    expect(allowArgs("none", now)).toEqual({ since: null, fromNow: true });
  });

  it("treats anything unexpected as all history", () => {
    expect(allowArgs("soon", now)).toEqual({ since: null, fromNow: false });
  });
});

describe("parseRetention", () => {
  it("reads the form", () => {
    expect(parseRetention({ level: "full", fullDays: "14", snippetDays: "90" })).toEqual({ level: "full", full_days: 14, snippet_days: 90 });
  });

  it("clamps to 1–3650 whole days (a u32 in Rust)", () => {
    expect(parseRetention({ level: "snippet", fullDays: "-5", snippetDays: "999999" })).toEqual({ level: "snippet", full_days: 1, snippet_days: 3650 });
    expect(parseRetention({ level: "snippet", fullDays: "2.6", snippetDays: "3650" })).toMatchObject({ full_days: 3, snippet_days: 3650 });
  });

  it("uses the defaults for blank, zero or nonsense", () => {
    expect(parseRetention({ level: "category", fullDays: "", snippetDays: "abc" })).toMatchObject({ full_days: 30, snippet_days: 365 });
    expect(parseRetention({ level: "category", fullDays: "0", snippetDays: "Infinity" })).toMatchObject({ full_days: 30, snippet_days: 365 });
  });

  it("refuses an unknown level", () => {
    expect(() => parseRetention({ level: "everything", fullDays: "1", snippetDays: "1" })).toThrow();
    expect(() => parseRetention({ level: null, fullDays: "1", snippetDays: "1" })).toThrow();
  });
});

describe("settingsView", () => {
  const base = {
    pane: "general" as const,
    sources: [source()],
    status: { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false },
    retention: { level: "snippet" as const, full_days: 30, snippet_days: 365 },
    autostart: true,
    menuBar: { enabled: true, available: true },
    reportCards: emptyReportCards(),
    deleteAll: { busy: false, error: "" },
    tipRules: "",
    update: null,
    demo: false,
  };

  it("shows one sub-tab at a time: Sources, General, Report cards or Privacy", () => {
    const tabs = (pane: "sources" | "general" | "report-cards" | "privacy") => settingsView({ ...base, pane }).value;
    expect(tabs("general")).toContain('data-action="settings-pane" data-id="general" data-tip="General" aria-current="page"');
    expect(tabs("general")).not.toContain('name="level"');
    expect(tabs("sources")).toContain('<section id="sources">');
    expect(tabs("sources")).not.toContain('id="menu-bar"');
    // The tab names the pane, so the pane's heading is for screen readers only.
    expect(tabs("privacy")).not.toContain("<h2>");
    expect(tabs("privacy")).toContain('<h1 class="sr-only">Settings</h1>');
    expect(tabs("privacy")).toContain('<h2 class="sr-only">Privacy</h2>');
    expect(tabs("general")).toContain('<h2 class="sr-only">General</h2>');
    const privacy = tabs("privacy");
    expect(privacy).toContain('value="snippet" data-action="level" checked');
    expect(privacy).toContain('max="3650"');
    expect(privacy).toContain('<fieldset class="levels"><legend class="sr-only">What to keep</legend>');
    expect(privacy).toContain("Hall of fame");
    expect(privacy).toContain("Not affiliated with or endorsed by Anthropic, OpenAI or any other AI company.");
    expect(tabs("general")).toContain('id="autostart" data-action="autostart" type="checkbox" aria-describedby="autostart-help" checked');
  });

  it("checks the menu-bar toggle when it's on, and leaves it clear when off", () => {
    expect(settingsView(base).value).toContain('id="menu-bar" data-action="menu-bar" type="checkbox" aria-describedby="menu-bar-help" checked');
    expect(settingsView(base).value).toContain("Keep running in the menu bar");
    const off = settingsView({ ...base, menuBar: { enabled: false, available: true } }).value;
    expect(off).toContain('id="menu-bar" data-action="menu-bar" type="checkbox" aria-describedby="menu-bar-help"/>');
    expect(off).toContain("Opens the app when you log in.");
    expect(off).not.toContain("no menu bar tray");
  });

  it("locks the menu-bar toggle, with a hint, when the system has no tray", () => {
    const m = settingsView({ ...base, menuBar: { enabled: true, available: false } }).value;
    expect(m).toContain('type="checkbox" aria-describedby="menu-bar-help" checked disabled');
    expect(m).toContain("Your system has no menu bar tray, so closing the window quits.");
    expect(m).toContain("Opens the app when you log in.");
  });

  it("ties each toggle to its help text", () => {
    const m = settingsView(base).value;
    for (const id of ["menu-bar", "autostart", "auto-update", "update-channel"]) {
      expect(m).toContain(`aria-describedby="${id}-help"`);
      expect(m).toContain(`id="${id}-help"`);
    }
  });

  it("shows the update channel that's saved", () => {
    const update = {
      current: "0.5.1",
      auto: true,
      channel: "beta",
      checking: false,
      installing: false,
      available: null,
      last_check: null,
      last_error: null,
    } as const;
    expect(settingsView({ ...base, update }).value).toContain('<option value="beta" selected>Beta</option>');
    expect(settingsView({ ...base, update: { ...update, channel: "stable" } }).value).toContain('<option value="stable" selected>Stable</option>');
  });

  it("disables what the demo can't do", () => {
    const m = settingsView({ ...base, demo: true }).value;
    expect(m).toContain('data-action="update-channel" aria-describedby="update-channel-help" disabled');
    expect(m).toContain('data-action="check-update" disabled');
    expect(m).toContain('id="menu-bar" data-action="menu-bar" type="checkbox" aria-describedby="menu-bar-help" checked disabled');
  });
});

describe("scorecard period picker", () => {
  it("marks the period, words the correction count for it, and uses it for the score too", async () => {
    const { scorecardView } = await import("./scorecard");
    const view = (period: "7d" | "28d" | "90d" | "all") =>
      scorecardView(stats({ overall: overall({ corrections: 37 }) }), { meta, filter: {}, still: true, period }).value;
    const week = view("7d");
    expect(week).toContain('data-action="period" data-id="7d" aria-current="true"');
    expect(week).toContain("You corrected it <b>37 times</b> in the last 7 days. It corrected you 2 times.");
    // Everything follows the period, the score included: no disclaimer needed.
    expect(week).not.toContain('<div class="eyebrow">');
    expect(week).not.toContain("always uses");
    expect(view("all")).toContain("across all time.");
    expect(view("all")).toContain("<b>37 times</b> across all time. It corrected you 2 times.");
  });
});

describe("periodSince", () => {
  it("counts the period back from today, local time, from the start of history for all time", async () => {
    const { periodSince } = await import("../state");
    const today = new Date(2026, 2, 1); // 1 March
    expect(periodSince("7d", today)).toBe("2026-02-23");
    expect(periodSince("28d", today)).toBe("2026-02-02");
    expect(periodSince("all", today)).toBe("1970-01-01"); // a start date, so the score covers all time too
  });
});

describe("status and refresh", () => {
  it("is a check-now button whose tooltip says what the numbers cover", async () => {
    const { statusPillView, coverage } = await import("./chrome");
    const covers = coverage(stats({ first_day: "2026-08-01", last_day: "2026-10-29", messages: 7485 }));
    const day = (iso: string) => new Date(`${iso}T00:00`).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
    expect(covers).toBe(`${day("2026-08-01")} – ${day("2026-10-29")} · ${(7485).toLocaleString()} messages`);
    const m = statusPillView({ syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false }, false, covers).value;
    expect(m).toContain('data-action="sync-now"');
    expect(m).toContain('data-tip="Check now"');
    expect(m).toContain('<i class="dot"></i><svg class="refresh"');
    expect(statusPillView({ syncing: true, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false }, false, covers).value).not.toContain(
      'data-action="sync-now"',
    );
    expect(m).not.toContain("title=");
    expect(m).toContain(`data-tip="${covers}"`);
    // aria-label on a generic span is ignored, so the extra detail is hidden text.
    expect(m).toContain(`<span class="sr-only">. Showing ${covers}</span>`);
    expect(m).not.toContain('aria-label="Checked');
    expect(coverage(null)).toBe("");
  });

  it("drops the 'pick one from All AI' line and the date line from the scorecard", async () => {
    const { scorecardView } = await import("./scorecard");
    const m = scorecardView(stats(), { meta, filter: {}, still: true }).value;
    expect(m).not.toContain("Pick one from");
    expect(m).not.toContain("messages</span></section>");
  });
});

describe("overview wording", () => {
  const ov = (o: Parameters<typeof overall>[0], extra: Parameters<typeof stats>[0] = {}) => scorecardView(stats({ overall: overall(o), ...extra }), ctx).value;

  it("counts corrections from the scored window and says plainly when it never corrected you", () => {
    const m = ov({ corrections: 52, humbled: 0 }, { corrections: 177, humbled: 3 });
    expect(m).toContain("<b>52 times</b> in the last 28 days. It never corrected you.");
    expect(m).not.toContain("177 times");
  });

  it("says nothing twice when unranked: no hand-note beside the 'give it more tasks' line", () => {
    const m = ov({ score: null, direction: null, judgment: null, tier: null, correction_tier: null, to_rank: 4 });
    expect(m).toContain("Give it <b>4 more tasks</b> with more than one message to get ranked.");
    expect(m).not.toContain("to get ranked</p>");
  });

  it("agrees a one-day streak", () => {
    expect(ov({}, { best_streak_days: 1 })).toContain('<span class="v">1 day</span>');
  });
});

describe("hall of fame awards", () => {
  it("gives each kind of correction an award, best snippet first, most earned first, unearned dimmed", async () => {
    const { scorecardView, AWARDS } = await import("./scorecard");
    const s = stats({
      categories: { made_it_up: 2, just_wrong: 9 },
      humbled: 0,
      hall_of_fame: [
        hallEntry({ id: 1, category: "just_wrong", human: "best one" }),
        hallEntry({ id: 2, category: "just_wrong", human: "runner up" }),
        hallEntry({ id: 3, category: "made_it_up" }),
      ],
    });
    const m = scorecardView(s, { meta, filter: {}, still: true, view: "hall" }).value;
    const big = m.indexOf(AWARDS.just_wrong?.name ?? "?");
    expect(big).toBeGreaterThan(-1);
    expect(big).toBeLessThan(m.indexOf(AWARDS.made_it_up?.name ?? "?"));
    // Entries flip in place: the best shows, the rest wait in the same cell, with "1 of 2" arrows.
    expect(m).toContain('<span class="slide-at">1</span> of top 2');
    expect(m).toContain('data-action="award-slide"');
    expect(m).toMatch(/class="slide" data-entry="\d+" aria-hidden="true" inert/);
    expect(m.indexOf("best one")).toBeLessThan(m.indexOf("runner up"));
    expect(m).toContain('class="award unearned"');
    expect(m).toContain(AWARDS[""]?.name ?? "?");
    expect(m).not.toContain("<img");
    // Each arrow names its award; the count is read as words, not an ignored aria-label on a span.
    expect(m).toContain(`aria-label="Previous ${AWARDS.just_wrong?.name} entry"`);
    expect(m).toContain(`aria-label="Next ${AWARDS.just_wrong?.name} entry"`);
    expect(m).toContain('<span aria-hidden="true">×9</span><span class="sr-only">9 times</span>');
    expect(m).toContain('<span class="sr-only">2 times</span>');
  });
});

describe("the pill, rebuilding and unreadable chats", () => {
  const ok = { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false };

  it("says it is recounting, with no check-now button and no coverage", () => {
    const m = statusPillView({ ...ok, rebuilding: true }, false, "Oct 2 – Oct 29 · 20 messages").value;
    expect(m).toContain("Recounting your history…");
    expect(m).not.toContain("sync-now");
    expect(m).not.toContain("20 messages");
  });

  it("counts chats it couldn't read, with the way to the log folder", () => {
    const m = statusPillView({ ...ok, files_with_errors: 3 }, false).value;
    expect(m).toContain('data-action="open-log-folder"');
    expect(m).toContain(">3 chats couldn't be read<");
    expect(m).toContain("Open the log folder");
    expect(statusPillView({ ...ok, files_with_errors: 1 }, false).value).toContain(">1 chat couldn't be read<");
    expect(statusPillView(ok, false).value).not.toContain("open-log-folder");
  });

  it("keeps the failed-check retry ahead of both", () => {
    const m = statusPillView({ ...ok, last_error: "disk full", files_with_errors: 3, rebuilding: true }, false).value;
    expect(m).toContain("Couldn't check. Try again");
  });

  it("holds the scorecard and the glance with one line, no numbers and no buttons to share", () => {
    expect(rebuildingView().value).toContain("Recounting your history…");
    expect(rebuildingView().value).not.toContain("<button");
    expect(glanceRebuildingView().value).toContain("Recounting your history…");
    expect(glanceRebuildingView().value).not.toContain("share");
  });
});

describe("the first read, English and the glance's error", () => {
  it("says it is reading your history, not that all is set", () => {
    const m = onboardingView("Claude Code", true).value;
    expect(m).toContain("Reading your history…");
    expect(m).not.toContain("You're all set.");
    expect(onboardingView("Claude Code", false).value).toContain("You're all set.");
  });

  it("says once in onboarding and once in the method text that scoring reads English", () => {
    expect(onboardingView("").value).toContain("Only English is graded fairly for now.");
    const m = scorecardView(stats(), { ...ctx, view: "prompts" }).value;
    expect(m).toContain("Only English is graded fairly for now.");
    expect(scorecardView(stats(), ctx).value).not.toContain("Only English is graded fairly for now.");
  });

  it("gives the glance an error page of its own, with Try again and Open app", () => {
    const m = glanceErrorView("database is locked").value;
    expect(m).toContain("database is locked");
    expect(m).toContain('data-action="retry"');
    expect(m).toContain('data-action="open-main"');
    expect(m).not.toContain('data-action="tab"');
    safe(glanceErrorView(HOSTILE).value);
  });
});

describe("no chats in a period", () => {
  it("has a heading, keeps the sections, and names the last chat", () => {
    const m = noChatsView({ meta, filter: {}, period: "28d", view: "prompts", lastDay: "2026-10-02" }).value;
    expect(m).toContain('<h1 class="q">No chats in the last 28 days.</h1>');
    expect(m).toContain('aria-label="Scorecard sections"');
    expect(m).toContain('data-action="view" data-id="prompts"');
    expect(m).toContain("Last chat Oct 2, 2026.");
  });

  it("says the last chat is of any AI when a model is picked, and leaves it out when unknown", () => {
    expect(noChatsView({ meta, filter: { family: "Claude" }, period: "all", lastDay: "2026-10-02" }).value).toContain("Last chat with any AI Oct 2, 2026.");
    expect(noChatsView({ meta, filter: {}, period: "all" }).value).not.toContain("Last chat");
  });
});

describe("someone who only imported an export", () => {
  const unranked = { score: null, direction: null, judgment: null, tier: null, next_tier: null, correction_tier: null, verdict: "hmm" as const, to_rank: 4 };
  const view = (importOnly: boolean, view: "overview" | "prompts" | "corrections" = "overview") =>
    scorecardView(stats({ last_day: "2026-09-30", overall: overall(unranked), prompts: { ...stats().prompts, tier: null, to_rank: 4 } }), {
      ...ctx,
      importOnly,
      view,
    }).value;

  it("gets the export's end date and a newer export, not more tasks", () => {
    for (const v of ["overview", "prompts", "corrections"] as const) {
      const m = view(true, v);
      expect(m, v).toContain("Add a newer export");
      expect(m, v).not.toContain("Give it");
      expect(m, v).not.toContain("more tasks to get");
    }
    expect(view(true)).toContain("Your export ends <b>Sep 30</b>");
  });

  it("is told to give it more tasks as before when chats keep arriving", () => {
    expect(view(false)).toContain("Give it <b>4 more tasks</b> with more than one message to get a score.");
    expect(view(false)).not.toContain("Add a newer export");
  });
});

describe("keeping less in Privacy", () => {
  const saved = { level: "snippet" as const, full_days: 30, snippet_days: 365 };

  it("tells keeping less from keeping the same or more", () => {
    expect(tightens(saved, saved)).toBe(false);
    expect(tightens(saved, { ...saved, level: "category" })).toBe(true);
    expect(tightens(saved, { ...saved, level: "full" })).toBe(false);
    expect(tightens(saved, { ...saved, snippet_days: 400 })).toBe(false);
    expect(tightens(saved, { ...saved, snippet_days: 30 })).toBe(true);
    const full = { level: "full" as const, full_days: 30, snippet_days: 365 };
    expect(tightens(full, { ...full, full_days: 10 })).toBe(true);
    expect(tightens(full, { ...full, full_days: 60 })).toBe(false);
  });

  it("ignores a days field the chosen level doesn't use, so no false confirm appears", () => {
    // Snippet keeps no full text, so its full-text days change nothing; category uses neither field.
    expect(tightens(saved, { ...saved, full_days: 10 })).toBe(false);
    const category = { level: "category" as const, full_days: 30, snippet_days: 365 };
    expect(tightens(category, { ...category, full_days: 1, snippet_days: 1 })).toBe(false);
    // Going from snippet to full: full text wasn't kept before, so a short full-text window deletes nothing.
    expect(tightens(saved, { level: "full", full_days: 1, snippet_days: 365 })).toBe(false);
  });

  it("counts the Hall of fame entries that lose their text, only when it can tell", () => {
    const s = stats({ hall_of_fame: [hallEntry(), hallEntry({ human: null, ai: null })] });
    expect(goneByCount(s, { ...saved, level: "category" })).toBe(1);
    expect(goneByCount(s, { ...saved, full_days: 5 })).toBeNull();
    expect(goneByCount(null, { ...saved, level: "category" })).toBeNull();
  });
});
