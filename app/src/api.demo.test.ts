import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import shipped from "../public/demo.json";
import { demoCall, demoStats, isDemoData, judge } from "./api.demo";
import { briefs, hallEntry, meta, progress, row, stats } from "./test/fixtures";

const base = stats({
  catches: 100,
  catches_last_7d: 20,
  messages: 400,
  models: [row("Anthropic", "Claude Opus", "5", 75, 300, 200), row("OpenAI", "GPT Astra", "6", 25, 100)],
  briefs: briefs({ continued: 110, window_tasks: 40, window_landed: 38 }),
  categories: { just_wrong: 8, made_it_up: 1 },
  weekly: [{ week: "2026-10-19", messages: 40, catches: 12 }],
  hall_of_fame: [hallEntry({ id: 1, model: "Claude Opus 5" }), hallEntry({ id: 2, model: "GPT Astra 6" })],
});
const data = { meta, stats: base, progress: progress() };

describe("demoStats", () => {
  it("returns the sample as is with no filter", () => {
    expect(demoStats(data, {})).toBe(base);
    expect(demoStats(data, { source: null })).toBe(base);
  });

  it("narrows to the picked models and scales the rest by their share of catches", () => {
    const s = demoStats(data, { provider: "OpenAI" });
    expect(s.models.map((m) => m.family)).toEqual(["GPT Astra"]);
    expect(s.catches).toBe(25);
    expect(s.messages).toBe(100);
    expect(s.catches_last_7d).toBe(5); // 20 × 25%
    expect(s.weekly[0]).toMatchObject({ catches: 3, messages: 10 });
    expect(s.categories).toEqual({ just_wrong: 2 }); // made_it_up rounds to 0 and is dropped
    expect(s.hall_of_fame.map((h) => h.id)).toEqual([2]);
  });

  it("scores the narrowed window the way Rust would", () => {
    // Opus holds 100 of the 110 tasks with a follow-up, so 36 of the window's 40 and 35 of its 38 landed.
    const s = demoStats(data, { provider: "Anthropic" });
    expect(s.briefs).toMatchObject({ tasks: 200, continued: 100, landed: 80, window_tasks: 36, window_landed: 35 });
    expect(s.overall).toEqual(judge(meta, { continued: 36, landed: 35, catches: 31, humbled: 2 }));
    // 2 corrections on 36 tasks (5.6%) cost 28 judgment points: Sharp-eyed, not Irreplaceable.
    expect(s.overall).toMatchObject({ score: 89, judgment: 72, tier: "sharp_eyed", verdict: "no", next_tier: "irreplaceable" });
  });

  it("gives no score when the narrowed window is too thin, and says how many tasks are missing", () => {
    const s = demoStats(data, { provider: "OpenAI" });
    expect(s.overall).toMatchObject({ score: null, tier: null, verdict: "hmm", to_rank: 6 });
  });

  it("matches on family and version too", () => {
    expect(demoStats(data, { family: "Claude Opus", version: "5" }).catches).toBe(75);
    expect(demoStats(data, { family: "Claude Opus", version: "9" }).catches).toBe(0);
  });
});

describe("judge", () => {
  const w = (continued: number, landed: number, catches: number, humbled: number) => ({ continued, landed, catches, humbled });

  it("matches the Rust score for the author's month", () => {
    // engine/src/tier.rs: 125 of 150 landed, 41 catches to 2.
    expect(judge(meta, w(150, 125, 41, 2))).toEqual({
      score: 86,
      direction: 83,
      judgment: 93,
      tier: "sharp_eyed",
      next_tier: "irreplaceable",
      verdict: "no",
      catches: 41,
      humbled: 2,
      to_rank: 0,
    });
  });

  it("matches the other Rust cases", () => {
    expect(judge(meta, w(100, 92, 10, 0))).toMatchObject({ tier: "irreplaceable", next_tier: null });
    // Landing everything but never catching anything: no proof you check its work.
    expect(judge(meta, w(50, 50, 0, 0))).toMatchObject({ judgment: 0, score: 70, tier: "coworker" });
    // The AI keeps correcting you: on 18% of tasks, 10 points are left.
    expect(judge(meta, w(50, 40, 3, 9)).judgment).toBe(10);
    expect(judge(meta, w(50, 40, 3, 30)).judgment).toBe(0);
    // More landed than continued is clamped.
    expect(judge(meta, w(10, 99, 3, 0)).direction).toBe(100);
  });

  it("gives nulls and the tasks still needed below min_briefs", () => {
    expect(judge(meta, w(meta.min_briefs - 1, 9, 50, 0))).toEqual({
      score: null,
      direction: null,
      judgment: null,
      tier: null,
      next_tier: null,
      verdict: "hmm",
      catches: 50,
      humbled: 0,
      to_rank: 1,
    });
    expect(judge(meta, w(0, 0, 0, 0)).to_rank).toBe(meta.min_briefs);
    expect(judge(meta, w(meta.min_briefs, 0, 0, 0)).score).toBe(0);
  });

  it("scores the same with fewer catches once past the proof", () => {
    const many = judge(meta, w(100, 85, 40, 0));
    const few = judge(meta, w(100, 85, meta.catch_proof, 0));
    expect(few.score).toBe(many.score);
    // Even when it has corrected you: judgment never compares catches with corrections.
    for (const humbled of [1, 2, 5]) expect(judge(meta, w(100, 85, meta.catch_proof, humbled)).score).toBe(judge(meta, w(100, 85, 40, humbled)).score);
    const one = judge(meta, w(100, 85, 1, 0)).score ?? -1;
    const two = judge(meta, w(100, 85, 2, 0)).score ?? -1;
    expect(one).toBeLessThan(two);
    expect(two).toBeLessThan(few.score ?? -1);
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
    // Every hall entry carries the display strings Rust now sends (a catch of you has no category).
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
});

describe("demoCall", () => {
  it("answers every read with sample data and accepts writes", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ json: async () => data })),
    );
    expect(await demoCall("meta", undefined)).toBe(meta);
    expect((await demoCall("stats", { filter: {} })).catches).toBe(100);
    expect(await demoCall("allow_source", { id: "codex", since: null, fromNow: true })).toBeNull();
    expect((await demoCall("get_retention", undefined)).level).toBe("snippet");
    expect((await demoCall("list_sources", undefined)).length).toBeGreaterThan(0);
    expect((await demoCall("sync_status", undefined)).syncing).toBe(false);
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
      await expect(demoCall("check_brief", { text: "hi" })).rejects.toThrow("runs in the app");
    });

    it("remembers which lessons are done for the visit, once each", async () => {
      expect(await demoCall("learned", undefined)).toEqual([]);
      await demoCall("set_learned", { id: "a", done: true });
      expect(await demoCall("set_learned", { id: "b", done: true })).toEqual(["a", "b"]);
      expect(await demoCall("set_learned", { id: "a", done: true })).toEqual(["b", "a"]);
      expect(await demoCall("set_learned", { id: "b", done: false })).toEqual(["a"]);
    });

    it("remembers the menu bar setting", async () => {
      expect(await demoCall("get_menu_bar", undefined)).toBe(true);
      await demoCall("set_menu_bar", { enabled: false });
      expect(await demoCall("get_menu_bar", undefined)).toBe(false);
    });
  });
});

describe("withSince", () => {
  it("shrinks the counts to the period and scores them again", async () => {
    const { withSince } = await import("./api.demo");
    const { stats } = await import("./test/fixtures");
    const s = stats();
    const last = s.weekly.at(-1)?.week ?? "";
    const p = withSince(meta, s, last);
    expect(p.weekly).toHaveLength(1);
    expect(p.catches).toBeLessThan(s.catches);
    expect(p.overall).toEqual(judge(meta, { continued: p.briefs.window_tasks, landed: p.briefs.window_landed, catches: p.catches, humbled: p.humbled }));
    expect(withSince(meta, s, null)).toBe(s);
  });
});
