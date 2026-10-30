import { describe, expect, it } from "vitest";
import { row } from "../test/fixtures";
import { buildPickItems, correctionRate, landedRate, matchPickItems, type PickItem, sameFilter } from "./picker";

const models = [
  row("Anthropic", "Claude Opus", "5", 60),
  row("Anthropic", "Claude Opus", "5.5", 39),
  row("Anthropic", "Claude Haiku", "4.5", 2),
  row("OpenAI", "GPT Astra", "6", 34),
  row("No model recorded", "No model recorded", "", 1),
];

const byLabel = (items: PickItem[], label: string): PickItem => {
  const item = items.find((i) => i.label === label);
  if (!item) throw new Error(`no item ${label}`);
  return item;
};

describe("buildPickItems", () => {
  const items = buildPickItems(models);
  const labels = items.map((i) => i.label);

  it("starts with everything, then providers by corrections", () => {
    expect(labels[0]).toBe("All AI");
    expect(items[0]?.corrections).toBe(136);
    expect(labels.indexOf("Anthropic")).toBeLessThan(labels.indexOf("OpenAI"));
  });

  it("nests versions under families that have several", () => {
    const opus = byLabel(items, "Claude Opus");
    expect(opus.corrections).toBe(99);
    expect(opus.filter).toEqual({ provider: "Anthropic", family: "Claude Opus" });
    const v55 = byLabel(items, "Claude Opus 5.5");
    expect(v55.depth).toBe(2);
    expect(v55.filter).toEqual({ provider: "Anthropic", family: "Claude Opus", version: "5.5" });
  });

  it("carries summed counts for corrections, messages and the prompt score", () => {
    const all = byLabel(items, "All AI");
    expect(all).toMatchObject({ corrections: 136, messages: 500, tasks: 100, continued: 50, landed: 40 });
    const opus = byLabel(items, "Claude Opus");
    expect(opus).toMatchObject({ corrections: 99, messages: 200, tasks: 40, continued: 20, landed: 16 });
    expect(byLabel(items, "Claude Opus 5.5")).toMatchObject({ corrections: 39, messages: 100, tasks: 20, continued: 10, landed: 8 });
  });

  it("sums the tasks of a model that has its own mix", () => {
    const mixed = buildPickItems([row("A", "F", "1", 1, 10, 30), row("A", "F", "2", 1, 10, 10)]);
    expect(byLabel(mixed, "A")).toMatchObject({ tasks: 40, continued: 20, landed: 16 });
  });

  it("lists a provider that is its own only family once", () => {
    expect(labels.filter((l) => l === "No model recorded")).toHaveLength(1);
  });

  it("collapses a family with a single version into that model", () => {
    expect(labels).not.toContain("Claude Haiku");
    expect(byLabel(items, "Claude Haiku 4.5").depth).toBe(1);
  });

  it("has just the 'All AI' scope when there are no models", () => {
    expect(buildPickItems([]).map((i) => i.label)).toEqual(["All AI"]);
  });
});

describe("matchPickItems", () => {
  const items = buildPickItems(models);
  const find = (q: string) => matchPickItems(items, q).map((i) => i.label);

  it("matches every word, in any order, across label and path", () => {
    expect(find("opus 5.5")).toEqual(["Claude Opus 5.5"]);
    expect(find("5.5 OPUS")).toEqual(["Claude Opus 5.5"]);
    expect(find("anthropic haiku")).toEqual(["Claude Haiku 4.5"]);
    expect(find("gpt")).toEqual(["GPT Astra 6"]);
  });

  it("returns everything for an empty query and nothing for nonsense", () => {
    expect(matchPickItems(items, "  ")).toHaveLength(items.length);
    expect(find("zzz")).toEqual([]);
  });
});

describe("sameFilter / correctionRate", () => {
  it("compares scopes by value", () => {
    expect(sameFilter({ provider: "OpenAI" }, { provider: "OpenAI" })).toBe(true);
    expect(sameFilter({}, { provider: "OpenAI" })).toBe(false);
  });

  it("formats a rate, or a dash when nothing was said", () => {
    expect(correctionRate(1, 8)).toBe("12.5%");
    expect(correctionRate(0, 0)).toBe("–");
  });

  it("rounds the landed rate down, like the tier floors, and shows a dash with no follow-ups", () => {
    expect(landedRate(5, 6)).toBe("83%"); // 83.3
    expect(landedRate(29, 30)).toBe("96%"); // 96.7 never reads as 97
    expect(landedRate(9, 10)).toBe("90%");
    expect(landedRate(0, 5)).toBe("0%");
    expect(landedRate(0, 0)).toBe("–");
  });
});
