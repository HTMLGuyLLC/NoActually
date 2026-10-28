import { describe, expect, it } from "vitest";
import { CARD_THEME, wrap } from "./cards";

/** A fake canvas where every character is 10px wide. */
const g = { measureText: (t: string) => ({ width: t.length * 10 }) as TextMetrics };

describe("wrap", () => {
  it("breaks text into lines no wider than the limit", () => {
    expect(wrap(g, "one two three four", 90, 3)).toEqual(["one two", "three", "four"]);
  });

  it("ends a cut-off last line with an ellipsis", () => {
    expect(wrap(g, "one two three four five six", 90, 2)).toEqual(["one two", "…"]);
    expect(wrap(g, "alpha beta gamma delta epsilon", 110, 2)).toEqual(["alpha beta", "gamma…"]);
  });

  it("leaves text that fits alone", () => {
    expect(wrap(g, "short", 100, 2)).toEqual(["short"]);
  });
});

describe("CARD_THEME", () => {
  it("is the light theme the cards always use", () => {
    expect(CARD_THEME).toMatchObject({ paper: "#fbfcfe", ink: "#141a2e", pen: "#e0322b" });
  });
});
