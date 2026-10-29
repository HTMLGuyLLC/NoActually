import { describe, expect, it } from "vitest";
import { HOSTILE, meta, prompts, stats } from "../test/fixtures";
import { glanceView } from "./glance";
import { promptsView } from "./prompts";
import { scorecardView } from "./scorecard";

const view = (over: Parameters<typeof prompts>[0] = {}, still = false) => promptsView(prompts(over), meta, still).value;

describe("promptsView", () => {
  it("answers with the tier's verdict, the window's landed share and the ladder", () => {
    const m = view();
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain(">Rarely.</div>");
    expect(m).toContain('<span class="sr-only">Rarely. Your prompt tier is Architect.</span>');
    expect(m).toContain("<b>95%</b> of your tasks in the last 28 days landed");
    expect(m).toContain('<li class="on" aria-current="true"><span>Architect</span><span><b class="at">95%</b>');
    expect(m).toContain("<span>Ticket writer</span><span>70–79%</span>");
    expect(m).not.toContain("Top tier."); // the row carries the number; no note below
    expect(m).toContain('data-action="share-prompts"');
    expect(m).not.toContain("style=");
  });

  it("puts where you are in your tier's row, and the tiers say the rest", () => {
    const m = view({ tier: "tech_lead", next_tier: "architect", window_landed: 34 });
    expect(m).toContain(">Sometimes.</div>");
    expect(m).toContain('<span>Tech lead</span><span><b class="at">85%</b><span class="sr-only">, you\'re at 85%. Range</span> · 80–89%</span>');
    expect(m).not.toContain("Architect at 90%");
  });

  it("waits to rank a thin week, and offers nothing to share yet", () => {
    const m = view({ tier: null, next_tier: null, to_rank: 3, window_tasks: 7 });
    expect(m).toContain('class="no maybe"');
    expect(m).toContain(">Hmm.</div>");
    expect(m).toContain("<b>3 more tasks</b> with a follow-up to get ranked");
    expect(m).not.toContain("share-prompts");
  });

  it("spells out the grade mix for screen readers, sharp first", () => {
    expect(view()).toContain('aria-label="Your prompts: 20% sharp, 30% solid, 50% thin"');
    expect(view()).toContain('class="g-sharp" data-w="20"');
  });

  it("lists every signal with its share and hint", () => {
    const m = view();
    expect(m).toContain('Points at it<small class="muted">a file, function, link or code</small>');
    expect(m).toContain('<span class="n">45%</span>'); // done: 90 of 200
  });

  it("tips the weakest signal, quoting the gap only when Rust found one", () => {
    expect(view()).toContain("<b>sets limits</b> (don&#39;t, only, keep, without). Only 10% of your prompts do.");
    expect(view()).not.toContain("landed <b>");
    const m = view({ tip: { signal: "why", pct_of_prompts: 20, landed_with_without: [93.3, 80] } });
    expect(m).toContain("Your prompts that do landed <b>93%</b> of the time, vs 80% without.");
    expect(view({ tip: null })).not.toContain("Try this");
    expect(view({ tip: { signal: "gone", pct_of_prompts: 1, landed_with_without: null } })).not.toContain("Try this");
  });

  it("splits steering between the AI and the prompt", () => {
    const m = view();
    expect(m).toContain('class="cat ai"><span>It got it wrong</span>');
    expect(m).toContain("25 steering messages. 12 were its mistakes (blue), which don't count against you; 13 could have been in the prompt (red).");
    expect(view({ steering: { ai_wrong: 0, added: 0, clarified: 0, redo: 0 } })).toContain("No steering yet.");
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
    expect(glanceView(stats(), meta).value).toContain('<span class="g-word">Rarely.</span><span class="muted small">95% landed · Architect</span>');
    expect(glanceView(stats({ prompts: prompts({ tier: null, to_rank: 3 }) }), meta).value).toContain("3 more tasks to get ranked");
  });
});
