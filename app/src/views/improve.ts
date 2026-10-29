// Improve: are you getting better? Last week at a glance, the one goal you're working on, how your
// numbers trend, your personal bests and trophies, and what your own history says works.
import type { Goal, GoalKind, GoalStatus, GoalWeek, HistoryWeek, Meta, Progress, Stats } from "../api";
import { type Html, html, num, raw } from "../html";
import { LESSONS } from "../learn/lessons";
import { signalTitle, steerTitle, tierName } from "../meta";
import { RANGES, type Range } from "../state";
import { lineChart, type Point, sparkline, stackChart } from "./charts";

/** `filtered`: a model is picked, so charts follow it while the goal and bests stay across all AI. */
export type ImproveContext = { meta: Meta; stats: Stats; progress: Progress; range: Range; subject: Html | string; filtered: boolean };

/** Fewer messages than this in a week and its catch rate is too noisy to put on the line. */
const RATE_MIN_MESSAGES = 50;

/** The landed chart starts at 50% so the tier bands (70/80/90) have room, lower when a week was lower. */
const landedFloor = (points: Point[]) => Math.min(50, ...points.filter((p) => p.value != null && !p.weak).map((p) => Math.floor((p.value ?? 0) / 10) * 10));

/** Half-width charts draw on a narrower canvas, so their text stays as big as the full-width ones'. */
const HALF = 460;

const RANGE_TITLES: Record<Range, string> = { "12w": "12 weeks", "6m": "6 months", all: "All time" };
const RANGE_WEEKS: Record<Range, number> = { "12w": 12, "6m": 26, all: Number.POSITIVE_INFINITY };

/** One point on the charts: a week, or a month when weeks are too sparse to read. */
export type Bucket = Omit<HistoryWeek, "week"> & { label: string };

const add = (a: Record<string, number>, b: Record<string, number>) => {
  const out = { ...a };
  for (const [k, v] of Object.entries(b)) out[k] = (out[k] ?? 0) + v;
  return out;
};

const fmtDay = (iso: string, opts: Intl.DateTimeFormatOptions = { month: "short", day: "numeric" }) =>
  new Date(`${iso}T00:00`).toLocaleDateString(undefined, opts);

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
            catches: m.catches + w.catches,
            humbled: m.humbled + w.humbled,
            tasks: m.tasks + w.tasks,
            continued: m.continued + w.continued,
            landed: m.landed + w.landed,
            signals: add(m.signals, w.signals),
            grades: add(m.grades, w.grades),
            steering: add(m.steering, w.steering),
            steered: add(m.steered, w.steered),
          }
        : { ...w, label: fmtDay(`${key}-01`, { month: "short", year: "2-digit" }) },
    );
  }
  return { points: [...months.values()], monthly };
}

const pctOf = (n: number, total: number) => (total > 0 ? Math.floor((100 * n) / total) : null);

/** "Says why" → "say why", for "Briefs that say why landed…". */
const verbPhrase = (title: string) => title.toLowerCase().replace(/^(\w+?)s\b/, "$1");

/** What a goal asks, in a sentence. */
export function goalTitle(g: GoalKind & { target: number }, meta: Meta): string {
  if (g.kind === "habit") {
    const phrase = verbPhrase(signalTitle(meta, g.signal));
    return `${phrase.charAt(0).toUpperCase()}${phrase.slice(1)} in ${g.target}% of your briefs each week`;
  }
  if (g.kind === "steer") {
    const steer = `“${steerTitle(meta, g.steer).toLowerCase()}”`;
    return g.target ? `Keep ${steer} to ${g.target}% of your tasks or fewer each week` : `Go a whole week with no ${steer}`;
  }
  return `Reach ${tierName(meta, g.tier)}`;
}

const WEEK_MARK: Record<GoalWeek["state"], string> = { met: "✓", missed: "✗", paused: "–", open: "•" };
const WEEK_WORD: Record<GoalWeek["state"], string> = { met: "met", missed: "missed", paused: "too few tasks", open: "in progress" };

function goalWeeksView(st: GoalStatus, meta: Meta): Html {
  const unit = st.goal.kind === "habit" ? "briefs" : "tasks";
  return html`<ol class="goal-weeks" aria-label="Weeks so far">${st.weeks.map(
    (w) =>
      html`<li class="gw ${w.state}" title="Week of ${fmtDay(w.week)}: ${w.value}% of ${w.count} ${unit} (${WEEK_WORD[w.state]})"><span aria-hidden="true">${WEEK_MARK[w.state]}</span><span class="sr-only">Week of ${fmtDay(w.week)}: ${w.value}% of ${w.count} ${unit}, ${WEEK_WORD[w.state]}.</span></li>`,
  )}</ol>
  <p class="muted small">Weeks with fewer than ${meta.min_week} ${unit} pause the goal instead of breaking the streak.</p>`;
}

/** This week's progress in words. */
function thisWeekLine(st: GoalStatus, meta: Meta): string {
  const g = st.goal;
  if (g.kind === "tier")
    return st.score == null
      ? "Not scored yet: you need more tasks with a follow-up."
      : `Your score is ${st.score}. ${tierName(meta, g.tier)} starts at ${meta.tiers.find((t) => t.id === g.tier)?.floor ?? "?"}.`;
  const w = st.weeks.at(-1);
  if (w?.state !== "open") return "";
  const unit = g.kind === "habit" ? "briefs" : "tasks";
  const so = `This week: ${w.value}% of ${w.count} ${unit} so far.`;
  if (g.kind === "habit") {
    if (st.need == null) return `${so} Out of reach this week; next week starts fresh.`;
    return st.need
      ? `${so} ${st.need} more ${st.need === 1 ? "brief" : "briefs"} that ${verbPhrase(signalTitle(meta, g.signal))} would hit it.`
      : `${so} On track.`;
  }
  return w.count >= meta.min_week && w.value <= g.target ? `${so} On track.` : so;
}

function goalPickerView(p: Progress, meta: Meta, current: Goal | null): Html {
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

function goalView(p: Progress, meta: Meta, filtered: boolean): Html {
  const st = p.goal;
  const across = filtered ? " · all AI" : "";
  if (!st) {
    return html`<section class="card mini goal"><div class="eyebrow">Your goal${across}</div><h2 class="q">Pick one thing to get better at</h2>
      <p class="muted">One goal at a time, measured each week from your own history. Hit it ${meta.goal_weeks} weeks to win a trophy.</p>
      ${goalPickerView(p, meta, null)}</section>`;
  }
  const g = st.goal;
  const progressLine =
    g.kind === "tier"
      ? ""
      : html`<p class="goal-count"><b>${st.met}</b> of ${st.needed} weeks met${st.streak > 1 ? html` · <span class="hand">${st.streak} in a row</span>` : ""}</p>`;
  return html`<section class="card mini goal${st.done ? " finished" : ""}">
    <div class="eyebrow">${st.done ? "Goal done 🏆" : `Your goal · since ${fmtDay(g.start)}`}${across}</div>
    <h2 class="goal-title">${goalTitle(g, meta)}</h2>
    ${progressLine}
    ${g.kind === "tier" ? "" : goalWeeksView(st, meta)}
    <p>${st.done ? "It's on your trophy shelf. Pick the next one." : thisWeekLine(st, meta)}</p>
    <details${st.done ? raw(" open") : ""}><summary>${st.done ? "Next goal" : "Change goal"}</summary>${goalPickerView(p, meta, g)}</details>
  </section>`;
}

/** Last full week against the week before: what moved. */
function recapView(weeks: HistoryWeek[], meta: Meta): Html | "" {
  const done = weeks.slice(0, -1); // the last one is this week, still under way
  const last = done.at(-1);
  const before = done.at(-2);
  if (!last || !before || last.tasks < meta.min_week) return "";
  const landed = (w: HistoryWeek) => pctOf(w.landed, w.continued);
  const sharp = (w: HistoryWeek) => pctOf(w.grades.sharp ?? 0, w.tasks);
  // Fewer catches isn't worse (it may be getting things right), so that row's change isn't colored.
  const row = (name: string, a: number | null, b: number | null, unit = "%", neutral = false) => {
    const diff = a != null && b != null ? a - b : null;
    const arrow = diff == null || diff === 0 ? "" : diff > 0 ? `▲ ${diff}` : `▼ ${-diff}`;
    return html`<div class="recap-row"><span>${name}</span><b>${a ?? "–"}${a != null ? unit : ""}</b><span class="delta${neutral || !diff ? "" : diff > 0 ? " up" : " down"}">${arrow}</span></div>`;
  };
  return html`<section class="card mini recap"><div class="eyebrow">Last week · ${fmtDay(last.week)}</div>
    ${row("Tasks landed", landed(last), landed(before))}
    ${row("Sharp briefs", sharp(last), sharp(before))}
    ${row("Catches", last.catches, before.catches, "", true)}
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
  const lastStrong = strong.at(-1);
  const landedSummary =
    firstStrong && lastStrong
      ? `Tasks landed went from ${firstStrong.value}% (${firstStrong.label}) to ${lastStrong.value}% (${lastStrong.label}).`
      : "Not enough tasks yet to show a trend.";
  const bands = meta.brief_tiers.slice(1).map((t) => ({ from: t.floor, name: t.name }));
  const signals = meta.signals.map((s) => {
    const pts = points.map((b) => ({ label: b.label, value: pctOf(b.signals[s.id] ?? 0, b.tasks), weak: weak(b.tasks) }));
    const now = pts.filter((x) => x.value != null && !x.weak).at(-1)?.value;
    return html`<div class="cat sig"><span>${s.title}<small class="muted">${s.hint}</small></span>${sparkline(pts, `${s.title} per ${per}`)}<span class="n">${now != null ? `${now}%` : "–"}</span></div>`;
  });
  const labels = points.map((b) => b.label);
  const steerSeries = meta.steers.map((s) => ({ title: s.title, values: points.map((b) => b.steering[s.id] ?? 0) }));
  const gradeSeries = [...meta.grades].reverse().map((g) => ({ title: g.title, values: points.map((b) => b.grades[g.id] ?? 0) }));
  const rate: Point[] = points.map((b) => ({
    label: b.label,
    value: b.messages ? (100 * b.catches) / b.messages : null,
    weak: b.messages < RATE_MIN_MESSAGES,
  }));
  const rateMax = Math.max(5, ...rate.map((r) => Math.ceil(r.value ?? 0)));
  const totalSteer = steerSeries.reduce((t, s) => t + s.values.reduce((a, v) => a + v, 0), 0);
  return html`
    <section><div class="head"><h2>Tasks that landed</h2><p class="muted">Per ${per}. Hollow dots had fewer than ${meta.min_week} tasks with a follow-up, so they're left off the line.</p></div>
      <div class="chart">${lineChart({ id: "landed", label: `Percent of tasks that landed, per ${per}`, summary: landedSummary, points: landed, bands, unit: "%", min: landedFloor(landed) })}</div></section>
    <section class="split">
      <div><div class="head"><h2>What your briefs carry</h2></div><div class="cats">${signals}</div></div>
      <div><div class="head"><h2>Brief grades</h2></div><div class="chart">${stackChart({ id: "grades", label: `Briefs by grade, per ${per}`, summary: `Briefs by grade over ${points.length} ${per}s.`, labels, series: gradeSeries, width: HALF, height: 220 })}</div></div>
    </section>
    <section class="split">
      <div><div class="head"><h2>Where steering went</h2></div><div class="chart">${stackChart({ id: "steering", label: `Steering messages by cause, per ${per}`, summary: `${num(totalSteer)} steering messages over ${points.length} ${per}s.`, labels, series: steerSeries, width: HALF, height: 220 })}</div></div>
      <div><div class="head"><h2>Catches per 100 messages</h2></div><div class="chart">${lineChart({ id: "rate", label: `Catches per 100 messages, per ${per}`, summary: "How often you caught a mistake, per 100 messages.", points: rate, max: rateMax, width: HALF, height: 220 })}</div></div>
    </section>`;
}

/** Personal bests are across all AI; with a model picked, they say so. */
function bestsView(p: Progress, filtered: boolean): Html {
  const b = p.bests;
  const stat = (v: string, l: string) => html`<div class="stat"><span class="v">${v}</span><span class="l">${l}${filtered ? " · all AI" : ""}</span></div>`;
  return html`<section class="stats">
    ${stat(b.landed ? `${b.landed[0]}%` : "–", b.landed ? `best 4 weeks landed · to ${fmtDay(b.landed[1])}` : "best 4 weeks landed")}
    ${stat(b.sharp ? `${b.sharp[0]}%` : "–", b.sharp ? `sharpest week · ${fmtDay(b.sharp[1])}` : "sharpest week")}
    ${stat(`${b.goal_streak} ${b.goal_streak === 1 ? "week" : "weeks"}`, "longest goal streak")}
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

/** The lesson that teaches a signal or a kind of steering. */
const lessonFor = (key: "signal" | "steer", id: string) => LESSONS.find((l) => key in l.teaches && (l.teaches as Record<string, string>)[key] === id);

function insightsView(p: Progress, meta: Meta): Html {
  const i = p.insights;
  const items: Html[] = [];
  for (const t of i.signals) {
    if (!t.landed_with_without) continue;
    const [w, wo] = t.landed_with_without;
    const l = lessonFor("signal", t.signal);
    items.push(
      html`<li>Briefs that ${verbPhrase(signalTitle(meta, t.signal))} landed <b>${Math.round(w)}%</b> of the time, vs ${Math.round(wo)}% without.${l ? html` <button class="link" data-action="open-lesson" data-id="${l.id}">${l.title} →</button>` : ""}</li>`,
    );
  }
  if (i.steer) {
    const [id, share] = i.steer;
    const l = lessonFor("steer", id);
    items.push(
      html`<li>Most of the steering that was on you is <b>“${steerTitle(meta, id)}”</b> (${share}%).${l ? html` <button class="link" data-action="open-lesson" data-id="${l.id}">${l.title} →</button>` : ""}</li>`,
    );
  }
  const [best, worst] = i.models;
  if (best && worst)
    items.push(html`<li>Your tasks land <b>${best.landed}%</b> of the time with ${best.family}, and ${worst.landed}% with ${worst.family}.</li>`);
  return html`<section><div class="head"><h2>What works for you</h2><p class="muted">From your own history, and only when there's enough of it to say.</p></div>
    ${items.length ? html`<ul class="insights">${items}</ul>` : html`<p class="muted">Not enough history yet. Keep going; these show up as your tasks add up.</p>`}</section>`;
}

function rangeNav(range: Range): Html {
  return html`<nav class="views" aria-label="Time range">${RANGES.map(
    (r) =>
      html`<button class="tab${r === range ? " on" : ""}" data-action="range" data-id="${r}"${r === range ? raw(' aria-current="true"') : ""}>${RANGE_TITLES[r]}</button>`,
  )}</nav>`;
}

export function improveView(ctx: ImproveContext): Html {
  const { meta, progress: p } = ctx;
  const empty = !p.weeks.length;
  return html`
    <section class="toolbar"><span class="eyebrow">Are you getting better?</span></section>
    <section class="duo">${goalView(p, meta, ctx.filtered)}${recapView(p.weeks, meta) || html`<section class="card mini recap"><div class="eyebrow">Last week</div><p class="muted">Your weekly recap shows up once a full week has ${meta.min_week}+ tasks.</p></section>`}</section>
    ${bestsView(p, ctx.filtered)}
    ${trophiesView(p, meta)}
    <div class="head trends-head"><h2>How ${ctx.subject} and you are trending</h2>${rangeNav(ctx.range)}</div>
    ${empty ? html`<p class="muted">No history yet. Your charts start with your first tasks.</p>` : trendsView(ctx)}
    ${insightsView(p, meta)}`;
}
