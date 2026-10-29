// The scorecard: the overall score and its two halves (briefs, who corrects whom), the weekly
// chart, model and category breakdowns, and the hall of fame.
import type { Filter, HallEntry, Meta, Overall, Stats } from "../api";
import { filterLabel, isFiltered, shortSourceName, subjectLabel } from "../format";
import { type Html, html, num, raw } from "../html";
import { categoryTitle, tierName, tierOf, tierRanges } from "../meta";
import { catchRate, modelName } from "../picker/picker";
import { PERIOD_WHEN, PERIODS, type Period, SCORE_VIEWS, type ScoreView } from "../state";
import { briefsSummaryView, briefsView } from "./briefs";
import { svg } from "./chrome";

export type ScorecardContext = {
  meta: Meta;
  filter: Filter;
  /** The verdict is the same one already drawn, so its scrawl doesn't replay. */
  still: boolean;
  /** The same, for the briefs verdict. */
  briefsStill?: boolean;
  /** Which section is showing (default: the overview). */
  view?: ScoreView;
  /** How far back the counts look (default: 28 days). */
  period?: Period;
};

const PERIOD_TITLES: Record<Period, string> = { "7d": "7 days", "28d": "28 days", "90d": "90 days", all: "All time" };
const PERIOD_EYEBROW: Record<Period, string> = { "7d": "Last 7 days", "28d": "Last 28 days", "90d": "Last 90 days", all: "All time" };

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

const VIEW_TITLES: Record<ScoreView, string> = { overview: "Overview", briefs: "Briefs", catches: "Catches", hall: "Hall of fame" };

/** Line icons beside each section's name (decoration: the name is the label). */
const VIEW_ICONS: Record<ScoreView, Html> = {
  // A dashboard: four panels.
  overview: svg("M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z"),
  // A page with lines: what you wrote.
  briefs: svg("M6 3h9l4 4v14H6zM14 3v5h5M9 12h7M9 16h5"),
  // A check in a circle: a mistake caught.
  catches: svg("M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12l3 3 5-6"),
  // A star.
  hall: svg("M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z"),
};

const CHEVRON = raw(`<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 4.5 6 8l3.5-3.5"/></svg>`);

/** The model picker, shown as the question's subject ("Could [Claude Opus 5.5] replace you?"). One per page. */
export const subjectButton = (ctx: Pick<ScorecardContext, "filter" | "meta">): Html =>
  html`<button id="model-trigger" class="subject" data-action="model-picker" aria-haspopup="listbox" aria-expanded="false" title="Pick a model">${subjectLabel(
    ctx.filter,
    ctx.meta,
  )}${CHEVRON}</button>`;

/** "You caught it 37 times in the last 28 days." */
const caughtLine = (s: Stats, period: Period) => html`You caught it <b>${num(s.catches)} ${s.catches === 1 ? "time" : "times"}</b> ${PERIOD_WHEN[period]}.`;

const plural = (n: number, one: string, many = `${one}s`) => `${num(n)} ${n === 1 ? one : many}`;

/** The hand-written answer, with a screen-reader version. */
function answerView(o: Overall, meta: Meta, still: boolean, small = false): Html {
  const no = o.verdict === "no";
  return html`<div class="no${small ? " sm" : ""}${no ? "" : " maybe"}${still ? " still" : ""}" aria-hidden="true">${no ? "No." : "Hmm."}</div>
    <span class="sr-only">${no ? "No." : "Not yet."} ${o.tier ? `Your tier is ${tierName(meta, o.tier)}, score ${o.score}.` : "Not scored yet."}</span>`;
}

/** Why the answer is what it is: the two parts, or how far a score is. */
function scoreBecause(o: Overall, when: string): Html {
  if (o.score == null)
    return html`Give it <b>${plural(o.to_rank, "more task")}</b> with a follow-up to get a score. One-shot tasks can't need steering, so they don't count.`;
  return html`Score <b>${o.score}</b>. Your tasks landed <b>${o.direction}%</b> of the time, and you caught it <b>${plural(o.catches, "time")}</b> to its ${num(o.humbled)} ${when}.`;
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
    ${part("Your briefs", o.direction, meta.direction_weight, "tasks that landed")}
    ${part("Who corrects whom", o.judgment, 100 - meta.direction_weight, "how rarely it has to correct you")}
  </div>`;
}

function ladderView(o: Overall, meta: Meta): Html {
  return html`<ol class="ladder" aria-label="Tiers">${tierRanges(meta).map(
    (t) => html`<li${t.id === o.tier ? raw(' class="on" aria-current="true"') : ""}><span>${t.name}</span><span>${t.range}</span></li>`,
  )}</ol>`;
}

/** The headline: could it replace you? Both sides of working with AI, one score. */
function heroView(s: Stats, ctx: ScorecardContext): Html {
  const o = s.overall;
  return html`<section class="card hero">
      <div>
        <div class="eyebrow">${PERIOD_EYEBROW[ctx.period ?? "28d"]}</div>
        <h1 class="q">Could ${subjectButton(ctx)} replace you?</h1>
        <div class="verdict">${answerView(o, ctx.meta, ctx.still)}<div class="because">${scoreBecause(o, PERIOD_WHEN[ctx.period ?? "28d"])}</div></div>
        ${partsView(o, ctx.meta)}
      </div>
      <div class="meter">
        ${ladderView(o, ctx.meta)}
        <p class="hand-note">${scoreNote(o, ctx.meta)}</p>
        ${o.tier ? html`<div class="share"><button data-action="open-share">Share your score</button></div>` : ""}
      </div>
    </section>`;
}

/** You vs it: all time, and in the window. */
function versusView(s: Stats): Html {
  return html`<div class="score">
      <div class="side you"><span class="eyebrow">You caught it</span><span class="n">${num(s.catches)}</span></div>
      <div class="vs">vs</div>
      <div class="side ai"><span class="eyebrow">It caught you</span><span class="n">${num(s.humbled)}</span></div>
    </div>`;
}

/** Why fewer catches never costs points. */
const proofNote = (meta: Meta) =>
  html`<p class="muted small">${meta.catch_proof} catches show you check its work. Past that, more don't add points, so fewer catches because it's getting it right never costs you.</p>`;

/** The overview's "Who corrects whom?" half. */
function correctsSummaryView(s: Stats, ctx: ScorecardContext): Html {
  return html`<article class="card mini">
      <div class="eyebrow">Who corrects whom</div>
      <h2 class="q">Do you catch its mistakes?</h2>
      <p class="because">${caughtLine(s, ctx.period ?? "28d")}</p>
      ${versusView(s)}
      ${proofNote(ctx.meta)}
      <div class="share"><button class="ghost" data-action="view" data-id="catches">See catches</button></div>
    </article>`;
}

/** The weekly bar chart, with a text summary for screen readers. */
export function chartView(s: Stats): Html {
  const weeks = s.weekly.slice(-20);
  if (!weeks.length) return html`<p class="muted">No weeks yet.</p>`;
  const W = 900;
  const H = 240;
  const L = 34;
  const R = 10;
  const T = 26;
  const B = 30;
  const max = Math.max(10, ...weeks.map((w) => w.catches));
  const step = max > 40 ? 20 : max > 20 ? 10 : 5;
  const bw = (W - L - R) / weeks.length;
  const y = (v: number) => T + (H - T - B) * (1 - v / max);
  const peak = Math.max(...weeks.map((w) => w.catches));
  const grid: Html[] = [];
  for (let g = 0; g <= max; g += step) {
    grid.push(html`<line x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}" class="grid"/><text x="${L - 8}" y="${y(g) + 4}" text-anchor="end">${g}</text>`);
  }
  const bars = weeks.map((w, i) => {
    const x = L + i * bw + bw * 0.18;
    const bwid = bw * 0.64;
    const barTop = y(w.catches);
    const hot = w.catches > 0 && w.catches === peak;
    const d = new Date(`${w.week}T00:00`);
    return html`<rect x="${x}" y="${barTop}" width="${bwid}" height="${y(0) - barTop}" rx="3" class="${hot ? "bar hot" : "bar"}"><title>Week of ${d.toLocaleDateString()}: ${w.catches} catches, ${w.messages} messages</title></rect>${
      hot ? html`<text x="${x + bwid / 2}" y="${barTop - 6}" text-anchor="middle" class="hotlabel">${w.catches}</text>` : ""
    }${i % 2 === 0 ? html`<text x="${x + bwid / 2}" y="${H - 10}" text-anchor="middle">${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}</text>` : ""}`;
  });
  return html`<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="Catches per week" aria-describedby="chart-summary">${grid}${bars}</svg>
    <p class="sr-only" id="chart-summary">${chartSummary(weeks)}</p>`;
}

/** One sentence for screen readers: span and peak week. */
export function chartSummary(weeks: Stats["weekly"]): string {
  const first = weeks[0];
  const peak = weeks.reduce<Stats["weekly"][number] | undefined>((p, w) => (!p || w.catches > p.catches ? w : p), undefined);
  if (!first || !peak) return "No weeks yet.";
  const fmt = (iso: string) => new Date(`${iso}T00:00`).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const weeksText = `${weeks.length} ${weeks.length === 1 ? "week" : "weeks"}`;
  return `${weeksText} from ${fmt(first.week)}. Most catches: ${peak.catches}, the week of ${fmt(peak.week)}.`;
}

export function hallView(entries: HallEntry[]): Html {
  if (!entries.length) {
    return html`<p class="muted">No saved catches to show. Your privacy setting may be "category only", or the text has aged out. Counts are unaffected.</p>`;
  }
  return html`<div class="fame">${entries.map(
    (e) => html`
      <article class="catch${e.pinned ? " pinned" : ""}">
        ${e.pinned ? html`<span class="pinned-tag">Pinned · kept forever</span>` : ""}
        <div class="meta"><span class="chip">${e.category_title}</span><span class="chip conf">${e.label} · ${e.day}</span></div>
        ${e.human ? html`<div class="you-said">“${e.human}”</div>` : ""}
        ${e.ai ? html`<div class="ai-said"><b>${e.model || shortSourceName(e.source_name)}:</b> ${e.ai}</div>` : ""}
        <div class="catch-actions">
          <button class="ghost small" data-action="share-catch" data-id="${e.id}">Share</button>
          <button class="ghost small" data-action="pin" data-id="${e.id}" data-pinned="${String(e.pinned)}">${e.pinned ? "Unpin" : "Pin, keep forever"}</button>
        </div>
      </article>`,
  )}</div>`;
}

/** The sub-tabs under the toolbar. Plain buttons, like the main nav, with the current one marked. */
function viewsNav(view: ScoreView): Html {
  return html`<nav class="views" aria-label="Scorecard sections">${SCORE_VIEWS.map(
    (v) =>
      html`<button class="tab${v === view ? " on" : ""}" data-action="view" data-id="${v}"${v === view ? raw(' aria-current="page"') : ""}>${VIEW_ICONS[v]}${VIEW_TITLES[v]}</button>`,
  )}</nav>`;
}

/** The headline score, then its two halves side by side, each linking to its details. */
function overviewView(s: Stats, ctx: ScorecardContext): Html {
  return html`
    ${heroView(s, ctx)}
    <section class="duo">
      ${briefsSummaryView(s.briefs, ctx.meta, ctx.briefsStill ?? false, PERIOD_WHEN[ctx.period ?? "28d"])}
      ${correctsSummaryView(s, ctx)}
    </section>
    ${statsView(s)}`;
}

function statsView(s: Stats): Html {
  const peak = s.peak_hour == null ? "–" : new Date(2000, 0, 1, s.peak_hour).toLocaleTimeString(undefined, { hour: "numeric" });
  return html`<section class="stats">
      <div class="stat"><span class="v">${num(s.catches)}</span><span class="l">catches</span></div>
      <div class="stat"><span class="v">${s.pct_real_chats_with_catch != null ? `${Math.round(s.pct_real_chats_with_catch)}%` : "–"}</span><span class="l">of real chats had a catch</span></div>
      <div class="stat"><span class="v">${s.best_streak_days} days</span><span class="l">best streak · now ${s.current_streak_days}</span></div>
      <div class="stat"><span class="v">${s.messages_per_catch ? `1 in ${s.messages_per_catch}` : "–"}</span><span class="l">messages is a catch · peak ${peak}</span></div>
    </section>`;
}

function catchesView(s: Stats, ctx: ScorecardContext): Html {
  const { meta } = ctx;
  const cats = Object.entries(s.categories).sort((a, b) => b[1] - a[1]);
  const topCat = cats[0]?.[1] ?? 1;
  const models = s.models
    .filter((m) => m.messages > 0)
    .map(
      (m) =>
        html`<tr><td>${modelName(m)}</td><td class="n">${num(m.catches)}</td><td class="n">${num(m.messages)}</td><td class="n">${catchRate(m.catches, m.messages)}</td></tr>`,
    );
  const catRows = cats.length
    ? cats.map(
        ([k, v]) =>
          html`<div class="cat"><span>${categoryTitle(meta, k)}</span><span class="bar-bg"><i data-w="${Math.max(2, (v / topCat) * 100)}"></i></span><span class="n">${v}</span></div>`,
      )
    : html`<p class="muted">No catches yet.</p>`;
  return html`
    <section class="card">
      <div>
        <div class="eyebrow">Who corrects whom</div>
        <h1 class="q">Do you catch ${subjectButton(ctx)}'s mistakes?</h1>
        <p class="because">${caughtLine(s, ctx.period ?? "28d")}${
          s.pct_real_chats_with_catch != null ? ` It slipped up in ${Math.round(s.pct_real_chats_with_catch)}% of your real chats.` : ""
        }</p>
        ${versusView(s)}
      </div>
      <div class="meter">
        <p class="big-stat"><span class="n">${s.overall.judgment ?? "–"}</span><span class="muted small">judgment ${PERIOD_WHEN[ctx.period ?? "28d"]}, out of 100: you check its work, and it rarely has to correct you</span></p>
        ${proofNote(meta)}
      </div>
    </section>
    <section><div class="head"><h2>Catches per week</h2></div><div class="chart">${chartView(s)}</div></section>
    <section class="split">
      <div>
        <div class="head"><h2>By model</h2></div>
        <div class="models-scroll" tabindex="0" role="region" aria-label="Catches by model"><table class="models"><thead><tr><th>Model</th><th>Catches</th><th>Messages</th><th>Rate</th></tr></thead><tbody>
        ${models}
        </tbody></table></div>
      </div>
      <div>
        <div class="head"><h2>What you catch</h2></div>
        <div class="cats">${catRows}</div>
      </div>
    </section>`;
}

/** One award per kind of catch, plus one for the times it caught you. Keys are category ids ("" is humbled). */
export const AWARDS: Record<string, { emoji: string; name: string; blurb: string }> = {
  made_it_up: { emoji: "🦄", name: "The Fabulist", blurb: "Made it up with a straight face." },
  ignored_instructions: { emoji: "🙉", name: "Selective Hearing", blurb: "You said it. It heard something else." },
  misread: { emoji: "🤓", name: "Reading Is Hard", blurb: "Got the gist. The wrong gist." },
  missed_something: { emoji: "🙈", name: "The Blind Spot", blurb: "It was right there. Right. There." },
  still_broken: { emoji: "🔧", name: "“Fixed” It", blurb: "Declared victory. The bug disagreed." },
  just_wrong: { emoji: "🧠", name: "Big Brain Energy", blurb: "Confidently, spectacularly wrong." },
  "": { emoji: "🪞", name: "Touché", blurb: "The times it caught you. Fair's fair." },
};

/** An award: its count, its best snippet, and the runners-up. Unearned awards wait, dimmed. */
function awardView(key: string, count: number, entries: HallEntry[], meta: Meta): Html {
  const a = AWARDS[key] ?? { emoji: "🏅", name: categoryTitle(meta, key), blurb: "" };
  const [best, ...rest] = entries;
  const body = !count
    ? html`<p class="muted small">Not earned yet.</p>`
    : best
      ? html`${hallView([best])}${rest.length ? html`<details class="runners-up"><summary>${rest.length} more</summary>${hallView(rest)}</details>` : ""}`
      : html`<p class="muted small">No snippets kept. Your privacy setting may be “category only”, or the text has aged out.</p>`;
  return html`<article class="award${count ? "" : " unearned"}">
    <header><span class="award-emoji" aria-hidden="true">${a.emoji}</span><div><h3>${a.name}</h3><p class="muted small">${a.blurb}</p></div><span class="award-count" aria-label="${count} times">×${num(count)}</span></header>
    ${body}
  </article>`;
}

function hallSection(s: Stats, ctx: ScorecardContext): Html {
  const keys = [...ctx.meta.categories.map((c) => c.id), ""];
  const count = (k: string) => (k ? (s.categories[k] ?? 0) : s.humbled);
  // Most earned first; unearned last, in display order.
  const order = keys.map((k, i) => ({ k, i, n: count(k) })).sort((x, y) => Number(y.n > 0) - Number(x.n > 0) || y.n - x.n || x.i - y.i);
  return html`<section><div class="head"><h2>Hall of fame · ${subjectButton(ctx)}</h2><p class="muted">The best catch for each award, picked from your history. Snippets are redacted and kept per your privacy setting.</p></div>
    <div class="awards">${order.map(({ k, n }) =>
      awardView(
        k,
        n,
        s.hall_of_fame.filter((e) => e.category === k),
        ctx.meta,
      ),
    )}</div></section>`;
}

export function scorecardView(s: Stats, ctx: ScorecardContext): Html {
  const { meta, filter } = ctx;
  const view = ctx.view ?? "overview";
  const toolbar = isFiltered(filter)
    ? html`<section class="toolbar"><button class="chip-clear" data-action="clear-filter">Showing ${filterLabel(filter, meta)} only <span aria-hidden="true">×</span></button></section>`
    : "";
  const body =
    view === "catches"
      ? catchesView(s, ctx)
      : view === "briefs"
        ? briefsView(s.briefs, meta, ctx.briefsStill ?? false, subjectButton(ctx), PERIOD_WHEN[ctx.period ?? "28d"])
        : view === "hall"
          ? hallSection(s, ctx)
          : overviewView(s, ctx);
  return html`
    ${toolbar}
    <div class="views-row">${viewsNav(view)}${periodNav(ctx.period ?? "28d")}</div>
    <div class="view-body">${body}</div>`;
}

/** Shown before there's anything to score. `watching` names the sources being read right now. */
export function onboardingView(watching: string): Html {
  if (watching) {
    // Allowed, but nothing counted yet (picked "From now on", or the tool has no history yet).
    return html`<section class="empty"><span class="eyebrow"><i class="dot live"></i> Watching ${watching}</span>
      <h1 class="q">You're all set.</h1>
      <p>Your next chats will be counted as they happen. The first time you catch your AI being wrong, your scorecard shows up here.</p>
      <button class="ghost" data-action="go-sources">Manage sources</button></section>`;
  }
  return html`<section class="empty"><h1 class="q">Let's see if AI could replace you.</h1>
    <p>No, Actually... counts the times you caught your AI being wrong, privately, on this computer. Pick which tools it may read.</p>
    <button data-action="go-sources">Choose sources</button></section>`;
}
