import { describe, expect, it } from "vitest";
import type { Goal, GoalStatus } from "../api";
import { HOSTILE, meta, stats } from "../test/fixtures";
import { emptyGlanceView, glanceView } from "./glance";

const status = (goal: Partial<Goal>, over: Partial<GoalStatus> = {}): GoalStatus => ({
  goal: { kind: "habit", signal: "why", id: 1, target: 40, start: "2026-10-05", ...goal } as Goal,
  weeks: [{ week: "2026-10-26", count: 12, value: 30, state: "open" }],
  met: 1,
  streak: 0,
  needed: 4,
  done: false,
  need: 2,
  score: null,
  ...over,
});

describe("glanceView", () => {
  it("opens the app or its settings, and has nothing to share or copy", () => {
    const m = glanceView(stats(), meta).value;
    expect(m).toContain('data-action="open-main" data-id="settings" aria-label="Settings"');
    expect(m).toContain('<button data-action="open-main">Open app</button>');
    expect(m).not.toContain("copy-share");
    expect(m).not.toContain("Share");
  });

  it("asks the scorecard's three questions, each answered in a word with why", () => {
    const m = glanceView(stats(), meta).value;
    expect(m).toContain("Could AI replace you?");
    expect(m).toContain('<span class="g-word">No.</span><span class="muted small">Score 86 · Sharp-eyed</span>');
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain("Do you correct it?");
    expect(m).toContain('<span class="g-word">Religiously.</span><span class="muted small">41 corrections · Hawk-eyed</span>');
  });

  it("says to add a newer export, like the scorecard, when only an export is imported", () => {
    const o = stats({ overall: { ...stats().overall, score: null, tier: null, to_rank: 7 } });
    expect(glanceView(o, meta).value).toContain("7 more tasks to get a score");
    const m = glanceView(o, meta, null, true).value;
    expect(m).toContain("Add a newer export to get a score");
    expect(m).not.toContain("7 more tasks");
  });

  it("shows a goal only when there is one, with this week's number", () => {
    expect(glanceView(stats(), meta).value).not.toContain("Your goal");
    const m = glanceView(stats(), meta, status({})).value;
    expect(m).toContain("Your goal");
    expect(m).toContain("30% this week · aim for at least 40%");
    expect(glanceView(stats(), meta, status({}, { done: true })).value).toContain("Done 🏆");
    expect(glanceView(stats(), meta, status({}, { weeks: [] })).value).toContain("1 of 4 weeks met");
    expect(glanceView(stats(), meta, status({ kind: "tier", tier: "irreplaceable" } as Partial<Goal>, { score: 86 })).value).toContain("Score 86 of 90");
  });

  it("keeps the gear and Open app before the first correction", () => {
    for (const m of [emptyGlanceView("").value, emptyGlanceView("Codex").value]) {
      expect(m).toContain('data-id="settings"');
      expect(m).toContain('data-action="open-main"');
    }
    expect(emptyGlanceView(HOSTILE).value).not.toContain("<img");
    for (const m of [emptyGlanceView("").value, emptyGlanceView("Codex").value, glanceView(stats(), meta).value])
      expect(m).toContain('<h1 class="sr-only">Quick glance</h1>');
    expect(emptyGlanceView("Codex").value).toContain("Your score shows up after 10 tasks with more than one message.");
    expect(emptyGlanceView("Codex", 12).value).toContain("after 12 tasks with more than one message.");
  });
});
