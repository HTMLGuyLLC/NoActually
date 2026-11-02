// Improve: are you getting better? Last week at a glance, the one goal you're working on, how your
// numbers trend, your personal bests and trophies, and what your own history says works.
import type {
  Goal,
  GoalKind,
  GoalStatus,
  GoalWeek,
  HistoryWeek,
  Meta,
  Progress,
  RecentReportCard,
  ReportCardTool,
  ReportCardWeek,
  SourceInfo,
  Stats,
} from "../api";
import { fmtDay, last, money, plural } from "../format";
import { type Html, html, num, raw } from "../html";
import { lessonFor } from "../learn/lessons";
import { floorPercent, habitTitle, signalNoun, sourceName, steerTitle, tierName, tierOf } from "../meta";
import { TOOL_WORDS } from "../report-cards";
import { RANGES, type Range } from "../state";
import { chartBox, lineChart, type Point, sparkline, stackChart } from "./charts";
import { needMoreNote } from "./prompts";

/** `filtered`: a model is picked, so charts follow it while the goal and bests stay across all AI. */
export type ImproveContext = {
  meta: Meta;
  stats: Stats;
  progress: Progress;
  range: Range;
  subject: string;
  /** The model picker, beside the range. */
  picker?: Html;
  filtered: boolean;
  /** The latest report cards, newest first. */
  recent: RecentReportCard[];
  /** A source that can carry a report card is allowed. */
  canGrade: boolean;
  /** The "get a report card" line was dismissed. */
  hintDismissed: boolean;
  /** Sources exist but none is allowed yet: the empty state then leads to Settings → Sources. */
  noSource?: boolean;
  /** Only an imported export is allowed: no new chats will come, so goals can't be started. */
  importOnly?: boolean;
  /** A tool report cards are already on in, if any: the line then says how to ask instead of how to set up. */
  cardsOn: ReportCardTool | null;
};

/** Whether a source that can carry a report card is allowed, so the empty state may suggest one. */
export const canGrade = (meta: Meta, sources: SourceInfo[]): boolean =>
  sources.some((s) => s.state?.allowed && meta.sources.some((m) => m.id === s.id && m.gradable));

/** Fewer messages than this in a week and its correction rate is too noisy to put on the line. */
const RATE_MIN_MESSAGES = 50;

/** Letter grades, best first. */
const LETTERS = ["A", "B", "C", "D", "F"];
/** Report card grades step from red (A) to ink (F) in five even lightness steps, each clear against the chart (see `--grade-*`). */
const LETTER_SEG: Record<string, string> = { A: "ga", B: "gb", C: "gc", D: "gd", F: "gf" };

/** Vague is muted: blue means the AI. */
const GRADE_SEG: Record<string, string> = { sharp: "s0", solid: "s1", thin: "s3" };
/** Follow-up reasons: blue is the AI's side. Yours get red, ink and muted so they can be told apart (Prompting draws all of them red). */
const STEER_SEG: Record<string, string> = { ai_wrong: "s2", added: "s0", clarified: "s1", redo: "s3" };

/** The clear-the-first-time chart starts at 50% so the tier bands (70/80/90) have room, lower when a week was lower. */
const landedFloor = (points: Point[]) => Math.min(50, ...points.filter((p) => p.value != null && !p.weak).map((p) => Math.floor((p.value ?? 0) / 10) * 10));

/** Half-width charts draw on a narrower canvas, so their text stays as big as the full-width ones'. */
const HALF = 460;

const RANGE_TITLES: Record<Range, string> = { "12w": "12 weeks", "6m": "6 months", all: "All time" };
const RANGE_WEEKS: Record<Range, number> = { "12w": 12, "6m": 26, all: Number.POSITIVE_INFINITY };

/** One point on the charts: a week, or a month when weeks are too sparse to read. */
export type Bucket = Omit<HistoryWeek, "week" | "tips"> & { label: string };

const add = (a: Record<string, number>, b: Record<string, number>) => {
  const out = { ...a };
  for (const [k, v] of Object.entries(b)) out[k] = (out[k] ?? 0) + v;
  return out;
};

/** Two buckets of report cards as one. Cost stays unknown only while neither has a price; the priced tasks add up too. */
const addCards = (a: ReportCardWeek, b: ReportCardWeek): ReportCardWeek => ({
  graded: a.graded + b.graded,
  grades: add(a.grades, b.grades),
  avoidable: a.avoidable + b.avoidable,
  extra_tokens: a.extra_tokens + b.extra_tokens,
  extra_cost: a.extra_cost == null && b.extra_cost == null ? null : (a.extra_cost ?? 0) + (b.extra_cost ?? 0),
  priced: a.priced + b.priced,
  extra_minutes: a.extra_minutes + b.extra_minutes,
  missing: add(a.missing, b.missing),
  gradable_tasks: a.gradable_tasks + b.gradable_tasks,
});

/**
 * The range's weeks as chart points. When a typical week has fewer than `minWeek` tasks, weeks
 * are summed into months instead, so the lines show a trend rather than noise.
 */
export function buckets(weeks: HistoryWeek[], range: Range, minWeek: number): { points: Bucket[]; monthly: boolean } {
  const shown = weeks.slice(-Math.min(weeks.length, RANGE_WEEKS[range]));
  const sorted = shown.map((w) => w.tasks).sort((a, b) => a - b);
  const median = sorted[Math.floor(sorted.length / 2)] ?? 0;
  const monthly = shown.length > 8 && median < minWeek;
  if (!monthly) {
    return { points: shown.map(({ week, ...w }) => ({ ...w, label: fmtDay(week) })), monthly };
  }
  const months = new Map<string, Bucket>();
  for (const { week, ...w } of shown) {
    const key = week.slice(0, 7);
    const m = months.get(key);
    months.set(
      key,
      m
        ? {
            label: m.label,
            messages: m.messages + w.messages,
            corrections: m.corrections + w.corrections,
            humbled: m.humbled + w.humbled,
            tasks: m.tasks + w.tasks,
            continued: m.continued + w.continued,
            landed: m.landed + w.landed,
            signals: add(m.signals, w.signals),
            grades: add(m.grades, w.grades),
            steering: add(m.steering, w.steering),
            steered: add(m.steered, w.steered),
            report_cards: addCards(m.report_cards, w.report_cards),
          }
        : { ...w, label: fmtDay(`${key}-01`, { month: "short", year: "2-digit" }) },
    );
  }
  return { points: [...months.values()], monthly };
}

const pctOf = (n: number, total: number) => (total > 0 ? floorPercent(n, total) : null);

/** One tenth: "0.8". */
const tenth = (v: number) => String(Math.round(v * 10) / 10);

/** What a week's number of avoidable rounds per task is called, as the CLI words it: "0.6 follow-ups that redid work per task", "1 follow-up…". */
const redidPerTask = (v: number) => `${tenth(v)} ${tenth(v) === "1" ? "follow-up" : "follow-ups"} that redid work per task`;

/** Each follow-up kind as a verb phrase (for "…in 15% of tasks or fewer") and as an "-ing" phrase (for "without …"). */
const STEER_PHRASE: Record<string, [string, string]> = {
  ai_wrong: ["Have the AI get it wrong", "the AI getting it wrong"],
  added: ["Add more after sending", "adding more after sending"],
  clarified: ["Clarify after sending", "clarifying after sending"],
  redo: ["Start over", "starting over"],
};

/** What a goal asks, in a sentence. */
export function goalTitle(g: GoalKind & { target: number }, meta: Meta): string {
  if (g.kind === "habit") {
    return `Include ${signalNoun(meta, g.signal)} in ${g.target}% of your prompts each week`;
  }
  if (g.kind === "steer") {
    const [verb, ing] = STEER_PHRASE[g.steer] ?? [`Have “${steerTitle(meta, g.steer).toLowerCase()}”`, `“${steerTitle(meta, g.steer).toLowerCase()}”`];
    return g.target ? `${verb} in ${g.target}% of tasks or fewer each week` : `Go a whole week without ${ing}`;
  }
  return `Reach ${tierName(meta, g.tier)}`;
}

const WEEK_MARK: Record<GoalWeek["state"], string> = { met: "✓", missed: "✗", paused: "–", open: "•" };
const WEEK_WORD: Record<GoalWeek["state"], string> = { met: "met", missed: "missed", paused: "too few tasks", open: "in progress" };

function goalWeeksView(st: GoalStatus, meta: Meta): Html {
  const unit = st.goal.kind === "habit" ? "prompts" : "tasks";
  return html`<ol class="goal-weeks" aria-label="Weeks so far">${st.weeks.map(
    (w) =>
      html`<li class="gw ${w.state}" tabindex="0" data-tip="Week of ${fmtDay(w.week)}: ${w.value}% of ${w.count} ${unit} (${WEEK_WORD[w.state]})"><span aria-hidden="true">${WEEK_MARK[w.state]}</span><span class="sr-only">Week of ${fmtDay(w.week)}: ${w.value}% of ${w.count} ${unit}, ${WEEK_WORD[w.state]}.</span></li>`,
  )}</ol>
  <p class="muted small">Weeks with fewer than ${meta.min_week} ${unit} pause the goal instead of breaking the streak.</p>`;
}

/** This week's progress in words. */
function thisWeekLine(st: GoalStatus, meta: Meta, importOnly: boolean): string {
  const g = st.goal;
  // A tier goal's meter already says "87 of 90 to reach …"; only an unscored one needs words.
  if (g.kind === "tier") {
    if (st.score != null) return "";
    return importOnly ? `Not scored yet. ${needMoreNote(0, "a score", { end: null })}.` : "Not scored yet: you need more tasks with more than one message.";
  }
  const w = last(st.weeks);
  if (w?.state !== "open") return "";
  // The meter above already shows the percent; this says how many and what's left.
  const so = `${plural(w.count, g.kind === "habit" ? "prompt" : "task")} so far.`;
  if (g.kind === "habit") {
    if (st.need == null) return `${so} Out of reach this week; next week starts fresh.`;
    return st.need ? `${so} ${plural(st.need, "more prompt", "more prompts")} with ${signalNoun(meta, g.signal)} would hit it.` : `${so} On track.`;
  }
  return w.count >= meta.min_week && w.value <= g.target ? `${so} On track.` : so;
}

/** Said in place of the goal picker when only an export is imported: a goal is measured on new chats, which won't come. */
const IMPORT_ONLY_GOALS = "Goals need a live AI tool. They are measured week by week as new chats come in, and an imported export doesn't change.";

function goalPickerView(p: Progress, meta: Meta, current: Goal | null, importOnly: boolean): Html {
  if (importOnly)
    return html`<div class="goal-pick"><p class="muted">${IMPORT_ONLY_GOALS}</p>${current ? html`<div class="share"><button class="ghost" data-action="clear-goal">Drop this goal</button></div>` : ""}</div>`;
  const options = p.options;
  const s = p.suggestion;
  const same = (a: GoalKind, b: GoalKind) => JSON.stringify(kindArgs(a)) === JSON.stringify(kindArgs(b));
  const suggested = options.findIndex((o) => same(o, s));
  return html`<div class="goal-pick">
    ${current ? "" : html`<p><span class="hand">Try this:</span> <b>${goalTitle(s, meta)}</b>.</p><div class="share"><button data-action="start-goal" data-goal="${goalValue(s)}">Start this goal</button></div>`}
    <label class="pick-label">${current ? "Switch to" : "Or pick another"}
      <select id="goal-pick">${options.map((o, i) => html`<option value="${goalValue(o)}"${i === suggested ? raw(" selected") : ""}>${goalTitle(o, meta)}</option>`)}</select>
    </label>
    <div class="share"><button class="ghost" data-action="start-goal"${current ? raw(' data-replaces="yes"') : ""}>${current ? "Switch" : "Start it"}</button>${current ? html`<button class="ghost" data-action="clear-goal">Drop this goal</button>` : ""}</div>
  </div>`;
}

const kindArgs = (k: GoalKind): Record<string, string> =>
  k.kind === "habit" ? { kind: k.kind, signal: k.signal } : k.kind === "steer" ? { kind: k.kind, steer: k.steer } : { kind: k.kind, tier: k.tier };

/** A goal choice as a form value: its kind and target, read back by the start-goal action. */
const goalValue = (g: GoalKind & { target: number }) => JSON.stringify({ ...kindArgs(g), target: g.target });

/** The one number to watch: your score against the tier, or this week against the target. */
function goalMeter(st: GoalStatus, meta: Meta): Html | "" {
  const g = st.goal;
  if (g.kind === "tier") {
    const floor = tierOf(meta, g.tier)?.floor ?? 100;
    if (st.score == null) return "";
    return html`<div class="goal-meter"><span class="big">${st.score}</span><span class="of">of ${floor} to reach ${tierName(meta, g.tier)}</span>
      <span class="bar-bg" role="img" aria-label="Score ${st.score} of ${floor}"><i data-w="${Math.min(100, Math.round((100 * st.score) / floor))}"></i></span></div>`;
  }
  const w = last(st.weeks);
  if (w?.state !== "open") return "";
  const word = g.kind === "habit" ? "at least" : "at most";
  return html`<div class="goal-meter"><span class="big">${w.value}%</span><span class="of">this week · aim for ${word} ${g.target}%</span></div>`;
}

function goalView(p: Progress, meta: Meta, filtered: boolean, importOnly: boolean): Html {
  const st = p.goal;
  const across = filtered ? " · all AI" : "";
  if (!st) {
    return html`<section class="card mini goal"><div class="eyebrow">Your goal${across}</div><h2 class="q">Pick one thing to get better at</h2>
      ${importOnly ? "" : html`<p class="muted">One goal at a time, measured each week from your own history. Hit it ${meta.goal_weeks} weeks to win a trophy.</p>`}
      ${goalPickerView(p, meta, null, importOnly)}</section>`;
  }
  const g = st.goal;
  const progressLine =
    g.kind === "tier"
      ? ""
      : html`<p class="goal-count"><b>${st.met}</b> of ${st.needed} weeks met${st.streak > 1 ? html` · <span class="hand">${st.streak} in a row</span>` : ""}</p>`;
  const line = st.done ? "It's on your trophy shelf. Pick the next one." : thisWeekLine(st, meta, importOnly);
  return html`<section class="card mini goal${st.done ? " finished" : ""}">
    <div class="eyebrow">${st.done ? "Goal done 🏆" : `Your goal · since ${fmtDay(g.start)}`}${across}</div>
    <h2 class="goal-title">${goalTitle(g, meta)}</h2>
    ${st.done ? "" : goalMeter(st, meta)}
    ${progressLine}
    ${g.kind === "tier" ? "" : goalWeeksView(st, meta)}
    ${line ? html`<p>${line}</p>` : ""}
    <details${st.done ? raw(" open") : ""}><summary>${st.done ? "Next goal" : "Change goal"}</summary>${goalPickerView(p, meta, g, importOnly)}</details>
  </section>`;
}

/** Graded tasks that got an A or B. */
const goodOf = (c: ReportCardWeek) => (c.grades.A ?? 0) + (c.grades.B ?? 0);

/** What the extra back-and-forth cost per task ("$0.15"), over the tasks that have a price; null when none do or it rounds to nothing. */
function costPerTask(c: ReportCardWeek): string | null {
  const each = c.extra_cost != null && c.priced ? c.extra_cost / c.priced : 0;
  return each >= 0.005 ? money(each) : null;
}

/** Whole minutes of redone work per graded task; every card has minutes, priced or not. */
const minutesPerTask = (c: ReportCardWeek): number => (c.graded ? Math.round(c.extra_minutes / c.graded) : 0);

/** Said when follow-ups redid work but it came to no whole minute: that is not the same as nothing redone. */
const UNDER_A_MINUTE = "under a minute extra";

/** What a week's redone work took per graded task: about so many minutes, or under a minute if follow-ups redid work and it rounds to none. Empty when nothing was redone. */
function minutesWords(c: ReportCardWeek): string {
  const minutes = minutesPerTask(c);
  if (minutes) return `about ${plural(minutes, "extra minute")} per task`;
  return c.avoidable > 0 ? `${UNDER_A_MINUTE} per task` : "";
}

/** Dollar figures are what the work would cost per token, not what a subscription charges. */
const PAYG = "at pay-as-you-go prices";
/** Said once under the recent cards, in place of on every row, when any of them shows dollars. */
const DOLLARS_NOTE = `Dollars are ${PAYG}. On a subscription this came out of your usage limit, not your wallet.`;

/** "Report cards: 4 of 6 tasks A or B (up from 2 of 5) · about 2 extra minutes per task." Only when both weeks have enough graded tasks. */
function cardLine(latest: ReportCardWeek, before: ReportCardWeek, meta: Meta): string {
  if (latest.graded < meta.min_card_week || before.graded < meta.min_card_week) return "";
  const [now, then] = [goodOf(latest), goodOf(before)];
  // Shares, not counts: 3 of 6 is the same as 2 of 4.
  const diff = now * before.graded - then * latest.graded;
  const change = diff > 0 ? "up from" : diff < 0 ? "down from" : "same as";
  const minutes = minutesWords(latest);
  return `Report cards: ${now} of ${latest.graded} tasks A or B (${change} ${then} of ${before.graded})${minutes ? ` · ${minutes}` : ""}.`;
}

/** "Last week, 6 of 40 tasks got a tip (down from 9 of 38 the week before)." and the tip you get most, each only when there's enough to say. */
function tipsView(p: Progress, meta: Meta): Html | "" {
  const { tip_trend: trend, top_tip: top } = p.insights;
  const lines: string[] = [];
  if (trend) {
    const { latest: now, before: then } = trend;
    // Shares, not counts: 3 of 30 is the same as 2 of 20.
    const diff = now.tips * then.tasks - then.tips * now.tasks;
    const change = diff > 0 ? "up from" : diff < 0 ? "down from" : "same as";
    lines.push(`Last week, ${now.tips} of ${now.tasks} tasks got a tip (${change} ${then.tips} of ${then.tasks} the week before).`);
  }
  const plain = meta.habits.find((h) => h.id === top?.habit)?.plain;
  if (top && plain) lines.push(`Your most common tip: ${plain} (${top.tips} of your last ${top.of}).`);
  return lines.length ? html`<div class="tip-lines">${lines.map((l) => html`<p>${l}</p>`)}</div>` : "";
}

/** Last full week against the week before: what moved. */
function recapView(weeks: HistoryWeek[], meta: Meta): Html | "" {
  const done = weeks.slice(0, -1); // the last one is this week, still under way
  const latest = last(done);
  const before = last(done, 2);
  if (!latest || !before) return "";
  const cards = cardLine(latest.report_cards, before.report_cards, meta);
  const rows = latest.tasks >= meta.min_week;
  if (!rows && !cards) return "";
  const landed = (w: HistoryWeek) => pctOf(w.landed, w.continued);
  const sharp = (w: HistoryWeek) => pctOf(w.grades.sharp ?? 0, w.tasks);
  // Fewer corrections isn't worse (it may be getting things right), so that row's change isn't colored.
  const row = (name: string, a: number | null, b: number | null, unit = "%", neutral = false) => {
    const diff = a != null && b != null ? a - b : null;
    const arrow =
      diff == null || diff === 0
        ? ""
        : html`<span aria-hidden="true">${diff > 0 ? "▲" : "▼"}</span><span class="sr-only">${diff > 0 ? "up" : "down"} </span> ${Math.abs(diff)}`;
    return html`<div class="recap-row"><span>${name}</span><b>${a ?? "–"}${a != null ? unit : ""}</b><span class="delta${neutral || !diff ? "" : diff > 0 ? " up" : " down"}">${arrow}</span></div>`;
  };
  return html`<section class="recap"><div class="eyebrow">Last week · ${fmtDay(latest.week)}</div>
    ${
      rows
        ? html`${row("Clear the first time", landed(latest), landed(before))}
    ${row("Specific prompts", sharp(latest), sharp(before))}
    ${row("Corrections", latest.corrections, before.corrections, "", true)}`
        : ""
    }
    ${cards ? html`<p class="recap-cards">${cards}</p>` : ""}
    <p class="muted small">Compared with the week before.</p>
  </section>`;
}

function trendsView(ctx: ImproveContext): Html {
  const { meta, progress: p, range } = ctx;
  const { points, monthly } = buckets(p.weeks, range, meta.min_week);
  const per = monthly ? "month" : "week";
  const weak = (n: number) => n < meta.min_week;
  const landed: Point[] = points.map((b) => ({ label: b.label, value: pctOf(b.landed, b.continued), weak: weak(b.continued) }));
  const strong = landed.filter((x) => x.value != null && !x.weak);
  const firstStrong = strong[0];
  const lastStrong = last(strong);
  const landedSummary =
    firstStrong && lastStrong
      ? `Clear the first time went from ${firstStrong.value}% (${firstStrong.label}) to ${lastStrong.value}% (${lastStrong.label}).`
      : "Not enough tasks yet to show a trend.";
  const bands = meta.prompt_tiers.slice(1).map((t) => ({ from: t.floor, name: t.name }));
  const signals = meta.signals.map((s) => {
    const pts = points.map((b) => ({ label: b.label, value: pctOf(b.signals[s.id] ?? 0, b.tasks), weak: weak(b.tasks) }));
    const now = last(pts.filter((x) => x.value != null && !x.weak))?.value;
    return html`<div class="cat sig"><span>${s.title}<small class="muted">${s.hint}</small></span>${sparkline(pts, `${s.title} per ${per}`)}<span class="n">${now != null ? `${now}%` : "–"}</span></div>`;
  });
  const labels = points.map((b) => b.label);
  // The AI's mistakes in its blue; the ones on you in red, ink and muted so each reason stands apart.
  const steerSeries = meta.steers.map((s) => ({ title: s.title, values: points.map((b) => b.steering[s.id] ?? 0), seg: STEER_SEG[s.id] }));
  const gradeSeries = [...meta.grades].reverse().map((g) => ({ title: g.title, values: points.map((b) => b.grades[g.id] ?? 0), seg: GRADE_SEG[g.id] }));
  const rate: Point[] = points.map((b) => ({
    label: b.label,
    value: b.messages ? (100 * b.corrections) / b.messages : null,
    weak: b.messages < RATE_MIN_MESSAGES,
  }));
  const rateMax = Math.max(5, ...rate.map((r) => Math.ceil(r.value ?? 0)));
  const latest = (series: { title: string; values: number[] }[]) =>
    `Latest ${per} (${last(labels) ?? ""}): ${series.map((s) => `${s.title} ${last(s.values) ?? 0}`).join(", ")}.`;
  const sum = (xs: number[]) => xs.reduce((a, v) => a + v, 0);
  const rateNow = last(rate.filter((r) => r.value != null));
  const messages = sum(points.map((b) => b.messages));
  const average = messages ? (100 * sum(points.map((b) => b.corrections))) / messages : null;
  const rateSummary =
    rateNow?.value != null && average != null
      ? `Corrections per 100 messages: ${tenth(rateNow.value)} in the latest ${per} (${rateNow.label}), ${tenth(average)} on average.`
      : "Not enough messages yet to show a rate.";
  const totalSteer = steerSeries.reduce((t, s) => t + s.values.reduce((a, v) => a + v, 0), 0);
  const reportCards = cardsView(ctx, points, per);
  return html`
    <section><div class="head"><h2>Prompts clear the first time</h2><p class="muted">Per ${per}. Hollow dots had fewer than ${meta.min_week} tasks with more than one message, so they're left off the line.</p></div>
      ${chartBox("Prompts clear the first time chart, scrolls sideways", lineChart({ id: "landed", label: `Percent of prompts clear the first time, per ${per}`, summary: landedSummary, points: landed, bands, unit: "%", min: landedFloor(landed) }))}</section>
    <section class="split">
      <div><div class="head"><h2>What your prompts include</h2></div><div class="cats">${signals}</div></div>
      <div><div class="head"><h2>Prompt grades</h2></div>${chartBox("Prompt grades chart, scrolls sideways", stackChart({ id: "grades", label: `Prompts by grade, per ${per}`, summary: `Prompts by grade over ${points.length} ${per}s. ${latest(gradeSeries)}`, labels, series: gradeSeries, width: HALF, height: 220 }))}</div>
    </section>
    <section class="split">
      <div><div class="head"><h2>Why you followed up</h2></div>${chartBox("Why you followed up chart, scrolls sideways", stackChart({ id: "steering", label: `Follow-ups by reason, per ${per}`, summary: `${plural(totalSteer, "follow-up")} over ${points.length} ${per}s. ${latest(steerSeries)}`, labels, series: steerSeries, width: HALF, height: 220 }))}</div>
      <div><div class="head"><h2>Corrections per 100 messages</h2><p class="muted">Hollow dots had fewer than ${RATE_MIN_MESSAGES} messages, so they're left off the line.</p></div>${chartBox("Corrections per 100 messages chart, scrolls sideways", lineChart({ id: "rate", label: `Corrections per 100 messages, per ${per}`, summary: rateSummary, points: rate, max: rateMax, width: HALF, height: 220, weakNote: `fewer than ${RATE_MIN_MESSAGES} messages` }))}</div>
    </section>
    ${reportCards}`;
}

/** A recent card shows dollars when its model has a price and the redone work came to a cent or two. */
const hasDollars = (c: RecentReportCard): boolean => c.extra_cost != null && c.extra_cost >= 0.005;

/**
 * What a recent card's redone work took: minutes first, and dollars only when the model has a price.
 * A card has no count of follow-ups, but work redone shows as tokens: some redone with no whole minute is "under a minute", not nothing.
 */
function extraView(c: RecentReportCard): Html {
  const minutes = c.extra_minutes > 0 ? `about ${plural(c.extra_minutes, "extra minute")}` : c.extra_tokens > 0 ? UNDER_A_MINUTE : "";
  const dollars = c.extra_cost != null && hasDollars(c) ? `about ${money(c.extra_cost)}` : "";
  if (!minutes && !dollars) return html`<span class="muted">Nothing redone</span>`;
  return html`${minutes ? html`<span class="muted">${minutes}</span>` : ""}${dollars ? html`<span class="muted">${dollars}</span>` : ""}`;
}

/** The last few report cards, one line each: when it was graded, where, grade, what was left out, what it took. Dollars are explained once, below. */
function recentView(recent: RecentReportCard[], meta: Meta): Html {
  const list = html`<ul class="recent-cards">${recent.map(
    (c) =>
      html`<li><span class="rc-day">${fmtDay(c.graded_day)}</span><span>${sourceName(meta, c.source)}</span><span>Grade <b>${c.grade}</b></span>${c.missing ? html`<span>Left out: ${habitTitle(meta, c.missing).toLowerCase()}</span>` : html`<span class="muted">Nothing left out</span>`}${extraView(c)}</li>`,
  )}</ul>`;
  return recent.some(hasDollars) ? html`${list}<p class="muted small">${DOLLARS_NOTE}</p>` : list;
}

/** "Your report cards": the grades over time, the follow-ups that redid work per graded task, the tip lines, the latest cards, and how much they cover. With no graded task in the range, the tip lines alone, if there are any. */
function cardsView(ctx: ImproveContext, points: Bucket[], per: string): Html | "" {
  const { meta, recent } = ctx;
  const sum = (f: (c: ReportCardWeek) => number) => points.reduce((t, b) => t + f(b.report_cards), 0);
  const graded = sum((c) => c.graded);
  const tips = tipsView(ctx.progress, meta);
  if (!graded) return tips && html`<section class="cards-block"><div class="head"><h2>Your tips</h2></div>${tips}</section>`;
  const labels = points.map((b) => b.label);
  const gradeSeries = LETTERS.map((g) => ({ title: g, values: points.map((b) => b.report_cards.grades[g] ?? 0), seg: LETTER_SEG[g] }));
  const rate = (c: ReportCardWeek) => c.avoidable / c.graded;
  /** What a week's follow-ups that redid work took per task: minutes always, dollars only where there's a price. */
  const tookPerTask = (c: ReportCardWeek) => {
    const cost = costPerTask(c);
    return [minutesWords(c), cost ? `about ${cost} per task ${PAYG}` : ""].filter(Boolean);
  };
  const rounds: Point[] = points.map((b) => {
    const c = b.report_cards;
    return {
      label: b.label,
      value: c.graded ? rate(c) : null,
      weak: c.graded < meta.min_card_week,
      tip: c.graded ? [`${b.label}: ${redidPerTask(rate(c))}`, plural(c.graded, "report card"), ...tookPerTask(c)].join(" · ") : "",
    };
  });
  const solid = points.filter((b) => b.report_cards.graded >= meta.min_card_week);
  const [from, to] = [solid[0], last(solid)];
  const roundsSummary =
    from && to
      ? `Follow-ups that redid work per graded task went from ${tenth(rate(from.report_cards))} (${from.label}) to ${tenth(rate(to.report_cards))} (${to.label}).${tookPerTask(
          to.report_cards,
        )
          .map((t) => ` ${t[0]?.toUpperCase()}${t.slice(1)}.`)
          .join("")}`
      : "Not enough report cards yet to show a trend.";
  const latest = `Latest ${per} (${last(labels) ?? ""}): ${gradeSeries.map((s) => `${s.title} ${last(s.values) ?? 0}`).join(", ")}.`;
  const roundsMax = Math.max(1, ...rounds.map((r) => Math.ceil(r.value ?? 0)));
  const gradable = Math.max(
    graded,
    sum((c) => c.gradable_tasks),
  );
  return html`<section class="cards-block">
    <div class="head"><h2>Your report cards</h2><p class="muted">A means nothing had to be redone. A letter grade looks at the whole task; a prompt grade looks at your first message only.</p></div>
    <div class="split">
      <div><div class="head"><h3>Report card grades</h3></div>${chartBox("Report card grades chart, scrolls sideways", stackChart({ id: "cards", label: `Report cards by letter grade, per ${per}`, summary: `${plural(graded, "report card")} over ${points.length} ${per}s. ${latest}`, labels, series: gradeSeries, width: HALF, height: 220 }))}</div>
      <div><div class="head"><h3>Follow-ups that redid work, per task</h3><p class="muted">Per ${per}. Hollow dots had fewer than ${meta.min_card_week} report cards, so they're left off the line.</p></div>${chartBox("Follow-ups that redid work, per task, chart, scrolls sideways", lineChart({ id: "card-rounds", label: `Follow-ups that redid work, per task, per ${per}`, summary: roundsSummary, points: rounds, max: roundsMax, width: HALF, height: 220, weakNote: `fewer than ${meta.min_card_week} report cards` }))}</div>
    </div>
    ${tips}
    ${recent.length ? html`<div><div class="head"><h3>Recent report cards</h3></div>${recentView(recent, meta)}</div>` : ""}
    <p class="muted small">From ${num(graded)} of ${num(gradable)} tasks in tools that support report cards. You choose which tasks to grade, so these may not be typical.</p>
  </section>`;
}

/** Personal bests are across all AI; with a model picked, they say so. */
function bestsView(p: Progress, filtered: boolean): Html {
  const b = p.bests;
  const stat = (v: string, l: string) => html`<div class="stat"><span class="v">${v}</span><span class="l">${l}${filtered ? " · all AI" : ""}</span></div>`;
  return html`<section class="stats">
    ${stat(b.landed ? `${b.landed[0]}%` : "–", b.landed ? `best 4 weeks clear the first time · to ${fmtDay(b.landed[1])}` : "best 4 weeks clear the first time")}
    ${stat(b.sharp ? `${b.sharp[0]}%` : "–", b.sharp ? `most specific week · ${fmtDay(b.sharp[1])}` : "most specific week")}
    ${stat(plural(b.goal_streak, "week"), "longest goal streak")}
    ${stat(`${p.trophies.length}`, p.trophies.length === 1 ? "trophy" : "trophies")}
  </section>`;
}

function trophiesView(p: Progress, meta: Meta): Html | "" {
  if (!p.trophies.length) return "";
  return html`<section><div class="head"><h2>Trophy shelf</h2></div><ul class="trophies">${p.trophies.map(
    (t) =>
      html`<li><span aria-hidden="true">🏆</span> <b>${goalTitle(t.goal, meta)}</b> <span class="muted small">${fmtDay(t.done, { month: "short", year: "numeric" })}</span></li>`,
  )}</ul></section>`;
}

function insightsView(p: Progress, meta: Meta): Html {
  const i = p.insights;
  const items: Html[] = [];
  for (const t of i.signals) {
    if (!t.landed_with_without) continue;
    const [w, wo] = t.landed_with_without;
    const l = lessonFor("signal", t.signal);
    items.push(
      html`<li>Prompts with ${signalNoun(meta, t.signal)} were clear the first time <b>${Math.round(w)}%</b> of the time, vs ${Math.round(wo)}% without.${l ? html` <button class="link" data-action="open-lesson" data-id="${l.id}">${l.title} →</button>` : ""}</li>`,
    );
  }
  if (i.steer) {
    const [id, share] = i.steer;
    const l = lessonFor("steer", id);
    items.push(
      html`<li>Your most common follow-up is <b>“${steerTitle(meta, id)}”</b> (${share}%).${l ? html` <button class="link" data-action="open-lesson" data-id="${l.id}">${l.title} →</button>` : ""}</li>`,
    );
  }
  const rc = i.report_cards;
  if (rc) {
    items.push(html`<li>Report cards A or B: <b>${rc.first_ab}% → ${rc.last_ab}%</b> since you started.</li>`);
    if (rc.habit)
      items.push(
        html`<li>You used to leave out the ${habitTitle(meta, rc.habit.habit).toLowerCase()} in <b>${rc.habit.first} of ${meta.min_card_half}</b> graded tasks. Now ${rc.habit.last} of ${meta.min_card_half}.</li>`,
      );
  }
  const [best, worst] = i.models;
  if (best && worst)
    items.push(
      html`<li>Your prompts are clear the first time <b>${best.landed}%</b> of the time with ${best.family}, and ${worst.landed}% with ${worst.family}.</li>`,
    );
  return html`<section><div class="head"><h2>What works for you</h2><p class="muted">From your own history, and only when there's enough of it to say.</p></div>
    ${items.length ? html`<ul class="insights">${items}</ul>` : html`<p class="muted">Not enough history yet. Keep going; these show up as your tasks add up.</p>`}</section>`;
}

/** Until you have a report card: one line on how to get one, that you can dismiss. Not while a model is picked, since the history shown is only that model's. */
function cardHintView(ctx: ImproveContext): Html | "" {
  if (!ctx.canGrade || ctx.hintDismissed || ctx.filtered || ctx.progress.weeks.some((w) => w.report_cards.graded > 0)) return "";
  const ask = ctx.cardsOn
    ? html`Type ${TOOL_WORDS[ctx.cardsOn].command} after your next task.`
    : html`Get a letter grade on how you asked, in Claude Code or Codex. <button class="link" data-action="go-settings" data-id="report-cards">Set up</button>`;
  return html`<p class="card-hint">${ask}<button class="ghost small" data-action="dismiss-card-hint" aria-label="Dismiss this tip about report cards"><span aria-hidden="true">×</span></button></p>`;
}

function rangeNav(range: Range): Html {
  return html`<nav class="views" aria-label="Time range">${RANGES.map(
    (r) =>
      html`<button class="tab${r === range ? " on" : ""}" data-action="range" data-id="${r}"${r === range ? raw(' aria-current="true"') : ""}>${RANGE_TITLES[r]}</button>`,
  )}</nav>`;
}

/** Charts with nothing to draw: with no source allowed, the way to get some leads. */
const emptyTrends = (noSource = false): Html =>
  noSource
    ? html`<p class="muted">Nothing is being counted yet. Pick which AI tools it may read, and your charts start with your first tasks.</p><p><button data-action="go-settings" data-id="sources">Choose sources</button></p>`
    : html`<p class="muted">No history yet. Your charts start with your first tasks.</p>`;

export function improveView(ctx: ImproveContext): Html {
  const { meta, progress: p } = ctx;
  const empty = !p.weeks.length;
  return html`
    <section class="toolbar"><h1 class="eyebrow">Are you getting better?</h1></section>
    <section class="improve-top">${goalView(p, meta, ctx.filtered, !!ctx.importOnly)}${recapView(p.weeks, meta) || html`<section class="recap"><div class="eyebrow">Last week</div><p class="muted small">Your weekly recap shows up once a full week has ${meta.min_week}+ tasks.</p></section>`}</section>
    ${bestsView(p, ctx.filtered)}
    ${trophiesView(p, meta)}
    <div class="head trends-head"><h2 id="trends-heading">How ${ctx.subject} and you are trending</h2><div class="filters">${ctx.picker ?? ""}${rangeNav(ctx.range)}</div></div>
    ${empty ? emptyTrends(ctx.noSource) : html`${cardHintView(ctx)}${trendsView(ctx)}`}
    ${insightsView(p, meta)}`;
}
