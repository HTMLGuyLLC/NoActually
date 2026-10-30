import { describe, expect, it } from "vitest";
import { HOSTILE, meta, prompts, stats } from "../test/fixtures";
import { glanceView } from "./glance";
import { promptsSummaryView, promptsView } from "./prompts";
import { scorecardView } from "./scorecard";

const view = (over: Parameters<typeof prompts>[0] = {}, still = false) => promptsView(prompts(over), meta, still).value;

describe("promptsView", () => {
  it("answers with the tier's verdict, the window's landed share and the ladder", () => {
    const m = view();
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain(">Rarely.</div>");
    expect(m).toContain('<span class="sr-only">Rarely. Your prompt tier is Architect.</span>');
    expect(m).toContain("<b>95%</b> of your prompts in the last 28 days were clear the first time.");
    // Rows say the answer; the tier's name and meaning sit under the answer.
    expect(m).toContain('<li class="on" aria-current="true"><span>Rarely</span><span><b class="at">95%</b>');
    expect(m).toContain("<span>Often</span><span>70–79%</span>");
    expect(m).toContain('<p class="tier-blurb"><b>Architect.</b> It rarely has to guess what you mean.</p>');
    expect(m).not.toContain("Top tier."); // the row carries the number; no note below
    expect(m).toContain('data-action="share-prompts"');
    expect(m).not.toContain("style=");
  });

  it("puts where you are in your tier's row, and the tiers say the rest", () => {
    const m = view({ tier: "tech_lead", next_tier: "architect", window_landed: 34 });
    expect(m).toContain(">Sometimes.</div>");
    expect(m).toContain('<span>Sometimes</span><span><b class="at">85%</b><span class="sr-only">, you\'re at 85%. Range</span> · 80–89%</span>');
    expect(m).not.toContain("Architect at 90%");
    // The overview card names the next tier by its answer, like the ladder rows.
    expect(promptsSummaryView(prompts({ tier: "tech_lead", next_tier: "architect", window_landed: 34 }), meta, true).value).toContain(
      "Next: Rarely, at 90%. You&#39;re at 85%.",
    );
  });

  it("waits to rank a thin week, and offers nothing to share yet", () => {
    const m = view({ tier: null, next_tier: null, to_rank: 3, window_tasks: 7 });
    expect(m).toContain('class="no maybe"');
    expect(m).toContain(">Hmm.</div>");
    expect(m).toContain("<b>3 more tasks</b> with a follow-up to get ranked");
    expect(m).not.toContain("share-prompts");
  });

  it("spells out the grade mix for screen readers, sharp first", () => {
    expect(view()).toContain('aria-label="Your prompts: 20% specific, 30% decent, 50% vague"');
    expect(view()).toContain('class="g-sharp" data-w="20"');
  });

  it("lists every signal with its share and hint", () => {
    const m = view();
    expect(m).toContain('Target<small class="muted">a file, function, page or link</small>');
    expect(m).toContain('<span class="n">45%</span>'); // done: 90 of 200
  });

  it("tips the weakest signal, quoting the gap only when Rust found one", () => {
    expect(view()).toContain("include <b>guardrails</b> (don&#39;t, only, keep, without). Only 10% of your prompts do.");
    expect(view()).not.toContain("were clear the first time <b>");
    const m = view({ tip: { signal: "why", pct_of_prompts: 20, landed_with_without: [93.3, 80] } });
    expect(m).toContain("Your prompts with a purpose were clear the first time <b>93%</b> of the time, vs 80% without.");
    expect(view({ tip: null })).not.toContain("Try this");
    expect(view({ tip: { signal: "gone", pct_of_prompts: 1, landed_with_without: null } })).not.toContain("Try this");
  });

  it("splits steering between the AI and the prompt", () => {
    const m = view();
    expect(m).toContain('class="cat ai"><span>It got it wrong</span>');
    expect(m).toContain("25 follow-ups. 12 were its mistakes (blue), which don't count against you; 13 could have been in the prompt (red).");
    expect(view({ steering: { ai_wrong: 0, added: 0, clarified: 0, redo: 0 } })).toContain("No follow-ups yet.");
    expect(view({ steering: { ai_wrong: 1, added: 0, clarified: 0, redo: 0 } })).toContain(
      "1 follow-up. 1 was its mistake (blue), which doesn't count against you; 0 could have been in the prompt (red).",
    );
  });

  it("explains itself before the first task", () => {
    const m = view({ tasks: 0 });
    expect(m).toContain("No tasks yet.");
    expect(m).not.toContain("ladder");
  });

  it("keeps the scrawl still on a redraw", () => {
    expect(view({}, true)).toContain('class="no still"');
  });

  it("escapes whatever arrives", () => {
    const m = promptsView(prompts({ tier: HOSTILE }), { ...meta, steers: [{ id: "added", title: HOSTILE }] }, false).value;
    expect(m).not.toContain("<img");
  });
});

describe("prompts on the scorecard and the glance", () => {
  it("sits on the scorecard, with its own still flag", () => {
    const ctx = { meta, filter: {}, still: true, promptsStill: false };
    const m = scorecardView(stats(), ctx).value;
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain('class="no" aria-hidden="true">Rarely.');
  });

  it("names the prompt tier in the glance once ranked", () => {
    expect(glanceView(stats(), meta).value).toContain('<span class="g-word">Rarely.</span><span class="muted small">95% clear · Architect</span>');
    expect(glanceView(stats({ prompts: prompts({ tier: null, to_rank: 3 }) }), meta).value).toContain("3 more tasks to get ranked");
  });

  it("charts prompts per week as clear vs needed more, adding up to each week's tasks", () => {
    const weekly = stats().weekly;
    const m = promptsView(prompts(), meta, true, "AI", "28d", weekly).value;
    expect(m).toContain("<h2>Prompts per week</h2>");
    expect(m).toContain("Clear the first time");
    expect(m).toContain("Needed more from you");
    // One tip per week's bar: its tasks, what landed, and what the rest needed.
    expect(m).toContain(
      'class="bar-group" data-tip="Week of Oct 19\n20 prompts · 17 clear the first time (85%)\n\nNeeded more from you:\nYou added more 2\nYou clarified 1\n\nIt got it wrong 4 (not on you)"',
    );
    expect(m).toContain("17 of 20 prompts were clear the first time.");
    expect(promptsView(prompts(), meta, true, "AI", "28d", []).value).toContain("No tasks with a follow-up yet.");
  });
});
