import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import shipped from "../public/demo.json";
import { demoCall, demoStats, isDemoData, judge } from "./api.demo";
import { periodSince } from "./state";
import { hallEntry, meta, progress, prompts, row, stats, week } from "./test/fixtures";

const base = stats({
  corrections: 100,
  corrections_last_7d: 20,
  messages: 400,
  models: [row("Anthropic", "Claude Opus", "5", 75, 300, 200), row("OpenAI", "GPT Astra", "6", 25, 100)],
  prompts: prompts({ continued: 110, window_tasks: 40, window_landed: 38 }),
  categories: { just_wrong: 8, made_it_up: 1 },
  weekly: [{ week: "2026-10-19", messages: 40, corrections: 12, continued: 10, landed: 8, humbled: 0, categories: {}, steering: {} }],
  hall_of_fame: [hallEntry({ id: 1, model: "Claude Opus 5" }), hallEntry({ id: 2, model: "GPT Astra 6" })],
});
const recent = {
  day: "2026-10-27",
  graded_day: "2026-10-28",
  source: "claude-code",
  grade: "B",
  missing: "finish",
  extra_cost: 0.3,
  extra_tokens: 1000,
  extra_minutes: 4,
};
const data = { meta, stats: base, progress: progress(), recent_report_cards: [recent] };

describe("demoStats", () => {
  it("returns the sample as is with no filter", () => {
    expect(demoStats(data, {})).toBe(base);
    expect(demoStats(data, { source: null })).toBe(base);
  });

  it("narrows to the picked models and scales the rest by their share of corrections", () => {
    const s = demoStats(data, { provider: "OpenAI" });
    expect(s.models.map((m) => m.family)).toEqual(["GPT Astra"]);
    expect(s.corrections).toBe(25);
    expect(s.messages).toBe(100);
    expect(s.corrections_last_7d).toBe(5); // 20 × 25%
    expect(s.weekly[0]).toMatchObject({ corrections: 3, messages: 10 });
    expect(s.categories).toEqual({ just_wrong: 2 }); // made_it_up rounds to 0 and is dropped
    expect(s.hall_of_fame.map((h) => h.id)).toEqual([2]);
  });

  it("scores the narrowed window the way Rust would", () => {
    // Opus holds 100 of the 110 tasks with a follow-up, so 36 of the window's 40 and 35 of its 38 landed.
    const s = demoStats(data, { provider: "Anthropic" });
    expect(s.prompts).toMatchObject({ tasks: 200, continued: 100, landed: 80, window_tasks: 36, window_landed: 35 });
    expect(s.overall).toEqual(judge(meta, { continued: 36, landed: 35, corrections: 31, humbled: 2, days: 28 }));
    // 2 corrections on 36 tasks (5.6%) cost 28 judgment points: Sharp-eyed, not Irreplaceable.
    expect(s.overall).toMatchObject({ score: 89, judgment: 72, tier: "sharp_eyed", verdict: "no", next_tier: "irreplaceable" });
  });

  it("gives no score when the narrowed window is too thin, and says how many tasks are missing", () => {
    const s = demoStats(data, { provider: "OpenAI" });
    expect(s.overall).toMatchObject({ score: null, tier: null, verdict: "hmm", to_rank: 6 });
  });

  it("matches on family and version too", () => {
    expect(demoStats(data, { family: "Claude Opus", version: "5" }).corrections).toBe(75);
    expect(demoStats(data, { family: "Claude Opus", version: "9" }).corrections).toBe(0);
  });
});

describe("judge", () => {
  const w = (continued: number, landed: number, corrections: number, humbled: number, days = 28) => ({ continued, landed, corrections, humbled, days });

  it("matches the Rust score for the author's month", () => {
    // engine/src/tier.rs: 125 of 150 landed, 41 corrections to 2.
    expect(judge(meta, w(150, 125, 41, 2))).toEqual({
      score: 86,
      direction: 83,
      judgment: 93,
      tier: "sharp_eyed",
      next_tier: "irreplaceable",
      verdict: "no",
      correction_tier: "hawk_eyed",
      next_correction_tier: null,
      corrections: 41,
      humbled: 2,
      to_rank: 0,
    });
  });

  it("matches the other Rust cases", () => {
    expect(judge(meta, w(100, 92, 10, 0))).toMatchObject({ tier: "irreplaceable", next_tier: null });
    // Landing everything but never correcting anything: no proof you check its work.
    expect(judge(meta, w(50, 50, 0, 0))).toMatchObject({ judgment: 0, score: 70, tier: "coworker" });
    // The AI keeps correcting you: on 18% of tasks, 10 points are left.
    expect(judge(meta, w(50, 40, 3, 9)).judgment).toBe(10);
    expect(judge(meta, w(50, 40, 3, 30)).judgment).toBe(0);
    // More landed than continued is clamped.
    expect(judge(meta, w(10, 99, 3, 0)).direction).toBe(100);
  });

  it("gives nulls and the tasks still needed below min_prompts", () => {
    expect(judge(meta, w(meta.min_prompts - 1, 9, 50, 0))).toEqual({
      score: null,
      direction: null,
      judgment: null,
      tier: null,
      next_tier: null,
      verdict: "hmm",
      correction_tier: null,
      next_correction_tier: null,
      corrections: 50,
      humbled: 0,
      to_rank: 1,
    });
    expect(judge(meta, w(0, 0, 0, 0)).to_rank).toBe(meta.min_prompts);
    expect(judge(meta, w(meta.min_prompts, 0, 0, 0)).score).toBe(0);
  });

  it("scores the same with fewer corrections once past the proof", () => {
    const many = judge(meta, w(100, 85, 40, 0));
    const few = judge(meta, w(100, 85, meta.correction_proof, 0));
    expect(few.score).toBe(many.score);
    // Even when it has corrected you: judgment never compares corrections with corrections.
    for (const humbled of [1, 2, 5]) expect(judge(meta, w(100, 85, meta.correction_proof, humbled)).score).toBe(judge(meta, w(100, 85, 40, humbled)).score);
    const one = judge(meta, w(100, 85, 1, 0)).score ?? -1;
    const two = judge(meta, w(100, 85, 2, 0)).score ?? -1;
    expect(one).toBeLessThan(two);
    expect(two).toBeLessThan(few.score ?? -1);
  });

  it("matches Rust's expected scores, table by table", () => {
    // [continued, landed, corrections, humbled, days, score]; engine/src/tier.rs `the_correction_proof_follows_the_window`.
    const table: [number, number, number, number, number, number][] = [
      [100, 80, 6, 0, 28, 86],
      [20, 16, 1, 0, 7, 86],
      [20, 16, 1, 0, 28, 65], // a week's habits in a month's window show only a third of the proof
      [100, 80, 6, 0, 90, 86], // longer than the default counts the same as 28 days
      [100, 80, 6, 0, 36500, 86], // "all" is a long window
      [100, 80, 2, 0, 14, 86], // 2 of the 2 corrections a fortnight needs
    ];
    for (const [continued, landed, corrections, humbled, days, score] of table)
      expect(judge(meta, w(continued, landed, corrections, humbled, days)).score, `${continued}/${landed}/${corrections}/${humbled} over ${days}d`).toBe(score);
  });

  it("never lets landing more cost points", () => {
    let last = -1;
    for (let landed = 0; landed <= 100; landed++) {
      const score = judge(meta, w(100, landed, 5, 1)).score ?? -1;
      expect(score).toBeGreaterThanOrEqual(last);
      last = score;
    }
  });

  it("says hmm below Coworker and no from it up", () => {
    expect(judge(meta, w(50, 40, 3, 9)).verdict).toBe("hmm"); // score 63
    expect(judge(meta, w(50, 50, 0, 0)).verdict).toBe("no"); // score 70
  });
});

describe("isDemoData", () => {
  it("accepts the shipped demo.json", () => {
    expect(isDemoData(shipped)).toBe(true);
    if (!isDemoData(shipped)) return;
    // Every hall entry carries the display strings Rust now sends (a correction of you has no category).
    for (const h of shipped.stats.hall_of_fame) {
      expect(h.source_name).toBeTruthy();
      expect(Boolean(h.category_title)).toBe(Boolean(h.category));
    }
    expect(shipped.meta.tiers.map((t) => t.id)).toContain(shipped.stats.overall.tier);
  });

  it("rejects the old shape (bare stats, no overall, no progress)", () => {
    expect(isDemoData(base)).toBe(false);
    expect(isDemoData({ meta, stats: base })).toBe(false); // no progress
    expect(isDemoData({ ...data, stats: { ...base, overall: undefined } })).toBe(false);
    expect(isDemoData({ ...data, stats: { ...base, overall: { ...base.overall, verdict: "maybe" } } })).toBe(false);
    expect(isDemoData(null)).toBe(false);
  });

  it("rejects a demo without the report card pieces", () => {
    const { recent_report_cards: _recent, ...noRecent } = data;
    expect(isDemoData(noRecent)).toBe(false);
    expect(isDemoData({ ...data, meta: { ...meta, habits: undefined } })).toBe(false);
    expect(isDemoData({ ...data, progress: { ...data.progress, weeks: [{ ...week("2026-10-05"), report_cards: undefined }] } })).toBe(false);
  });

  it("has report cards to show in every piece of Improve", () => {
    if (!isDemoData(shipped)) throw new Error("not demo data");
    const weeks = shipped.progress.weeks;
    expect(weeks.filter((w) => w.report_cards.graded > 0).length).toBeGreaterThan(5);
    expect(weeks.some((w) => w.report_cards.graded >= 1 && w.report_cards.graded < shipped.meta.min_card_week)).toBe(true); // a hollow dot
    expect(weeks.some((w) => w.report_cards.graded > 0 && w.report_cards.extra_cost == null)).toBe(true); // an unpriced week
    expect(weeks.every((w) => (w.report_cards.extra_cost == null) === (w.report_cards.priced === 0))).toBe(true);
    expect(weeks.some((w) => w.report_cards.priced > 0 && w.report_cards.priced < w.report_cards.graded)).toBe(true); // a partly priced week
    expect(shipped.progress.bests.report_card).not.toBeNull();
    expect(shipped.progress.insights.report_cards?.habit).not.toBeNull();
    expect(shipped.recent_report_cards).toHaveLength(5);
  });
});

describe("demoCall", () => {
  it("answers every read with sample data and accepts writes", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ json: async () => data })),
    );
    expect(await demoCall("meta", undefined)).toBe(meta);
    expect((await demoCall("stats", { filter: {} })).corrections).toBe(100);
    expect(await demoCall("allow_source", { id: "codex", since: null, fromNow: true })).toBeNull();
    expect((await demoCall("get_retention", undefined)).level).toBe("snippet");
    expect((await demoCall("list_sources", undefined)).length).toBeGreaterThan(0);
    expect((await demoCall("sync_status", undefined)).syncing).toBe(false);
    vi.unstubAllGlobals();
  });

  it("gives a dated stats call counts that match its score, even for the 28-day default", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ json: async () => data })),
    );
    const s = await demoCall("stats", { filter: { since: "2026-10-19" } });
    expect(s.overall).toMatchObject({ corrections: s.corrections, humbled: s.humbled });
    vi.unstubAllGlobals();
  });

  it("scores an undated call over 28 days, so the quick glance and the scorecard agree", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ json: async () => data })),
    );
    const undated = await demoCall("stats", { filter: {} });
    const dated = await demoCall("stats", { filter: { since: periodSince("28d") } });
    expect(undated.overall).toEqual(dated.overall);
    vi.unstubAllGlobals();
  });

  describe("learn and improve", () => {
    beforeEach(() => {
      vi.stubGlobal(
        "fetch",
        vi.fn(async () => ({ json: async () => data })),
      );
    });
    afterEach(() => vi.unstubAllGlobals());

    it("answers the recent report cards", async () => {
      expect(await demoCall("recent_report_cards", { filter: {} })).toEqual([recent]);
    });

    it("answers progress, and shows the goal set during the visit", async () => {
      expect((await demoCall("progress", { filter: {} })).goal).toBeNull();
      const goal = await demoCall("set_goal", { kind: { kind: "habit", signal: "why" }, target: 40 });
      expect(goal).toMatchObject({ kind: "habit", signal: "why", target: 40 });
      expect((await demoCall("progress", { filter: {} })).goal).toMatchObject({ goal, weeks: [], met: 0, done: false, needed: 4 });
      await demoCall("set_goal", { kind: { kind: "tier", tier: "coworker" }, target: 0 });
      expect((await demoCall("progress", { filter: {} })).goal?.needed).toBe(0);
      expect(await demoCall("clear_goal", undefined)).toBeNull();
      expect((await demoCall("progress", { filter: {} })).goal).toBeNull();
    });

    it("can't grade a draft in the browser", async () => {
      await expect(demoCall("check_prompt", { text: "hi" })).rejects.toThrow("runs in the app");
    });

    it("remembers which lessons are done for the visit, once each", async () => {
      expect(await demoCall("learned", undefined)).toEqual([]);
      await demoCall("set_learned", { id: "a", done: true });
      expect(await demoCall("set_learned", { id: "b", done: true })).toEqual(["a", "b"]);
      expect(await demoCall("set_learned", { id: "a", done: true })).toEqual(["b", "a"]);
      expect(await demoCall("set_learned", { id: "b", done: false })).toEqual(["a"]);
    });

    it("shows report cards on in Claude Code with the day its latest card was graded, and Codex not installed", async () => {
      expect(await demoCall("report_cards_status", { look: false })).toEqual({
        claude_code: { installed_tool: true, on: true, last_card_day: "2026-10-28", problem: null, left_out: 0, unavailable: null },
        codex: { installed_tool: false, on: false, last_card_day: null, problem: null, left_out: 0, unavailable: null },
        tips: false,
        tip_activity: { active: 0, tips: 0, blocked: false },
        reminders: true,
        translocated: false,
      });
    });

    it("remembers report card choices for the visit and changes nothing else", async () => {
      const look = { look: false };
      expect(await demoCall("report_cards_turn_off", { tool: "claude_code" })).toBeNull();
      expect((await demoCall("report_cards_status", look)).claude_code.on).toBe(false);
      await demoCall("report_cards_turn_on", { tool: "claude_code" });
      expect((await demoCall("report_cards_status", look)).claude_code.on).toBe(true);
      await demoCall("report_cards_turn_on", { tool: "codex" });
      expect((await demoCall("report_cards_status", look)).codex).toEqual({
        installed_tool: false,
        on: false,
        last_card_day: null,
        problem: null,
        left_out: 0,
        unavailable: null,
      });
      await demoCall("report_cards_set_tips", { on: true });
      expect((await demoCall("report_cards_status", look)).tips).toBe(true);
      await demoCall("report_cards_set_tips", { on: false });
      await demoCall("report_cards_set_reminders", { on: false });
      expect((await demoCall("report_cards_status", look)).reminders).toBe(false);
      await demoCall("report_cards_set_reminders", { on: true });
      expect(await demoCall("report_cards_delete_history", undefined)).toBe(0);
    });

    it("remembers the menu bar setting", async () => {
      expect(await demoCall("get_menu_bar", undefined)).toEqual({ enabled: true, available: true });
      await demoCall("set_menu_bar", { enabled: false });
      expect(await demoCall("get_menu_bar", undefined)).toEqual({ enabled: false, available: true });
    });
  });
});

describe("the demo's date", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("keeps its score on a date far in the future", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ json: async () => data })),
    );
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-28T12:00:00"));
    const now = await demoCall("stats", { filter: {} });
    const nowPeriod = await demoCall("stats", { filter: { since: periodSince("28d") } });
    vi.setSystemTime(new Date("2040-06-15T12:00:00"));
    const later = await demoCall("stats", { filter: {} });
    expect(later.overall).toEqual(now.overall);
    expect(later.overall.score).not.toBeNull();
    expect(await demoCall("stats", { filter: { since: periodSince("28d") } })).toEqual(nowPeriod);
  });
});

describe("withSince", () => {
  it("shrinks the counts to the period and scores them again", async () => {
    const { withSince } = await import("./api.demo");
    const { stats } = await import("./test/fixtures");
    const s = stats();
    const last = s.weekly[s.weekly.length - 1]?.week ?? "";
    const p = withSince(meta, s, last);
    expect(p.weekly).toHaveLength(1);
    expect(p.corrections).toBeLessThan(s.corrections);
    expect(p.overall).toEqual(
      judge(meta, { continued: p.prompts.window_tasks, landed: p.prompts.window_landed, corrections: p.corrections, humbled: p.humbled, days: 2 }),
    );
    expect(withSince(meta, s, null)).toBe(s);
  });
});
