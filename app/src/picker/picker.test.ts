import { describe, expect, it } from "vitest";
import { row } from "../test/fixtures";
import { buildPickItems, catchRate, matchPickItems, type PickItem, sameFilter } from "./picker";

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

  it("starts with everything, then providers by catches", () => {
    expect(labels[0]).toBe("All AI");
    expect(items[0]?.catches).toBe(136);
    expect(labels.indexOf("Anthropic")).toBeLessThan(labels.indexOf("OpenAI"));
  });

  it("nests versions under families that have several", () => {
    const opus = byLabel(items, "Claude Opus");
    expect(opus.catches).toBe(99);
    expect(opus.filter).toEqual({ provider: "Anthropic", family: "Claude Opus" });
    const v55 = byLabel(items, "Claude Opus 5.5");
    expect(v55.depth).toBe(2);
    expect(v55.filter).toEqual({ provider: "Anthropic", family: "Claude Opus", version: "5.5" });
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

describe("sameFilter / catchRate", () => {
  it("compares scopes by value", () => {
    expect(sameFilter({ provider: "OpenAI" }, { provider: "OpenAI" })).toBe(true);
    expect(sameFilter({}, { provider: "OpenAI" })).toBe(false);
  });

  it("formats a rate, or a dash when nothing was said", () => {
    expect(catchRate(1, 8)).toBe("12.5%");
    expect(catchRate(0, 0)).toBe("–");
  });
});
