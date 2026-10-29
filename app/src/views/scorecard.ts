// The scorecard: the overall score and its two halves (prompts, who corrects whom), the weekly
// chart, model and category breakdowns, and the hall of fame.
import type { Filter, HallEntry, Meta, Overall, Stats, WeekRow } from "../api";
import { fmtDay, plural, shortSourceName, subjectLabel, subjectText } from "../format";
import { type Html, html, num, raw } from "../html";
import { categoryTitle, correctionTier, correctionTierRanges, tierName, tierOf, tierRanges } from "../meta";
import { correctionRate, modelName } from "../picker/picker";
import { PERIOD_WHEN, PERIODS, type Period, SCORE_VIEWS, type ScoreView } from "../state";
import { answerWord, byAnswer, ladderView, svg, tierBlurb, verdictMark } from "./chrome";
import { promptsSummaryView, promptsView } from "./prompts";

export type ScorecardContext = {
  meta: Meta;
  filter: Filter;
  /** The verdict is the same one already drawn, so its scrawl doesn't replay. */
  still: boolean;
  /** The same, for the prompts verdict. */
  promptsStill?: boolean;
  /** The same, for the corrections verdict. */
  correctionsStill?: boolean;
  /** Which section is showing (default: the overview). */
  view?: ScoreView;
  /** How far back the counts look (default: 28 days). */
  period?: Period;
  /** Hall of fame: the entry id each award shows (default: its best). */
  awardSlide?: Record<string, number>;
};

/** The context with its defaults filled in once, for the views below. */
type Ctx = ScorecardContext & { period: Period; when: string; promptsStill: boolean; correctionsStill: boolean };

const PERIOD_TITLES: Record<Period, string> = { "7d": "7 days", "28d": "28 days", "90d": "90 days", all: "All time" };

/** The scorecard's time picker: how far back every count looks. Buttons on wide windows, a
 * dropdown on narrow ones (CSS shows one; the hidden one is out of the accessibility tree). */
function periodNav(period: Period): Html {
  return html`<nav class="views period" aria-label="Time period">${PERIODS.map(
    (p) =>
      html`<button class="tab${p === period ? " on" : ""}" data-action="period" data-id="${p}"${p === period ? raw(' aria-current="true"') : ""}>${PERIOD_TITLES[p]}</button>`,
  )}</nav><select class="period-select" data-action="period-pick" aria-label="Time period">${PERIODS.map(
    (p) => html`<option value="${p}"${p === period ? raw(" selected") : ""}>${PERIOD_TITLES[p]}</option>`,
  )}</select>`;
}

const VIEW_TITLES: Record<ScoreView, string> = { overview: "Overview", prompts: "Prompting", corrections: "Corrections", hall: "Hall of fame" };

/** Line icons beside each section's name (decoration: the name is the label). */
const VIEW_ICONS: Record<ScoreView, Html> = {
  // A dashboard: four panels.
  overview: svg("M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z"),
  // A page with lines: what you wrote.
  prompts: svg("M6 3h9l4 4v14H6zM14 3v5h5M9 12h7M9 16h5"),
  // A check in a circle: a mistake corrected.
  corrections: svg("M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12l3 3 5-6"),
  // A star.
  hall: svg("M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z"),
};

const CHEVRON = raw(`<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 4.5 6 8l3.5-3.5"/></svg>`);

/** The model picker, beside the time range, so it reads as filtering the whole page. */
export const modelPicker = (ctx: Pick<ScorecardContext, "filter" | "meta">): Html =>
  html`<button id="model-trigger" class="model-pick" data-action="model-picker" aria-haspopup="listbox" aria-expanded="false" aria-label="Model: ${subjectLabel(
    ctx.filter,
    ctx.meta,
  )}. Pick a model">${subjectLabel(ctx.filter, ctx.meta)}${CHEVRON}</button>`;

/** The hand-written answer, with a screen-reader version. */
function answerView(o: Overall, meta: Meta, still: boolean): Html {
  const no = o.verdict === "no";
  return verdictMark(
    no ? "No." : null,
    `${no ? "No." : "Not yet."} ${o.tier ? `Your tier is ${tierName(meta, o.tier)}, score ${o.score}.` : "Not scored yet."}`,
    still,
  );
}

/** Why the answer is what it is: the two parts, or how far a score is. */
function scoreBecause(o: Overall, when: string): Html {
  if (o.score == null)
    return html`Give it <b>${plural(o.to_rank, "more task")}</b> with a follow-up to get a score. One-shot tasks can't need steering, so they don't count.`;
  return html`Score <b>${o.score}</b>. Your tasks landed <b>${o.direction}%</b> of the time, and you corrected it <b>${plural(o.corrections, "time")}</b> to its ${num(o.humbled)} ${when}.`;
}

/** The next tier's target ("Irreplaceable at 90. You're at 86."). */
function scoreNote(o: Overall, meta: Meta): string {
  if (o.score == null) return `${plural(o.to_rank, "more task")} to get a score`;
  const next = tierOf(meta, o.next_tier);
  return next ? `${next.name} at ${next.floor}. You're at ${o.score}.` : "Top tier. Don't let it slip.";
}

/** The two parts of the score, as bars, with what each is worth. */
function partsView(o: Overall, meta: Meta): Html {
  const part = (label: string, value: number | null, weight: number, hint: string) =>
    html`<div class="part"><span class="part-name">${label}<small class="muted">${hint} · ${weight}% of the score</small></span><span class="bar-bg"><i data-w="${value ?? 0}"></i></span><span class="n">${value ?? "–"}</span></div>`;
  return html`<div class="parts" role="group" aria-label="What the score is made of">
    ${part("Who corrects whom", o.judgment, 100 - meta.direction_weight, "how rarely it has to correct you")}
    ${part("Your prompts", o.direction, meta.direction_weight, "tasks that landed")}
  </div>`;
}

function scoreLadderView(o: Overall, meta: Meta): Html {
  return ladderView({ label: "Tiers", tiers: tierRanges(meta), on: o.tier, at: o.score == null ? null : String(o.score), unranked: scoreNote(o, meta) });
}

/** The headline: could it replace you? Both sides of working with AI, one score. */
function heroView(s: Stats, ctx: Ctx): Html {
  const o = s.overall;
  return html`<section class="card hero">
      <div>
        <h1 class="q">Could ${subjectText(ctx.filter, ctx.meta)} replace you?</h1>
        <div class="verdict">${answerView(o, ctx.meta, ctx.still)}<div class="because">${scoreBecause(o, ctx.when)}</div></div>
        ${partsView(o, ctx.meta)}
      </div>
      <div class="meter">
        ${scoreLadderView(o, ctx.meta)}
        ${o.tier ? html`<div class="share"><button data-action="open-share">Share your score</button></div>` : ""}
      </div>
    </section>`;
}

/** "Religiously.", hand-written, with a screen-reader version. */
function correctionMark(o: Overall, meta: Meta, still: boolean): Html {
  const tier = correctionTier(meta, o.correction_tier);
  return verdictMark(tier?.verdict ?? null, tier ? `${tier.verdict} Your correction tier is ${tier.name}.` : "Not ranked yet.", still);
}

/** Why: who corrected whom, in one line, with the same counts as "you vs it" beside it. */
function correctionBecause(s: Stats, when: string): Html {
  if (s.overall.judgment == null) return html`Give it <b>${plural(s.overall.to_rank, "more task")}</b> with a follow-up to get ranked.`;
  if (!s.corrections) return html`You haven't corrected it ${when}, so there's no proof yet that you check its work.`;
  return html`You corrected it <b>${plural(s.corrections, "time")}</b> ${when}. It corrected you ${s.humbled ? plural(s.humbled, "time") : "never"}.`;
}

/** The next correction tier's target ("Hawk-eyed at 90. You're at 88."). */
function correctionNote(o: Overall, meta: Meta): string {
  if (o.judgment == null) return `${plural(o.to_rank, "more task")} to get ranked`;
  const next = correctionTier(meta, o.next_correction_tier);
  return next ? `Next: ${answerWord(next)}, at ${next.floor}. You're at ${o.judgment}.` : "Top tier. Keep checking.";
}

function correctionLadderView(o: Overall, meta: Meta): Html {
  return ladderView({
    label: "Correction tiers",
    tiers: byAnswer(correctionTierRanges(meta)),
    on: o.correction_tier,
    at: o.judgment == null ? null : String(o.judgment),
    unranked: correctionNote(o, meta),
  });
}

/** The overview's "Who corrects whom?" half: the answer, the tier, and a way to the details. */
function correctsSummaryView(s: Stats, ctx: Ctx): Html {
  const o = s.overall;
  const tier = correctionTier(ctx.meta, o.correction_tier);
  return html`<article class="card mini corrections">
      <h2 class="q">Do you correct it?</h2>
      <div class="verdict">${correctionMark(o, ctx.meta, ctx.correctionsStill)}<div class="because">${correctionBecause(s, ctx.when)}</div></div>
      ${tierBlurb(tier)}<p class="hand-note">${correctionNote(o, ctx.meta)}</p>
      <div class="share"><button class="ghost" data-action="view" data-id="corrections">See corrections</button></div>
    </article>`;
}

/** A week's tip: corrections by kind, most first, then the times it corrected you. */
function correctionWeekTip(w: WeekRow, meta: Meta): string {
  const lines = [`Week of ${fmtDay(w.week)}`, `${plural(w.corrections, "correction")} · ${plural(w.messages, "message")}`];
  const kinds = Object.entries(w.categories)
    .filter(([, n]) => n > 0)
    .sort((a, b) => b[1] - a[1]);
  if (kinds.length) lines.push("", ...kinds.map(([id, n]) => `${categoryTitle(meta, id)} ${n}`));
  if (w.humbled) lines.push("", `It corrected you ${plural(w.humbled, "time")}`);
  return lines.join("\n");
}

/** The weekly bar chart, with a text summary for screen readers. */
export function chartView(s: Stats, meta: Meta): Html {
  const weeks = s.weekly.slice(-20);
  if (!weeks.length) return html`<p class="muted">No weeks yet.</p>`;
  const W = 900;
  const H = 240;
  const L = 34;
  const R = 10;
  const T = 26;
  const B = 30;
  const max = Math.max(10, ...weeks.map((w) => w.corrections));
  const step = max > 40 ? 20 : max > 20 ? 10 : 5;
  const bw = (W - L - R) / weeks.length;
  const y = (v: number) => T + (H - T - B) * (1 - v / max);
  const peak = Math.max(...weeks.map((w) => w.corrections));
  const grid: Html[] = [];
  for (let g = 0; g <= max; g += step) {
    grid.push(html`<line x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}" class="grid"/><text x="${L - 8}" y="${y(g) + 4}" text-anchor="end">${g}</text>`);
  }
  const bars = weeks.map((w, i) => {
    const x = L + i * bw + bw * 0.18;
    const bwid = bw * 0.64;
    const barTop = y(w.corrections);
    // The busiest week gets a label, not a color: more corrections isn't better, so no week stands out as the good one.
    const busiest = w.corrections > 0 && w.corrections === peak && weeks.findIndex((v) => v.corrections === peak) === i;
    return html`<rect x="${x}" y="${barTop}" width="${bwid}" height="${y(0) - barTop}" rx="3" class="bar" data-tip="${correctionWeekTip(w, meta)}"></rect>${
      busiest ? html`<text x="${x + bwid / 2}" y="${barTop - 6}" text-anchor="middle" class="peak-label">Most: ${w.corrections}</text>` : ""
    }${i % 2 === 0 ? html`<text x="${x + bwid / 2}" y="${H - 10}" text-anchor="middle">${fmtDay(w.week)}</text>` : ""}`;
  });
  return html`<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="Corrections per week" aria-describedby="chart-summary">${grid}${bars}</svg>
    <p class="sr-only" id="chart-summary">${chartSummary(weeks)}</p>`;
}

/** One sentence for screen readers: span and peak week. */
export function chartSummary(weeks: Stats["weekly"]): string {
  const first = weeks[0];
  const peak = weeks.reduce<Stats["weekly"][number] | undefined>((p, w) => (!p || w.corrections > p.corrections ? w : p), undefined);
  if (!first || !peak) return "No weeks yet.";
  return `${weeks.length} ${weeks.length === 1 ? "week" : "weeks"} from ${fmtDay(first.week)}. Most corrections: ${peak.corrections}, the week of ${fmtDay(peak.week)}.`;
}

export function hallView(entries: HallEntry[]): Html {
  if (!entries.length) {
    return html`<p class="muted">No saved corrections to show. Your privacy setting may be "category only", or the text has aged out. Counts are unaffected.</p>`;
  }
  return html`<div class="fame">${entries.map(
    (e) => html`
      <article class="correction${e.pinned ? " pinned" : ""}">
        ${e.pinned ? html`<span class="pinned-tag">Pinned · kept forever</span>` : ""}
        <div class="meta">${e.label} · ${e.day}</div>
        ${e.human ? html`<div class="you-said">“${e.human}”</div>` : ""}
        ${e.ai ? html`<div class="ai-said"><b>${e.model || shortSourceName(e.source_name)}:</b> ${e.ai}</div>` : ""}
        <div class="correction-actions">
          <button class="ghost small" data-action="share-correction" data-id="${e.id}">Share</button>
          <button class="ghost small" data-action="pin" data-id="${e.id}" data-pinned="${String(e.pinned)}">${e.pinned ? "Unpin" : "Pin, keep forever"}</button>
        </div>
      </article>`,
  )}</div>`;
}

/** The sub-tabs. Plain buttons, like the main nav, with the current one marked. */
function viewsNav(view: ScoreView): Html {
  return html`<nav class="views" aria-label="Scorecard sections">${SCORE_VIEWS.map(
    (v) =>
      html`<button class="tab${v === view ? " on" : ""}" data-action="view" data-id="${v}" data-tip="${VIEW_TITLES[v]}"${v === view ? raw(' aria-current="page"') : ""}>${VIEW_ICONS[v]}<span class="tab-name">${VIEW_TITLES[v]}</span></button>`,
  )}</nav>`;
}

/** The headline score, then its two halves side by side, each linking to its details. */
function overviewView(s: Stats, ctx: Ctx): Html {
  return html`
    ${heroView(s, ctx)}
    <section class="duo">
      ${correctsSummaryView(s, ctx)}
      ${promptsSummaryView(s.prompts, ctx.meta, ctx.promptsStill, ctx.when)}
    </section>
    ${statsView(s)}`;
}

function statsView(s: Stats): Html {
  const peak = s.peak_hour == null ? "–" : new Date(2000, 0, 1, s.peak_hour).toLocaleTimeString(undefined, { hour: "numeric" });
  return html`<section class="stats">
      <div class="stat"><span class="v">${num(s.corrections)}</span><span class="l">corrections</span></div>
      <div class="stat"><span class="v">${s.pct_real_chats_with_correction != null ? `${Math.round(s.pct_real_chats_with_correction)}%` : "–"}</span><span class="l">of real chats had a correction</span></div>
      <div class="stat"><span class="v">${s.best_streak_days} days</span><span class="l">best streak · now ${s.current_streak_days}</span></div>
      <div class="stat"><span class="v">${s.messages_per_correction ? `1 in ${s.messages_per_correction}` : "–"}</span><span class="l">messages is a correction · peak ${peak}</span></div>
    </section>`;
}

function correctionsView(s: Stats, ctx: Ctx): Html {
  const { meta } = ctx;
  const cats = Object.entries(s.categories).sort((a, b) => b[1] - a[1]);
  const topCat = cats[0]?.[1] ?? 1;
  const models = s.models
    .filter((m) => m.messages > 0)
    .map(
      (m) =>
        html`<tr><td>${modelName(m)}</td><td class="n">${num(m.corrections)}</td><td class="n">${num(m.messages)}</td><td class="n">${correctionRate(m.corrections, m.messages)}</td></tr>`,
    );
  const catRows = cats.length
    ? cats.map(
        ([k, v]) =>
          html`<div class="cat"><span>${categoryTitle(meta, k)}</span><span class="bar-bg"><i data-w="${Math.max(2, (v / topCat) * 100)}"></i></span><span class="n">${v}</span></div>`,
      )
    : html`<p class="muted">No corrections yet.</p>`;
  return html`
    <section class="card corrections">
      <div>
        <h1 class="q">Do you correct ${subjectText(ctx.filter, ctx.meta)}?</h1>
        <div class="verdict">${correctionMark(s.overall, meta, ctx.correctionsStill)}<div class="because">${correctionBecause(s, ctx.when)}</div></div>
        ${tierBlurb(correctionTier(meta, s.overall.correction_tier))}
      </div>
      <div class="meter">
        ${correctionLadderView(s.overall, meta)}
        ${correctionTier(meta, s.overall.correction_tier) ? html`<div class="share"><button data-action="share-corrections">Share your corrections</button></div>` : ""}
      </div>
    </section>
    <section><div class="head"><h2>Corrections per week</h2></div><div class="chart">${chartView(s, meta)}</div></section>
    <section class="split">
      <div>
        <div class="head"><h2>By model</h2></div>
        <div class="models-scroll" tabindex="0" role="region" aria-label="Corrections by model"><table class="models"><thead><tr><th>Model</th><th>Corrections</th><th>Messages</th><th>Rate</th></tr></thead><tbody>
        ${models}
        </tbody></table></div>
      </div>
      <div>
        <div class="head"><h2>What you correct</h2></div>
        <div class="cats">${catRows}</div>
      </div>
    </section>`;
}

/** One award per kind of correction, plus one for the times it corrected you. Keys are category ids ("" is humbled). */
export const AWARDS: Record<string, { emoji: string; name: string; blurb: string }> = {
  made_it_up: { emoji: "🦄", name: "The Fabulist", blurb: "Made it up with a straight face." },
  ignored_instructions: { emoji: "🙉", name: "Selective Hearing", blurb: "You said it. It heard something else." },
  misread: { emoji: "🤓", name: "Reading Is Hard", blurb: "Got the gist. The wrong gist." },
  missed_something: { emoji: "🙈", name: "The Blind Spot", blurb: "It was right there. Right. There." },
  still_broken: { emoji: "🔧", name: "“Fixed” It", blurb: "Declared victory. The bug disagreed." },
  just_wrong: { emoji: "🧠", name: "Big Brain Energy", blurb: "Confidently, spectacularly wrong." },
  "": { emoji: "🪞", name: "Touché", blurb: "The times it corrected you. Fair's fair." },
};

/** Drawn chevrons: text arrows sit on the font's baseline and never center in a round button. */
const ARROW_PREV = raw(`<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M7.5 2.5 4 6l3.5 3.5"/></svg>`);
const ARROW_NEXT = raw(`<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5 8 6l-3.5 3.5"/></svg>`);

/** An award's entries, best first, one at a time. All of them share one grid cell, so the card is as
 * tall as the tallest and flipping never moves the page; prev/next flip in place (actions.ts). */
function slidesView(key: string, entries: HallEntry[], showing: number | undefined): Html {
  // The entry you were on, wherever it sits now; the best one if it's gone.
  const i = Math.max(
    0,
    entries.findIndex((e) => e.id === showing),
  );
  const slides = html`<div class="slides">${entries.map(
    (e, n) => html`<div class="slide" data-entry="${e.id}"${n === i ? "" : raw(' aria-hidden="true" inert')}>${hallView([e])}</div>`,
  )}</div>`;
  if (entries.length < 2) return slides;
  const nav = html`<div class="slide-nav">
      <button class="ghost small" data-action="award-slide" data-id="${key}" data-step="-1" aria-label="Previous">${ARROW_PREV}</button>
      <span class="muted small" aria-live="polite"><span class="slide-at">${i + 1}</span> of top ${entries.length}</span>
      <button class="ghost small" data-action="award-slide" data-id="${key}" data-step="1" aria-label="Next">${ARROW_NEXT}</button>
    </div>`;
  return html`${slides}${nav}`;
}

/** An award: its count and its snippets, best first. Unearned awards wait, dimmed. */
function awardView(key: string, count: number, entries: HallEntry[], meta: Meta, showing?: number): Html {
  const a = AWARDS[key] ?? { emoji: "🏅", name: categoryTitle(meta, key), blurb: "" };
  const body = !count
    ? html`<p class="muted small">Not earned yet.</p>`
    : entries.length
      ? slidesView(key, entries, showing)
      : html`<p class="muted small">No snippets kept. Your privacy setting may be “category only”, or the text has aged out.</p>`;
  return html`<article class="award${count ? "" : " unearned"}" data-award="${key}">
    <header><span class="award-emoji" aria-hidden="true">${a.emoji}</span><div><h3>${a.name}</h3><p class="muted small">${a.blurb}</p></div><span class="award-count" aria-label="${count} times">×${num(count)}</span></header>
    ${body}
  </article>`;
}

function hallSection(s: Stats, ctx: Ctx): Html {
  const keys = [...ctx.meta.categories.map((c) => c.id), ""];
  const count = (k: string) => (k ? (s.categories[k] ?? 0) : s.humbled);
  // Most earned first; unearned last, in display order.
  const order = keys.map((k, i) => ({ k, i, n: count(k) })).sort((x, y) => Number(y.n > 0) - Number(x.n > 0) || y.n - x.n || x.i - y.i);
  return html`<section><div class="head"><h2>Hall of fame · ${subjectText(ctx.filter, ctx.meta)}</h2><p class="muted">The best correction for each award, picked from your history. Snippets are redacted and kept per your privacy setting.</p></div>
    <div class="awards">${order.map(({ k, n }) =>
      awardView(
        k,
        n,
        s.hall_of_fame.filter((e) => e.category === k),
        ctx.meta,
        ctx.awardSlide?.[k],
      ),
    )}</div></section>`;
}

export function scorecardView(s: Stats, input: ScorecardContext): Html {
  const period = input.period ?? "28d";
  const ctx: Ctx = {
    ...input,
    period,
    when: PERIOD_WHEN[period],
    promptsStill: input.promptsStill ?? false,
    correctionsStill: input.correctionsStill ?? false,
  };
  const { meta } = ctx;
  const view = ctx.view ?? "overview";
  const body =
    view === "corrections"
      ? correctionsView(s, ctx)
      : view === "prompts"
        ? promptsView(s.prompts, meta, ctx.promptsStill, subjectText(ctx.filter, meta), ctx.period, s.weekly)
        : view === "hall"
          ? hallSection(s, ctx)
          : overviewView(s, ctx);
  return html`
    <div class="views-row">${viewsNav(view)}<div class="filters">${modelPicker(ctx)}${periodNav(period)}</div></div>
    <div class="view-body">${body}</div>`;
}

/** Shown before there's anything to score. `watching` names the sources being read right now. */
export function onboardingView(watching: string): Html {
  if (watching) {
    // Allowed, but nothing counted yet (picked "From now on", or the tool has no history yet).
    return html`<section class="empty"><span class="eyebrow"><i class="dot live"></i> Watching ${watching}</span>
      <h1 class="q">You're all set.</h1>
      <p>Your next chats will be counted as they happen. The first time you correct your AI, your scorecard shows up here.</p>
      <button class="ghost" data-action="go-sources">Manage sources</button></section>`;
  }
  return html`<section class="empty"><h1 class="q">Let's see if AI could replace you.</h1>
    <p>No, Actually... counts the times you corrected your AI, privately, on this computer. Pick which tools it may read.</p>
    <button data-action="go-sources">Choose sources</button></section>`;
}
