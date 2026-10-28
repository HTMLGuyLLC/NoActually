// "Do you make it guess?": how specific your briefs are and what it took to land them.
import type { Briefs, Meta } from "../api";
import { type Html, html, num, raw } from "../html";
import { briefTier, briefTierRanges, landedPct, percent, steerTitle } from "../meta";

const plural = (n: number, one: string, many = `${one}s`) => `${num(n)} ${n === 1 ? one : many}`;

/** "92% of your tasks this week landed…", or how many more tasks it takes to get ranked. */
function becauseLine(b: Briefs, meta: Meta): Html {
  return briefTier(meta, b.tier)
    ? html`<b>${landedPct(b)}%</b> of your tasks this week landed without you adding, clarifying or starting over.`
    : html`Give it <b>${plural(b.to_rank, "more task")}</b> this week to get ranked. Your first message in a chat is its brief.`;
}

/** The verdict word, hand-written, with a screen-reader version. */
function markView(b: Briefs, meta: Meta, still: boolean): Html {
  const tier = briefTier(meta, b.tier);
  return html`<div class="no${tier ? "" : " maybe"}${still ? " still" : ""}" aria-hidden="true">${tier?.verdict ?? "Hmm."}</div>
      <span class="sr-only">${tier ? `${tier.verdict} Your brief tier is ${tier.name}.` : "Not ranked yet."}</span>`;
}

/** The overview's half: the verdict, the tier, and a way to the details. */
export function briefsSummaryView(b: Briefs, meta: Meta, still: boolean): Html {
  const tier = briefTier(meta, b.tier);
  const body = b.tasks
    ? html`<div class="verdict">${markView(b, meta, still)}<div class="because">${becauseLine(b, meta)}</div></div>
        <p class="tier-line">${tier ? html`<span class="tier-label"><span>Tier</span>${tier.name}</span>` : ""}<span class="hand-note">${ladderNote(b, meta)}</span></p>`
    : html`<p class="muted">No tasks yet. The first message of each chat is its brief.</p>`;
  return html`<article class="card mini briefs">
      <div class="eyebrow">How you brief it</div>
      <h2 class="q">Do you make it guess?</h2>
      ${body}
      <div class="share">${tier ? html`<button data-action="share-briefs">Share your briefs</button>` : ""}<button class="ghost" data-action="view" data-id="briefs">See briefs</button></div>
    </article>`;
}

function verdictView(b: Briefs, meta: Meta, still: boolean, subject: Html | string): Html {
  return html`
    <div class="eyebrow">How you brief it</div>
    <h1 class="q">Do you make ${subject} guess?</h1>
    <div class="verdict">
      ${markView(b, meta, still)}
      <div class="because">${becauseLine(b, meta)}</div>
    </div>
    ${gradesView(b, meta)}`;
}

/** One bar split into sharp / solid / thin, with the numbers spelled out beside it. */
function gradesView(b: Briefs, meta: Meta): Html {
  const top = [...meta.grades].reverse();
  const parts = top.map((g) => ({ ...g, pct: percent(b.grades[g.id] ?? 0, b.tasks) }));
  const label = parts.map((p) => `${p.pct}% ${p.title.toLowerCase()}`).join(", ");
  return html`<div class="grades">
    <div class="grade-bar" role="img" aria-label="Your briefs: ${label}">${parts.map((p) => html`<i class="g-${p.id}" data-w="${p.pct}"></i>`)}</div>
    <ul class="grade-key">${parts.map((p) => html`<li><i class="g-${p.id}" aria-hidden="true"></i>${p.title} <b>${p.pct}%</b></li>`)}</ul>
    <p class="muted small">Sharp briefs carry ${meta.sharp_signals}+ of the ${meta.signals.length} things below. Questions aren't graded.</p>
  </div>`;
}

/** How far the next tier (or a ranking) is. */
function ladderNote(b: Briefs, meta: Meta): string {
  const next = b.next_tier && briefTier(meta, b.next_tier[0]);
  if (!b.tier) return `${plural(b.to_rank, "more task")} to get ranked`;
  return next && b.next_tier ? `${plural(b.next_tier[1], "more task")} landing in a row for ${next.name}` : "Top tier. Don't let it slip.";
}

function ladderView(b: Briefs, meta: Meta): Html {
  const note = ladderNote(b, meta);
  return html`
    <ol class="ladder" aria-label="Brief tiers">${briefTierRanges(meta).map(
      (t) => html`<li${t.id === b.tier ? raw(' class="on" aria-current="true"') : ""}><span>${t.name}</span><span>${t.range}</span></li>`,
    )}</ol>
    <p class="hand-note">${note}</p>
    ${b.tier ? html`<div class="share"><button data-action="share-briefs">Share your briefs</button></div>` : ""}`;
}

function signalsView(b: Briefs, meta: Meta): Html {
  const rows = meta.signals.map((s) => {
    const pct = percent(b.signals[s.id] ?? 0, b.tasks);
    return html`<div class="cat sig"><span>${s.title}<small class="muted">${s.hint}</small></span><span class="bar-bg"><i data-w="${Math.max(pct ? 2 : 0, pct)}"></i></span><span class="n">${pct}%</span></div>`;
  });
  return html`<div class="cats">${rows}</div>${tipView(b, meta)}`;
}

function tipView(b: Briefs, meta: Meta): Html | "" {
  const tip = b.tip;
  const s = tip && meta.signals.find((x) => x.id === tip.signal);
  if (!tip || !s) return "";
  const proof = tip.landed_with_without
    ? html` Your briefs that do landed <b>${Math.round(tip.landed_with_without[0])}%</b> of the time, vs ${Math.round(tip.landed_with_without[1])}% without.`
    : "";
  return html`<p class="tip"><span class="hand">Try this:</span> <b>${s.title.toLowerCase()}</b> (${s.hint}). Only ${Math.round(tip.pct_of_briefs)}% of your briefs do.${proof}</p>`;
}

function steeringView(b: Briefs, meta: Meta): Html {
  const counts = meta.steers.map((c) => ({ ...c, n: b.steering[c.id] ?? 0 }));
  const total = counts.reduce((t, c) => t + c.n, 0);
  if (!total) return html`<p class="muted">No steering yet. Every task landed as asked.</p>`;
  const top = Math.max(...counts.map((c) => c.n));
  const yours = counts.filter((c) => c.id !== "ai_wrong").reduce((t, c) => t + c.n, 0);
  return html`<div class="cats">${counts.map(
    (c) =>
      html`<div class="cat${c.id === "ai_wrong" ? " ai" : ""}"><span>${steerTitle(meta, c.id)}</span><span class="bar-bg"><i data-w="${c.n ? Math.max(2, (c.n / top) * 100) : 0}"></i></span><span class="n">${num(c.n)}</span></div>`,
  )}</div>
  <p class="muted small">${plural(total, "steering message")}. ${num(yours)} of them could have been in the brief.</p>`;
}

/** The Briefs section: verdict card, then what briefs carry and where steering went. `subject` is the
 * model picker ("it" when there's none). */
export function briefsView(b: Briefs, meta: Meta, still: boolean, subject: Html | string = "it"): Html {
  if (!b.tasks) {
    return html`<section class="card briefs"><div><div class="eyebrow">How you brief it</div><h1 class="q">Do you make ${subject} guess?</h1>
      <p class="muted">No tasks yet. The first message of each chat is its brief, so start a chat and ask for something.</p></div></section>`;
  }
  return html`
    <section class="card briefs">
      <div>${verdictView(b, meta, still, subject)}</div>
      <div class="meter">${ladderView(b, meta)}</div>
    </section>
    <section class="split">
      <div><div class="head"><h2>What your briefs carry</h2></div>${signalsView(b, meta)}</div>
      <div><div class="head"><h2>Where steering went</h2></div>${steeringView(b, meta)}</div>
    </section>`;
}
