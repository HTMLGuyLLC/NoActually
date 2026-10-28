import { describe, expect, it } from "vitest";
import { briefs, HOSTILE, meta, stats } from "../test/fixtures";
import { briefsView } from "./briefs";
import { glanceView } from "./glance";
import { scorecardView } from "./scorecard";

const view = (over: Parameters<typeof briefs>[0] = {}, still = false) => briefsView(briefs(over), meta, still).value;

describe("briefsView", () => {
  it("answers with the tier's verdict, the window's landed share and the ladder", () => {
    const m = view();
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain(">Rarely.</div>");
    expect(m).toContain('<span class="sr-only">Rarely. Your brief tier is Architect.</span>');
    expect(m).toContain("<b>95%</b> of your tasks in the last 28 days landed");
    expect(m).toContain('<li class="on" aria-current="true"><span>Architect</span><span>90%+ landed</span></li>');
    expect(m).toContain("<span>Ticket writer</span><span>70–79%</span>");
    expect(m).toContain("Top tier.");
    expect(m).toContain('data-action="share-briefs"');
    expect(m).not.toContain("style=");
  });

  it("names the next tier's target and where you are", () => {
    const m = view({ tier: "tech_lead", next_tier: "architect", landed_last_28d: 34 });
    expect(m).toContain(">Sometimes.</div>");
    expect(m).toContain("Architect at 90%. You&#39;re at 85%.");
  });

  it("waits to rank a thin week, and offers nothing to share yet", () => {
    const m = view({ tier: null, next_tier: null, to_rank: 3, tasks_last_28d: 7 });
    expect(m).toContain('class="no maybe"');
    expect(m).toContain(">Hmm.</div>");
    expect(m).toContain("<b>3 more tasks</b> with a follow-up to get ranked");
    expect(m).not.toContain("share-briefs");
  });

  it("spells out the grade mix for screen readers, sharp first", () => {
    expect(view()).toContain('aria-label="Your briefs: 20% sharp, 30% solid, 50% thin"');
    expect(view()).toContain('class="g-sharp" data-w="20"');
  });

  it("lists every signal with its share and hint", () => {
    const m = view();
    expect(m).toContain('Points at it<small class="muted">a file, function, link or code</small>');
    expect(m).toContain('<span class="n">45%</span>'); // done: 90 of 200
  });

  it("tips the weakest signal, quoting the gap only when Rust found one", () => {
    expect(view()).toContain("<b>sets limits</b> (don&#39;t, only, keep, without). Only 10% of your briefs do.");
    expect(view()).not.toContain("landed <b>");
    const m = view({ tip: { signal: "why", pct_of_briefs: 20, landed_with_without: [93.3, 80] } });
    expect(m).toContain("Your briefs that do landed <b>93%</b> of the time, vs 80% without.");
    expect(view({ tip: null })).not.toContain("Try this");
    expect(view({ tip: { signal: "gone", pct_of_briefs: 1, landed_with_without: null } })).not.toContain("Try this");
  });

  it("splits steering between the AI and the brief", () => {
    const m = view();
    expect(m).toContain('class="cat ai"><span>It got it wrong</span>');
    expect(m).toContain("25 steering messages. 13 of them could have been in the brief.");
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
    const m = briefsView(briefs({ tier: HOSTILE }), { ...meta, steers: [{ id: "added", title: HOSTILE }] }, false).value;
    expect(m).not.toContain("<img");
  });
});

describe("briefs on the scorecard and the glance", () => {
  it("sits on the scorecard, with its own still flag", () => {
    const ctx = { meta, filter: {}, still: true, briefsStill: false };
    const m = scorecardView(stats(), ctx).value;
    expect(m).toContain("Do you make it guess?");
    expect(m).toContain('class="no" aria-hidden="true">Rarely.');
  });

  it("adds one line to the glance once ranked", () => {
    expect(glanceView(stats(), meta).value).toContain("Briefs: Architect · 95% landed");
    expect(glanceView(stats({ briefs: briefs({ tier: null }) }), meta).value).not.toContain("Briefs:");
  });
});
