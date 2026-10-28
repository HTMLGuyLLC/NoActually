import { describe, expect, it } from "vitest";
import { categoryTitle, sourceName, tierName, tierRanges, topTier } from "./meta";
import { meta } from "./test/fixtures";

describe("tierRanges", () => {
  it("derives the ladder's ranges from the floors, top tier first", () => {
    expect(tierRanges(meta).map((t) => [t.name, t.range])).toEqual([
      ["Irreplaceable", "15+ catches/wk"],
      ["Sharp-eyed", "7–14"],
      ["Coworker", "3–6"],
      ["Rubber stamp", "0–2"],
    ]);
  });

  it("doesn't depend on the order tiers arrive in, and shows a one-number range plainly", () => {
    const tiers = [
      { id: "b", name: "B", floor: 1 },
      { id: "a", name: "A", floor: 0 },
    ];
    expect(tierRanges({ ...meta, tiers }).map((t) => t.range)).toEqual(["1+ catches/wk", "0"]);
  });
});

describe("lookups", () => {
  it("finds display names by id", () => {
    expect(tierName(meta, "sharp_eyed")).toBe("Sharp-eyed");
    expect(categoryTitle(meta, "just_wrong")).toBe("Flat-out wrong");
    expect(sourceName(meta, "claude-code")).toBe("Claude Code");
  });

  it("falls back to the id for anything unknown (a newer Rust side)", () => {
    expect(tierName(meta, "legend")).toBe("legend");
    expect(categoryTitle(meta, "new_kind")).toBe("new_kind");
    expect(sourceName(meta, "new-tool")).toBe("new-tool");
  });

  it("finds the top tier, or none", () => {
    expect(topTier(meta)?.floor).toBe(15);
    expect(topTier({ ...meta, tiers: [] })).toBeUndefined();
  });
});
