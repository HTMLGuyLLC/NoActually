// The scorecard: verdict, tier ladder, weekly chart, model and category breakdowns, hall of fame.
import type { Filter, HallEntry, Meta, Stats } from "../api";
import { filterLabel, isFiltered, shortSourceName, subjectLabel } from "../format";
import { type Html, html, num, raw } from "../html";
import { categoryTitle, tierName, tierRanges, topTier } from "../meta";
import { catchRate, modelName } from "../picker/picker";
import { SCORE_VIEWS, type ScoreView } from "../state";
import { briefsSummaryView, briefsView } from "./briefs";

export type ScorecardContext = {
  meta: Meta;
  filter: Filter;
  /** The verdict is the same one already drawn, so its scrawl doesn't replay. */
  still: boolean;
  /** The same, for the briefs verdict. */
  briefsStill?: boolean;
  /** Which section is showing (default: the overview). */
  view?: ScoreView;
};

const VIEW_TITLES: Record<ScoreView, string> = { overview: "Overview", catches: "Catches", briefs: "Briefs", hall: "Hall of fame" };

const CHEVRON = raw(`<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 4.5 6 8l3.5-3.5"/></svg>`);

/** The model picker, shown as the question's subject ("Could [Claude Opus 5.5] replace you?"). One per page. */
export const subjectButton = (ctx: Pick<ScorecardContext, "filter" | "meta">): Html =>
  html`<button id="model-trigger" class="subject" data-action="model-picker" aria-haspopup="listbox" aria-expanded="false" title="Pick a model">${subjectLabel(
    ctx.filter,
    ctx.meta,
  )}${CHEVRON}</button>`;

/** "You caught it 17 times in the last 7 days." */
const caughtLine = (s: Stats) => html`You caught it <b>${num(s.catches_last_7d)} ${s.catches_last_7d === 1 ? "time" : "times"}</b> in the last 7 days.`;

function verdictView(s: Stats, ctx: ScorecardContext): Html {
  const no = s.verdict === "no";
  return html`
    <div class="eyebrow">This week's verdict</div>
    <h1 class="q">Could ${subjectButton(ctx)} replace you?</h1>
    <div class="verdict">
      <div class="no${no ? "" : " maybe"}${ctx.still ? " still" : ""}" aria-hidden="true">${no ? "No." : "Hmm."}</div>
      <span class="sr-only">${no ? "No." : "Not yet."} Your tier is ${tierName(ctx.meta, s.tier)}.</span>
      <div class="because">${caughtLine(s)}${
        s.pct_real_chats_with_catch != null ? ` It slipped up in ${Math.round(s.pct_real_chats_with_catch)}% of your real chats.` : ""
      }</div>
    </div>
    <div class="score">
      <div class="side you"><span class="eyebrow">You caught it</span><span class="n">${num(s.catches)}</span></div>
      <div class="vs">vs</div>
      <div class="side ai"><span class="eyebrow">It caught you</span><span class="n">${num(s.humbled)}</span></div>
    </div>`;
}

function ladderView(s: Stats, meta: Meta): Html {
  return html`<ol class="ladder">${tierRanges(meta).map(
    (t) => html`<li${t.id === s.tier ? raw(' class="on" aria-current="true"') : ""}><span>${t.name}</span><span>${t.range}</span></li>`,
  )}</ol>`;
}

/** The weekly bar chart, with a text summary for screen readers. */
export function chartView(s: Stats, meta: Meta): Html {
  const weeks = s.weekly.slice(-20);
  if (!weeks.length) return html`<p class="muted">No weeks yet.</p>`;
  const top = topTier(meta);
  const line = top?.floor ?? Number.POSITIVE_INFINITY;
  const W = 900;
  const H = 240;
  const L = 34;
  const R = 10;
  const T = 26;
  const B = 30;
  const max = Math.max(20, ...weeks.map((w) => w.catches));
  const step = max > 40 ? 20 : max > 20 ? 10 : 5;
  const bw = (W - L - R) / weeks.length;
  const y = (v: number) => T + (H - T - B) * (1 - v / max);
  const grid: Html[] = [];
  for (let g = 0; g <= max; g += step) {
    grid.push(html`<line x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}" class="grid"/><text x="${L - 8}" y="${y(g) + 4}" text-anchor="end">${g}</text>`);
  }
  const topLine = top
    ? html`<line x1="${L}" x2="${W - R}" y1="${y(line)}" y2="${y(line)}" class="line-top"/><text x="${W - R}" y="${y(line) - 6}" text-anchor="end" class="hand">${top.name.toLowerCase()} line</text>`
    : "";
  const bars = weeks.map((w, i) => {
    const x = L + i * bw + bw * 0.18;
    const bwid = bw * 0.64;
    const barTop = y(w.catches);
    const hot = w.catches >= line;
    const d = new Date(`${w.week}T00:00`);
    return html`<rect x="${x}" y="${barTop}" width="${bwid}" height="${y(0) - barTop}" rx="3" class="${hot ? "bar hot" : "bar"}"><title>Week of ${d.toLocaleDateString()}: ${w.catches} catches, ${w.messages} messages</title></rect>${
      hot ? html`<text x="${x + bwid / 2}" y="${barTop - 6}" text-anchor="middle" class="hotlabel">${w.catches}</text>` : ""
    }${i % 2 === 0 ? html`<text x="${x + bwid / 2}" y="${H - 10}" text-anchor="middle">${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}</text>` : ""}`;
  });
  return html`<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="Catches per week" aria-describedby="chart-summary">${grid}${topLine}${bars}</svg>
    <p class="sr-only" id="chart-summary">${chartSummary(weeks, top?.name ?? null, line)}</p>`;
}

/** One sentence for screen readers: span, peak week, and weeks over the top tier's line. */
export function chartSummary(weeks: Stats["weekly"], topName: string | null, line: number): string {
  const first = weeks[0];
  const peak = weeks.reduce<Stats["weekly"][number] | undefined>((p, w) => (!p || w.catches > p.catches ? w : p), undefined);
  if (!first || !peak) return "No weeks yet.";
  const fmt = (iso: string) => new Date(`${iso}T00:00`).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const over = weeks.filter((w) => w.catches >= line).length;
  const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
  return `${plural(weeks.length, "week")} from ${fmt(first.week)}. Most catches: ${peak.catches}, the week of ${fmt(peak.week)}.${
    topName ? ` ${plural(over, "week")} reached the ${topName.toLowerCase()} line of ${line}.` : ""
  }`;
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
      html`<button class="tab${v === view ? " on" : ""}" data-action="view" data-id="${v}"${v === view ? raw(' aria-current="page"') : ""}>${VIEW_TITLES[v]}</button>`,
  )}</nav>`;
}

/** Both verdicts side by side, each linking to its details. */
function overviewView(s: Stats, ctx: ScorecardContext): Html {
  const no = s.verdict === "no";
  const progress = s.next_tier ? `${s.next_tier[1]} more this week for ${tierName(ctx.meta, s.next_tier[0])}` : "Top tier";
  return html`
    <section class="duo">
      <article class="card mini">
        <div class="eyebrow">This week's verdict</div>
        <h1 class="q">Could ${subjectButton(ctx)} replace you?</h1>
        <div class="verdict">
          <div class="no${no ? "" : " maybe"}${ctx.still ? " still" : ""}" aria-hidden="true">${no ? "No." : "Hmm."}</div>
          <span class="sr-only">${no ? "No." : "Not yet."} Your tier is ${tierName(ctx.meta, s.tier)}.</span>
          <div class="because">${caughtLine(s)}</div>
        </div>
        <p class="tier-line"><span class="tier-label"><span>Tier</span>${tierName(ctx.meta, s.tier)}</span><span class="hand-note">${progress}</span></p>
        <div class="share"><button data-action="open-share">Share your score</button><button class="ghost" data-action="view" data-id="catches">See catches</button></div>
      </article>
      ${briefsSummaryView(s.briefs, ctx.meta, ctx.briefsStill ?? false)}
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
      <div>${verdictView(s, ctx)}</div>
      <div class="meter">
        ${ladderView(s, meta)}
        ${s.next_tier ? html`<p class="hand-note">${s.next_tier[1]} more this week for ${tierName(meta, s.next_tier[0])}</p>` : ""}
        <div class="share"><button data-action="open-share">Share your score</button></div>
      </div>
    </section>
    <section><div class="head"><h2>Catches per week</h2></div><div class="chart">${chartView(s, meta)}</div></section>
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

function hallSection(s: Stats, ctx: ScorecardContext): Html {
  return html`<section><div class="head"><h2>Hall of fame · ${subjectButton(ctx)}</h2><p class="muted">Snippets are redacted and kept per your privacy setting.</p></div>${hallView(s.hall_of_fame)}</section>`;
}

export function scorecardView(s: Stats, ctx: ScorecardContext): Html {
  const { meta, filter } = ctx;
  const view = ctx.view ?? "overview";
  const toolbar = isFiltered(filter)
    ? html`<button class="chip-clear" data-action="clear-filter">Showing ${filterLabel(filter, meta)} only <span aria-hidden="true">×</span></button>`
    : html`<span class="eyebrow">All models · pick one from “All AI”</span>`;
  const body =
    view === "catches"
      ? catchesView(s, ctx)
      : view === "briefs"
        ? briefsView(s.briefs, meta, ctx.briefsStill ?? false, subjectButton(ctx))
        : view === "hall"
          ? hallSection(s, ctx)
          : overviewView(s, ctx);
  return html`
    <section class="toolbar">${toolbar}<span class="muted small">${s.first_day ?? ""} – ${s.last_day ?? ""} · ${num(s.messages)} messages</span></section>
    ${viewsNav(view)}
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
