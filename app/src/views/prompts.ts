// "Do you make it guess?": how specific your prompts are and what it took to land them.
import type { Meta, Prompts, WeekRow } from "../api";
import { fmtDay, plural } from "../format";
import { type Html, html, num } from "../html";
import { landedPct, percent, promptTier, promptTierRanges, steerTitle } from "../meta";
import { PERIOD_WHEN, type Period } from "../state";
import { stackChart } from "./charts";
import { answerWord, byAnswer, ladderView, shareButton, tierBlurb, verdictMark } from "./chrome";

/** "86% of your prompts in the last 28 days were clear the first time…", or how many more it takes to get ranked. */
function becauseLine(b: Prompts, meta: Meta, when: string): Html {
  return promptTier(meta, b.tier)
    ? html`<b>${landedPct(b)}%</b> of your prompts ${when} were clear the first time.`
    : html`Give it <b>${plural(b.to_rank, "more task")}</b> with a follow-up to get ranked. A one-shot prompt can't need a follow-up, so it doesn't count.`;
}

/** The verdict word, hand-written, with a screen-reader version. */
function markView(b: Prompts, meta: Meta, still: boolean): Html {
  const tier = promptTier(meta, b.tier);
  return verdictMark(tier?.verdict ?? null, tier ? `${tier.verdict} Your prompt tier is ${tier.name}.` : "Not ranked yet.", still);
}

/** The overview's half: the verdict, the tier, and a way to the details. */
export function promptsSummaryView(b: Prompts, meta: Meta, still: boolean, when = PERIOD_WHEN["28d"]): Html {
  const tier = promptTier(meta, b.tier);
  const body = b.tasks
    ? html`<div class="verdict">${markView(b, meta, still)}<div class="because">${becauseLine(b, meta, when)}</div></div>
        ${tierBlurb(tier)}<p class="hand-note">${ladderNote(b, meta)}</p>`
    : html`<p class="muted">No tasks yet. The first message of each chat is its prompt.</p>`;
  return html`<article class="card mini prompts">
      <h2 class="q">Do you make it guess?</h2>
      ${body}
      <div class="share">${tier ? shareButton("share-prompts") : ""}<button class="ghost" data-action="view" data-id="prompts">See prompts</button></div>
    </article>`;
}

function verdictView(b: Prompts, meta: Meta, still: boolean, subject: string, period: Period): Html {
  const when = PERIOD_WHEN[period];
  return html`
    <h1 class="q">Do you make ${subject} guess?</h1>
    <div class="verdict">
      ${markView(b, meta, still)}
      <div class="because">${becauseLine(b, meta, when)}</div>
    </div>
    ${tierBlurb(promptTier(meta, b.tier))}`;
}

/** One bar split into sharp / solid / thin, with the numbers spelled out beside it. */
function gradesView(b: Prompts, meta: Meta): Html {
  const top = [...meta.grades].reverse();
  const parts = top.map((g) => ({ ...g, pct: percent(b.grades[g.id] ?? 0, b.tasks) }));
  const label = parts.map((p) => `${p.pct}% ${p.title.toLowerCase()}`).join(", ");
  return html`<div class="grades">
    <div class="grade-bar" role="img" aria-label="Your prompts: ${label}">${parts.map((p) => html`<i class="g-${p.id}" data-w="${p.pct}"></i>`)}</div>
    <ul class="grade-key">${parts.map((p) => html`<li><i class="g-${p.id}" aria-hidden="true"></i>${p.title} <b>${p.pct}%</b></li>`)}</ul>
    <p class="muted small">Sharp prompts include ${meta.sharp_signals}+ of the ${meta.signals.length} things below. Questions aren't graded.</p>
  </div>`;
}

/** The next tier's target ("Architect at 90%. You're at 86%."), or how far a ranking is. */
function ladderNote(b: Prompts, meta: Meta): string {
  if (!b.tier) return `${plural(b.to_rank, "more task")} to get ranked`;
  const next = promptTier(meta, b.next_tier);
  return next ? `Next: ${answerWord(next)}, at ${next.floor}%. You're at ${landedPct(b)}%.` : "Top tier. Don't let it slip.";
}

function promptLadderView(b: Prompts, meta: Meta): Html {
  return html`
    ${ladderView({ label: "Prompt tiers", tiers: byAnswer(promptTierRanges(meta)), on: b.tier, at: b.tier ? `${landedPct(b)}%` : null, unranked: ladderNote(b, meta) })}
    ${b.tier ? html`<div class="share">${shareButton("share-prompts", "Share your score")}</div>` : ""}`;
}

function signalsView(b: Prompts, meta: Meta): Html {
  const rows = meta.signals.map((s) => {
    const pct = percent(b.signals[s.id] ?? 0, b.tasks);
    return html`<div class="cat sig"><span>${s.title}<small class="muted">${s.hint}</small></span><span class="bar-bg"><i data-w="${Math.max(pct ? 2 : 0, pct)}"></i></span><span class="n">${pct}%</span></div>`;
  });
  return html`<div class="cats">${rows}</div>${tipView(b, meta)}`;
}

function tipView(b: Prompts, meta: Meta): Html | "" {
  const tip = b.tip;
  const s = tip && meta.signals.find((x) => x.id === tip.signal);
  if (!tip || !s) return "";
  const proof = tip.landed_with_without
    ? html` Your prompts that do were clear the first time <b>${Math.round(tip.landed_with_without[0])}%</b> of the time, vs ${Math.round(tip.landed_with_without[1])}% without.`
    : "";
  return html`<p class="tip"><span class="hand">Try this:</span> <b>${s.title.toLowerCase()}</b> (${s.hint}). Only ${Math.round(tip.pct_of_prompts)}% of your prompts do.${proof}</p>`;
}

function steeringView(b: Prompts, meta: Meta): Html {
  const counts = meta.steers.map((c) => ({ ...c, n: b.steering[c.id] ?? 0 }));
  const total = counts.reduce((t, c) => t + c.n, 0);
  if (!total) return html`<p class="muted">No follow-ups yet. Every prompt was clear the first time.</p>`;
  const top = Math.max(...counts.map((c) => c.n));
  const yours = counts.filter((c) => c.id !== "ai_wrong").reduce((t, c) => t + c.n, 0);
  return html`<div class="cats">${counts.map(
    (c) =>
      html`<div class="cat${c.id === "ai_wrong" ? " ai" : ""}"><span>${steerTitle(meta, c.id)}</span><span class="bar-bg"><i data-w="${c.n ? Math.max(2, (c.n / top) * 100) : 0}"></i></span><span class="n">${num(c.n)}</span></div>`,
  )}</div>
  <p class="muted small">${plural(total, "follow-up")}. ${num(total - yours)} were its mistakes (blue), which don't count against you; ${num(yours)} could have been in the prompt (red).</p>`;
}

/** The Prompts section: verdict card, then what prompts carry and where steering went. `subject` is who
 * the question asks about ("it" by default). */
/** Prompts per week, split into the ones that were clear the first time and the ones that needed more from you: the
 * same tasks the prompt tier counts, so the bars add up to the number at the top. */
/** A week's tip: its tasks, how many were clear the first time, and what the rest needed. */
function weekTip(w: WeekRow, meta: Meta): string {
  const lines = [`Week of ${fmtDay(w.week)}`, `${plural(w.continued, "prompt")} · ${w.landed} clear the first time (${percent(w.landed, w.continued)}%)`];
  const onYou = meta.steers.filter((s) => s.id !== "ai_wrong" && (w.steering[s.id] ?? 0) > 0);
  if (onYou.length) lines.push("", "Needed more from you:", ...onYou.map((s) => `${s.title} ${w.steering[s.id]}`));
  const wrong = w.steering.ai_wrong ?? 0;
  if (wrong) lines.push("", `${steerTitle(meta, "ai_wrong")} ${wrong} (not on you)`);
  return lines.join("\n");
}

function weeksView(weekly: WeekRow[], meta: Meta): Html {
  const weeks = weekly.slice(-20);
  if (!weeks.some((w) => w.continued)) return html`<p class="muted">No tasks with a follow-up yet.</p>`;
  const recent = weeks.filter((w) => w.continued);
  const last = recent[recent.length - 1];
  const summary = last
    ? `${plural(weeks.length, "week")} from ${fmtDay(weeks[0]?.week ?? last.week)}. The latest, the week of ${fmtDay(last.week)}: ${last.landed} of ${plural(last.continued, "prompt")} were clear the first time.`
    : "";
  return stackChart({
    id: "prompt-weeks",
    label: "Prompts per week",
    summary,
    labels: weeks.map((w) => fmtDay(w.week)),
    tips: weeks.map((w) => weekTip(w, meta)),
    series: [
      { title: "Clear the first time", values: weeks.map((w) => w.landed) },
      { title: "Needed more from you", values: weeks.map((w) => w.continued - w.landed) },
    ],
  });
}

export function promptsView(b: Prompts, meta: Meta, still: boolean, subject = "it", period: Period = "28d", weekly: WeekRow[] = []): Html {
  if (!b.tasks) {
    return html`<section class="card prompts"><div><h1 class="q">Do you make ${subject} guess?</h1>
      <p class="muted">No tasks yet. The first message of each chat is its prompt, so start a chat and ask for something.</p></div></section>`;
  }
  return html`
    <section class="card prompts">
      <div>${verdictView(b, meta, still, subject, period)}</div>
      <div class="meter">${promptLadderView(b, meta)}</div>
    </section>
    <section><div class="head"><h2>Prompts per week</h2><p class="muted">Prompts with a follow-up. A one-shot prompt can't need more from you, so it doesn't count.</p></div><div class="chart">${weeksView(weekly, meta)}</div></section>
    <section class="split">
      <div><div class="head"><h2>What your prompts include</h2></div>${gradesView(b, meta)}${signalsView(b, meta)}</div>
      <div><div class="head"><h2>Why you followed up</h2></div>${steeringView(b, meta)}</div>
    </section>`;
}
