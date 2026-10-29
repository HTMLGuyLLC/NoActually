import { describe, expect, it } from "vitest";
import {
  categoryTitle,
  floorPercent,
  gradeTitle,
  hasSignal,
  landedPct,
  percent,
  promptTier,
  promptTierRanges,
  signalTitle,
  sourceName,
  steerTitle,
  tierName,
  tierOf,
  tierRanges,
} from "./meta";
import { meta } from "./test/fixtures";

describe("promptTierRanges", () => {
  it("shows percent ranges, top tier first", () => {
    expect(promptTierRanges(meta).map((t) => [t.name, t.range])).toEqual([
      ["Architect", "90%+ clear"],
      ["Tech lead", "80–89%"],
      ["Ticket writer", "70–79%"],
      ["Vibes only", "0–69%"],
    ]);
  });
});

describe("percents", () => {
  it("rounds, and never divides by zero", () => {
    expect([percent(1, 3), percent(2, 3), percent(5, 0)]).toEqual([33, 67, 0]);
    expect(landedPct({ window_landed: 19, window_tasks: 20 })).toBe(95);
    expect(landedPct({ window_landed: 26, window_tasks: 29 })).toBe(89); // 89.7%: never shows 90% for a Tech lead
  });
});

describe("tierRanges", () => {
  it("derives the ladder's ranges from the floors, top tier first", () => {
    expect(tierRanges(meta).map((t) => [t.name, t.range])).toEqual([
      ["Irreplaceable", "90+"],
      ["Sharp-eyed", "80–89"],
      ["Coworker", "65–79"],
      ["Rubber stamp", "0–64"],
    ]);
  });

  it("doesn't depend on the order tiers arrive in, and shows a one-number range plainly", () => {
    const tiers = [
      { id: "b", name: "B", floor: 1 },
      { id: "a", name: "A", floor: 0 },
    ];
    expect(tierRanges({ ...meta, tiers }).map((t) => t.range)).toEqual(["1+", "0"]);
  });
});

describe("lookups", () => {
  it("finds display names by id", () => {
    expect(tierName(meta, "sharp_eyed")).toBe("Sharp-eyed");
    expect(categoryTitle(meta, "just_wrong")).toBe("Flat-out wrong");
    expect(sourceName(meta, "claude-code")).toBe("Claude Code");
    expect(promptTier(meta, "tech_lead")?.verdict).toBe("Sometimes.");
    expect(promptTier(meta, null)).toBeUndefined();
    expect(steerTitle(meta, "redo")).toBe("You started over");
    expect(steerTitle(meta, "unknown")).toBe("unknown");
  });

  it("falls back to the id for anything unknown (a newer Rust side)", () => {
    expect(tierName(meta, "legend")).toBe("legend");
    expect(categoryTitle(meta, "new_kind")).toBe("new_kind");
    expect(sourceName(meta, "new-tool")).toBe("new-tool");
  });

  it("says a missing tier is not scored yet, not a made-up name", () => {
    expect(tierName(meta, null)).toBe("Not scored yet");
  });

  it("finds a tier by id, and a signal's title", () => {
    expect(tierOf(meta, "coworker")?.floor).toBe(65);
    expect(tierOf(meta, null)).toBeUndefined();
    expect(tierOf(meta, "legend")).toBeUndefined();
    expect(signalTitle(meta, "why")).toBe("Purpose");
    expect(signalTitle(meta, "unknown")).toBe("unknown");
  });
});

describe("floorPercent", () => {
  it("rounds down where percent rounds", () => {
    expect(percent(897, 1000)).toBe(90);
    expect(floorPercent(897, 1000)).toBe(89);
    expect(floorPercent(1, 0)).toBe(0);
  });
});

describe("gradeTitle and hasSignal", () => {
  it("names a grade, falling back to its id", () => {
    expect(gradeTitle(meta, "sharp")).toBe(meta.grades.find((g) => g.id === "sharp")?.title);
    expect(gradeTitle(meta, "nope")).toBe("nope");
  });

  it("reads signal bit i", () => {
    expect(hasSignal(0b101, 0)).toBe(true);
    expect(hasSignal(0b101, 1)).toBe(false);
    expect(hasSignal(0b101, 2)).toBe(true);
  });
});
