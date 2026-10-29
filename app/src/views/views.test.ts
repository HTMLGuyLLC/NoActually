import { describe, expect, it } from "vitest";
import { HOSTILE, hallEntry, meta, overall, row, source, stats } from "../test/fixtures";
import { headerView, statusPillView } from "./chrome";
import { emptyGlanceView, glanceView } from "./glance";
import { chartSummary, chartView, hallView, onboardingView, scorecardView } from "./scorecard";
import { parseRetention, settingsView } from "./settings";
import { allowArgs, sourcesView, watchingNames } from "./sources";

const ctx = { meta, filter: {}, still: false };
/** No hostile markup made it through as markup. */
const safe = (markup: string) => {
  expect(markup).not.toContain("<img");
  expect(markup).not.toContain('onerror="');
};

describe("scorecardView", () => {
  it("opens on an overview with both verdicts and the section tabs", () => {
    const m = scorecardView(stats(), ctx).value;
    expect(m).toContain('aria-label="Scorecard sections"');
    expect(m).toMatch(
      /data-id="overview" data-tip="Overview" aria-current="page"><svg class="tab-icon"[^>]*aria-hidden="true">.*?<\/svg><span class="tab-name">Overview<\/span><\/button>/,
    );
    expect(m).toContain('data-id="prompts" data-tip="Prompting"');
    expect(m).toContain("Could ");
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain("Do you correct its mistakes?");
    expect(m).toContain('data-action="view" data-id="corrections"');
    expect(m).not.toContain("Hall of fame ·");
  });

  it("puts exactly one model picker in every section", () => {
    for (const view of ["overview", "corrections", "prompts", "hall"] as const) {
      const m = scorecardView(stats(), { ...ctx, view }).value;
      expect(m.split('id="model-trigger"').length - 1, view).toBe(1);
    }
    expect(scorecardView(stats(), { ...ctx, view: "prompts" }).value).toContain("Where steering went");
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
    expect(m).toContain('aria-hidden="true">Usually.</div>');
    expect(m).toContain('<ol class="ladder" aria-label="Correction tiers">');
    expect(m).toContain('<span>Hawk-eyed</span><span><b class="at">95</b>');
    expect(m).toContain("<span>Spot-checker</span><span>50–74</span>");
    expect(m).toContain('data-action="share-corrections">Share your corrections</button>');
    const unranked = scorecardView(stats({ overall: overall({ judgment: null, correction_tier: null, to_rank: 3 }) }), { ...ctx, view: "corrections" }).value;
    expect(unranked).toContain(">Hmm.</div>");
    expect(unranked).not.toContain("share-corrections");
  });

  it("shows you vs it as one tug-of-war bar, keeping a sliver for a tiny side", () => {
    const view = (corrections: number, humbled: number) => scorecardView(stats({ corrections, humbled }), { ...ctx, view: "corrections" }).value;
    const yours = (corrections: number, humbled: number) => view(corrections, humbled).match(/<i class="you" data-w="(\d+)"><\/i>/)?.[1];
    expect(yours(37, 1)).toBe("97");
    expect(yours(180, 1)).toBe("98");
    expect(yours(1, 180)).toBe("2");
    expect(yours(5, 0)).toBe("100");
    expect(yours(0, 5)).toBe("0");
    expect(yours(0, 0)).toBe("50");
    expect(view(0, 0)).toContain('class="tug empty"');
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
      hall_of_fame: [hallEntry({ human: HOSTILE, ai: HOSTILE, model: HOSTILE, category_title: HOSTILE, label: HOSTILE, day: HOSTILE })],
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
    expect(m).toContain("Last 28 days");
    expect(m).toContain("Score <b>86</b>. Your tasks landed <b>83%</b> of the time, and you corrected it <b>41 times</b> to its 2 in the last 28 days.");
    expect(m).toContain("Your tier is Sharp-eyed, score 86.");
    expect(m).not.toContain("Irreplaceable at 90. You&#39;re at 86."); // the row says it
    expect(m).toContain('data-action="open-share"');
    // The overview shares the score; Prompts and Corrections each share their own answer.
    expect(m).not.toContain("share-prompts");
    expect(m).not.toContain("share-corrections");
  });

  it("answers Do you correct its mistakes? in a word on the overview, with its tier", () => {
    const m = hero({ correction_tier: "reviewer", next_correction_tier: "hawk_eyed", judgment: 88 }, { corrections: 41, humbled: 2 });
    expect(m).toContain('aria-hidden="true">Often.</div>');
    expect(m).toContain("You corrected it <b>41 times</b> in the last 28 days. It corrected you 2 times.");
    expect(m).toContain("<span>Tier</span>Reviewer");
    expect(m).toContain("Hawk-eyed at 90. You&#39;re at 88.");
  });

  it("shows the two parts as bars with their weights from meta", () => {
    const m = hero();
    expect(m).toContain('aria-label="What the score is made of"');
    expect(m).toContain("Your prompts<small");
    expect(m).toContain("tasks that landed · 70% of the score");
    expect(m).toContain('data-w="83"');
    expect(m).toContain("Who corrects whom<small");
    expect(m).toContain("how rarely it has to correct you");
    expect(m).toContain("· 30% of the score");
    expect(m).toContain('data-w="95"');
    expect(hero({}, {}).split('class="part"')).toHaveLength(3);
  });

  it("marks the top tier's row with your score", () => {
    expect(hero({ tier: "irreplaceable", next_tier: null, score: 93 })).toContain('<span>Irreplaceable</span><span><b class="at">93</b>');
  });

  it("asks for more tasks, and offers nothing to share, until there is a score", () => {
    const m = hero({ score: null, direction: null, judgment: null, tier: null, next_tier: null, verdict: "hmm", to_rank: 7 });
    expect(m).toContain("<b>7 more tasks</b> with a follow-up to get a score");
    expect(m).toContain("7 more tasks to get a score");
    expect(m).toContain("Not scored yet.");
    expect(m).toContain('data-w="0"');
    expect(m).not.toContain('data-action="open-share"');
    expect(m).not.toMatch(/<li class="on" aria-current="true"/); // no tier on the ladder
  });

  it("says task, not tasks, for one", () => {
    expect(hero({ score: null, tier: null, next_tier: null, verdict: "hmm", to_rank: 1 })).toContain("<b>1 more task</b> with");
  });

  it("says why fewer corrections never costs points", () => {
    const m = scorecardView(stats(), { ...ctx, view: "corrections" }).value;
    expect(m).toContain("3 corrections prove you check its work. Correcting it less because it got it right never costs you.");
  });

  it("escapes names from meta and unknown ids", () => {
    const bad = { ...meta, tiers: meta.tiers.map((t) => ({ ...t, name: HOSTILE })) };
    safe(scorecardView(stats(), { ...ctx, meta: bad }).value);
    safe(scorecardView(stats({ overall: overall({ tier: HOSTILE, next_tier: HOSTILE }) }), ctx).value);
  });
});

describe("hallView", () => {
  it("uses Rust's category title, never the label, and the source name when no model was recorded", () => {
    const m = hallView([hallEntry({ category_title: "Made it up", label: "confirmed", model: "", source_name: "ChatGPT (export)" })]).value;
    expect(m).toContain('<span class="chip">Made it up</span>');
    expect(m).toContain("<b>ChatGPT:</b>");
  });

  it("marks pinned corrections and says how to unpin", () => {
    const m = hallView([hallEntry({ pinned: true })]).value;
    expect(m).toContain("Pinned · kept forever");
    expect(m).toContain('data-pinned="true"');
    expect(m).toContain(">Unpin<");
  });

  it("explains an empty hall", () => {
    expect(hallView([]).value).toContain("No saved corrections to show.");
  });
});

describe("chartView", () => {
  it("labels the busiest week once without coloring it as the good one", () => {
    const m = chartView(stats()).value;
    expect(m).not.toContain("hot");
    expect(m.match(/class="peak-label">Most: /g)).toHaveLength(1);
    expect(m).toContain('aria-label="Corrections per week"');
    expect(m).toContain('aria-describedby="chart-summary"');
  });

  it("says when there are no weeks", () => {
    expect(chartView(stats({ weekly: [] })).value).toContain("No weeks yet.");
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
    const m = headerView("settings", { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 }, false).value;
    expect(m).toContain('data-id="settings" data-tip="Settings" aria-current="page"');
    expect(m.match(/aria-current/g)).toHaveLength(1);
  });

  it("gives every tab an icon and makes the logo a way home", () => {
    const m = headerView("settings", { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 }, false).value;
    expect(m.match(/class="tab-icon"/g)).toHaveLength(4);
    expect(m).toContain('<button class="home" data-action="home"');
  });

  it("escapes a sync error in the retry pill", () => {
    const m = statusPillView({ syncing: false, last_sync: null, last_error: HOSTILE, files_with_errors: 0 }, false).value;
    expect(m).toContain("Couldn't check. Retry");
    safe(m);
  });

  it("says Demo data in the demo, and Checking while syncing", () => {
    expect(statusPillView({ syncing: false, last_sync: null, last_error: null, files_with_errors: 0 }, true).value).toContain("Demo data");
    expect(statusPillView({ syncing: true, last_sync: null, last_error: null, files_with_errors: 0 }, false).value).toContain("Checking…");
  });
});

const status = { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 };

describe("sourcesView", () => {
  it("checks for new chats from Sources, and leaves app updates to General", () => {
    const m = sourcesView([source()], status).value;
    expect(m).toContain('data-action="sync-now">Check for updates</button>');
    expect(m).toContain('id="sync-line">New chats last checked');
    const g = settingsView({
      retention: { level: "snippet", full_days: 30, snippet_days: 365 },
      autostart: false,
      menuBar: true,
      update: null,
      demo: false,
    }).value;
    expect(g).toContain(">Check for app updates</button>");
    expect(g).not.toContain("sync-now");
  });

  it("offers Allow with a how-far-back choice for a found, unallowed source", () => {
    const m = sourcesView([source()], status).value;
    expect(m).toContain('data-action="allow" data-id="claude-code"');
    expect(m).toContain('<option value="none">From now on</option>');
    expect(m).not.toContain('data-action="forget"');
  });

  it("offers pause, stop and forget once allowed", () => {
    const state = { id: "claude-code", allowed: true, paused: true, backfill_since: null, granted_at: null, start_after: null, paths: [] };
    const m = sourcesView([source({ state })], status).value;
    expect(m).toContain(">Resume<");
    expect(m).toContain('data-paused="true"');
    expect(m).toContain('data-action="forget"');
  });

  it("explains how to get an export", () => {
    expect(sourcesView([source({ id: "chatgpt-export", kind: "import", roots: [] })], status).value).toContain("Settings → Data controls → Export data.");
  });

  it("escapes names and paths", () => {
    safe(sourcesView([source({ name: HOSTILE, roots: [[HOSTILE, true]] })], status).value);
  });

  it("lists what's being read right now", () => {
    const on = { id: "x", allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] };
    const list = [source({ name: "ChatGPT (export)", state: on }), source({ name: "Codex", state: { ...on, paused: true } }), source()];
    expect(watchingNames(list)).toEqual(["ChatGPT"]);
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
    retention: { level: "snippet" as const, full_days: 30, snippet_days: 365 },
    autostart: true,
    menuBar: true,
    update: null,
    demo: false,
  };

  it("checks the saved level and toggles", () => {
    const m = settingsView(base).value;
    expect(m).toContain('value="snippet" data-action="level" checked');
    expect(m).toContain('id="autostart" data-action="autostart" type="checkbox" checked');
    expect(m).toContain('max="3650"');
  });

  it("checks the menu-bar toggle when it's on, and leaves it clear when off", () => {
    expect(settingsView(base).value).toContain('id="menu-bar" data-action="menu-bar" type="checkbox" checked');
    expect(settingsView(base).value).toContain("Keep running in the menu bar");
    const off = settingsView({ ...base, menuBar: false }).value;
    expect(off).toContain('id="menu-bar" data-action="menu-bar" type="checkbox"/>');
    expect(off).toContain("Opens the app when you log in.");
  });

  it("disables what the demo can't do", () => {
    const m = settingsView({ ...base, demo: true }).value;
    expect(m).toContain('data-action="check-update" disabled');
    expect(m).toContain('id="menu-bar" data-action="menu-bar" type="checkbox" checked disabled');
  });
});

describe("scorecard period picker", () => {
  it("marks the period, words the correction count for it, and uses it for the score too", async () => {
    const { scorecardView } = await import("./scorecard");
    const view = (period: "7d" | "28d" | "90d" | "all") =>
      scorecardView(stats({ overall: overall({ corrections: 37 }) }), { meta, filter: {}, still: true, period }).value;
    const week = view("7d");
    expect(week).toContain('data-action="period" data-id="7d" aria-current="true"');
    expect(week).toContain("you corrected it <b>37 times</b> to its 2 in the last 7 days.");
    // Everything follows the period, the score included: no disclaimer needed.
    expect(week).toContain('<div class="eyebrow">Last 7 days</div>');
    expect(week).not.toContain("always uses");
    expect(view("all")).toContain('<div class="eyebrow">All time</div>');
    expect(view("all")).toContain("<b>37 times</b> to its 2 ever.");
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
    const m = statusPillView({ syncing: false, last_sync: null, last_error: null, files_with_errors: 0 }, false, covers).value;
    expect(m).toContain('data-action="sync-now"');
    expect(m).toContain('data-tip="Check now"');
    expect(m).not.toContain("title=");
    expect(m).toContain(`data-tip="${covers}"`);
    expect(coverage(null)).toBe("");
  });

  it("drops the 'pick one from All AI' line and the date line from the scorecard", async () => {
    const { scorecardView } = await import("./scorecard");
    const m = scorecardView(stats(), { meta, filter: {}, still: true }).value;
    expect(m).not.toContain("Pick one from");
    expect(m).not.toContain("messages</span></section>");
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
    expect(m).toContain("<summary>1 more</summary>");
    expect(m.indexOf("best one")).toBeLessThan(m.indexOf("runner up"));
    expect(m).toContain('class="award unearned"');
    expect(m).toContain(AWARDS[""]?.name ?? "?");
    expect(m).not.toContain("<img");
  });
});
