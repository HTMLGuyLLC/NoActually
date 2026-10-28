// "Do you make it guess?": how specific your briefs are and what it took to land them.
import type { Briefs, Meta } from "../api";
import { type Html, html, num, raw } from "../html";
import { briefTier, briefTierRanges, landedPct, percent, steerTitle } from "../meta";

const plural = (n: number, one: string, many = `${one}s`) => `${num(n)} ${n === 1 ? one : many}`;

function verdictView(b: Briefs, meta: Meta, still: boolean): Html {
  const tier = briefTier(meta, b.tier);
  const because = tier
    ? html`<b>${landedPct(b)}%</b> of your tasks this week landed without you adding, clarifying or starting over.`
    : html`Give it <b>${plural(b.to_rank, "more task")}</b> this week to get ranked. Your first message in a chat is its brief.`;
  return html`
    <div class="eyebrow">How you brief it</div>
    <h2 class="q">Do you make it guess?</h2>
    <div class="verdict">
      <div class="no${tier ? "" : " maybe"}${still ? " still" : ""}" aria-hidden="true">${tier?.verdict ?? "Hmm."}</div>
      <span class="sr-only">${tier ? `${tier.verdict} Your brief tier is ${tier.name}.` : "Not ranked yet."}</span>
      <div class="because">${because}</div>
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

function ladderView(b: Briefs, meta: Meta): Html {
  const next = b.next_tier && briefTier(meta, b.next_tier[0]);
  const note = !b.tier
    ? `${plural(b.to_rank, "more task")} to get ranked`
    : next && b.next_tier
      ? `${plural(b.next_tier[1], "more task")} landing in a row for ${next.name}`
      : "Top tier. Don't let it slip.";
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

/** The whole briefs block: verdict card, then what briefs carry and where steering went. */
export function briefsView(b: Briefs, meta: Meta, still: boolean): Html {
  if (!b.tasks) {
    return html`<section class="card briefs"><div><div class="eyebrow">How you brief it</div><h2 class="q">Do you make it guess?</h2>
      <p class="muted">No tasks yet. The first message of each chat is its brief, so start a chat and ask for something.</p></div></section>`;
  }
  return html`
    <section class="card briefs">
      <div>${verdictView(b, meta, still)}</div>
      <div class="meter">${ladderView(b, meta)}</div>
    </section>
    <section class="split">
      <div><div class="head"><h2>What your briefs carry</h2></div>${signalsView(b, meta)}</div>
      <div><div class="head"><h2>Where steering went</h2></div>${steeringView(b, meta)}</div>
    </section>`;
}
