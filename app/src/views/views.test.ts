import { describe, expect, it } from "vitest";
import { HOSTILE, hallEntry, meta, row, source, stats } from "../test/fixtures";
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
    expect(m).toContain('data-id="overview" aria-current="page">Overview</button>');
    expect(m).toContain("Could ");
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain('data-action="view" data-id="catches"');
    expect(m).not.toContain("Hall of fame ·");
  });

  it("puts exactly one model picker in every section", () => {
    for (const view of ["overview", "catches", "briefs", "hall"] as const) {
      const m = scorecardView(stats(), { ...ctx, view }).value;
      expect(m.split('id="model-trigger"').length - 1, view).toBe(1);
    }
    expect(scorecardView(stats(), { ...ctx, view: "briefs" }).value).toContain("Where steering went");
    expect(scorecardView(stats(), { ...ctx, view: "hall" }).value).toContain('data-action="pin"');
  });

  it("draws the verdict, ladder and breakdowns from Rust's data", () => {
    const m = scorecardView(stats(), { ...ctx, view: "catches" }).value;
    expect(m).toContain(">No.</div>");
    expect(m).toContain('<li class="on" aria-current="true"><span>Irreplaceable</span><span>15+ catches/wk</span></li>');
    expect(m).toContain("Flat-out wrong"); // category title from meta
    expect(m).toContain('data-w="100"'); // the top category's bar; width set through CSSOM
    expect(m).not.toContain("style=");
  });

  it("shows Hmm. and the next tier by name when Rust says hmm", () => {
    const m = scorecardView(stats({ verdict: "hmm", tier: "rubber_stamp", next_tier: ["coworker", 2] }), ctx).value;
    expect(m).toContain("no maybe");
    expect(m).toContain("2 more this week for Coworker");
  });

  it("keeps the scrawl still when the verdict didn't change", () => {
    expect(scorecardView(stats(), { ...ctx, still: true }).value).toContain('class="no still"');
  });

  it("offers to clear a filter, naming it", () => {
    const m = scorecardView(stats(), { ...ctx, filter: { provider: "Anthropic", family: "Claude Opus", version: "5" } }).value;
    expect(m).toContain("Showing Claude Opus 5 only");
    expect(m).toContain('data-action="clear-filter"');
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

describe("hallView", () => {
  it("uses Rust's category title, never the label, and the source name when no model was recorded", () => {
    const m = hallView([hallEntry({ category_title: "Made it up", label: "confirmed", model: "", source_name: "ChatGPT (export)" })]).value;
    expect(m).toContain('<span class="chip">Made it up</span>');
    expect(m).toContain("<b>ChatGPT:</b>");
  });

  it("marks pinned catches and says how to unpin", () => {
    const m = hallView([hallEntry({ pinned: true })]).value;
    expect(m).toContain("Pinned · kept forever");
    expect(m).toContain('data-pinned="true"');
    expect(m).toContain(">Unpin<");
  });

  it("explains an empty hall", () => {
    expect(hallView([]).value).toContain("No saved catches to show.");
  });
});

describe("chartView", () => {
  it("draws the top tier's floor as the line and marks weeks over it", () => {
    const m = chartView(stats(), meta).value;
    expect(m).toContain("irreplaceable line");
    expect(m).toContain('class="bar hot"');
    expect(m).toContain('aria-describedby="chart-summary"');
  });

  it("says when there are no weeks", () => {
    expect(chartView(stats({ weekly: [] }), meta).value).toContain("No weeks yet.");
  });

  it("summarises the chart in words for screen readers", () => {
    const t = chartSummary(stats().weekly, "Irreplaceable", 15);
    expect(t).toMatch(/^2 weeks from .+\. Most catches: 16, the week of .+\. 1 week reached the irreplaceable line of 15\.$/);
    expect(chartSummary([], "Irreplaceable", 15)).toBe("No weeks yet.");
  });
});

describe("onboarding and glance", () => {
  it("welcomes a new person, or says what's being watched", () => {
    expect(onboardingView("").value).toContain("Choose sources");
    const m = onboardingView(HOSTILE).value;
    expect(m).toContain("You're all set.");
    safe(m);
  });

  it("shows the tier by name in the glance", () => {
    const m = glanceView(stats({ tier: "sharp_eyed", next_tier: ["irreplaceable", 3] }), meta).value;
    expect(m).toContain("Sharp-eyed");
    expect(m).toContain("3 more for Irreplaceable");
    expect(m).toContain('role="status"'); // copy confirmation is announced, not the button itself
    expect(glanceView(stats(), meta).value).toContain("Top tier");
  });

  it("has an empty glance before the first catch", () => {
    expect(emptyGlanceView("").value).toContain("Set up");
    safe(emptyGlanceView(HOSTILE).value);
  });
});

describe("chrome", () => {
  it("marks the current tab for assistive tech", () => {
    const m = headerView("sources", { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 }, false).value;
    expect(m).toContain('data-id="sources" aria-current="page"');
    expect(m.match(/aria-current/g)).toHaveLength(1);
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

describe("sourcesView", () => {
  it("offers Allow with a how-far-back choice for a found, unallowed source", () => {
    const m = sourcesView([source()]).value;
    expect(m).toContain('data-action="allow" data-id="claude-code"');
    expect(m).toContain('<option value="none">From now on</option>');
    expect(m).not.toContain('data-action="forget"');
  });

  it("offers pause, stop and forget once allowed", () => {
    const state = { id: "claude-code", allowed: true, paused: true, backfill_since: null, granted_at: null, start_after: null, paths: [] };
    const m = sourcesView([source({ state })]).value;
    expect(m).toContain(">Resume<");
    expect(m).toContain('data-paused="true"');
    expect(m).toContain('data-action="forget"');
  });

  it("explains how to get an export", () => {
    expect(sourcesView([source({ id: "chatgpt-export", kind: "import", roots: [] })]).value).toContain("Settings → Data controls → Export data.");
  });

  it("escapes names and paths", () => {
    safe(sourcesView([source({ name: HOSTILE, roots: [[HOSTILE, true]] })]).value);
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
    update: null,
    status: { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 },
    demo: false,
  };

  it("checks the saved level and toggles", () => {
    const m = settingsView(base).value;
    expect(m).toContain('value="snippet" data-action="level" checked');
    expect(m).toContain('id="autostart" data-action="autostart" type="checkbox" checked');
    expect(m).toContain('max="3650"');
    expect(m).toContain("Last checked never.");
  });

  it("disables what the demo can't do", () => {
    expect(settingsView({ ...base, demo: true }).value).toContain('data-action="check-update" disabled');
  });
});
