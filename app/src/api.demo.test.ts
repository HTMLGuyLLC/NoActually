import { describe, expect, it, vi } from "vitest";
import shipped from "../public/demo.json";
import { demoCall, demoStats, isDemoData, judge } from "./api.demo";
import { hallEntry, meta, row, stats } from "./test/fixtures";

const base = stats({
  catches: 100,
  catches_last_7d: 20,
  messages: 400,
  models: [row("Anthropic", "Claude Opus", "5", 75, 300), row("OpenAI", "GPT Astra", "6", 25, 100)],
  categories: { just_wrong: 8, made_it_up: 1 },
  weekly: [{ week: "2026-10-19", messages: 40, catches: 12 }],
  hall_of_fame: [hallEntry({ id: 1, model: "Claude Opus 5" }), hallEntry({ id: 2, model: "GPT Astra 6" })],
});
const data = { meta, stats: base };

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

  it("recomputes the tier and verdict the way Rust would for the narrowed count", () => {
    const s = demoStats(data, { provider: "OpenAI" });
    expect(s).toMatchObject({ tier: "coworker", next_tier: ["sharp_eyed", 2], verdict: "no" });
  });

  it("matches on family and version too", () => {
    expect(demoStats(data, { family: "Claude Opus", version: "5" }).catches).toBe(75);
    expect(demoStats(data, { family: "Claude Opus", version: "9" }).catches).toBe(0);
  });
});

describe("judge", () => {
  it("follows the floors from meta", () => {
    expect(judge(meta, 0)).toEqual({ tier: "rubber_stamp", next_tier: ["coworker", 3], verdict: "hmm" });
    expect(judge(meta, 3)).toMatchObject({ tier: "coworker", verdict: "no" });
    expect(judge(meta, 15)).toEqual({ tier: "irreplaceable", next_tier: null, verdict: "no" });
  });
});

describe("isDemoData", () => {
  it("accepts the shipped demo.json", () => {
    expect(isDemoData(shipped)).toBe(true);
    if (!isDemoData(shipped)) return;
    // Every hall entry carries the display strings Rust now sends.
    for (const h of shipped.stats.hall_of_fame) expect(h.category_title && h.source_name).toBeTruthy();
    expect(shipped.meta.tiers.map((t) => t.id)).toContain(shipped.stats.tier);
  });

  it("rejects the old shape (bare stats, tier names, no verdict)", () => {
    expect(isDemoData(base)).toBe(false);
    expect(isDemoData({ meta, stats: { ...base, verdict: undefined } })).toBe(false);
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
});
