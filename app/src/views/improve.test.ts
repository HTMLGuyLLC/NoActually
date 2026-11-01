import { describe, expect, it } from "vitest";
import type { Goal, GoalStatus, GoalWeek, HistoryWeek, Progress, ReportCardWeek, SourceInfo } from "../api";
import { fmtDay } from "../format";
import { cards, HOSTILE, meta, noCards, progress, recentCard, source, stats, week } from "../test/fixtures";
import { buckets, canGrade, goalTitle, improveView } from "./improve";

const safe = (markup: string) => {
  expect(markup).not.toContain("<img");
  expect(markup).not.toContain('onerror="');
};
const count = (m: string, needle: string) => m.split(needle).length - 1;

/** `n` Mondays starting 2026-08-03, each with `tasks` tasks, and the report cards `report(i)` gives week `i`. */
const weeks = (n: number, tasks = 20, report: (i: number) => ReportCardWeek = noCards): HistoryWeek[] => {
  const start = Date.UTC(2026, 7, 3);
  return Array.from({ length: n }, (_, i) =>
    week(new Date(start + i * 7 * 86_400_000).toISOString().slice(0, 10), {
      tasks,
      continued: Math.ceil(tasks / 2),
      landed: Math.floor(tasks / 3),
      report_cards: report(i),
    }),
  );
};

describe("buckets", () => {
  it("shows the last 12 weeks, 26 for six months, and everything for all time", () => {
    const w = weeks(40);
    expect(buckets(w, "12w", meta.min_week).points).toHaveLength(12);
    expect(buckets(w, "6m", meta.min_week).points).toHaveLength(26);
    expect(buckets(w, "all", meta.min_week).points).toHaveLength(40);
    expect(buckets(weeks(5), "12w", meta.min_week).points).toHaveLength(5);
  });

  it("keeps the newest weeks when it slices, and carries the week's numbers", () => {
    const w = weeks(14);
    const { points, monthly } = buckets(w, "12w", meta.min_week);
    expect(monthly).toBe(false);
    expect(points[0]).toMatchObject({ tasks: 20, continued: 10, landed: 6, corrections: 8, signals: { spot: 12 } });
    expect(points[0]).not.toHaveProperty("week");
    expect(points[0]?.label).toBeTruthy();
  });

  it("is empty for no history", () => {
    expect(buckets([], "all", meta.min_week)).toEqual({ points: [], monthly: false });
  });

  it("switches to months when a typical week has fewer than min_week tasks and there are more than 8 weeks", () => {
    const { points, monthly } = buckets(weeks(12, 2), "all", meta.min_week);
    expect(monthly).toBe(true);
    // 12 Mondays from Aug 3: five in August, four in September, three in October.
    expect(points.map((p) => p.tasks)).toEqual([10, 8, 6]);
  });

  it("sums every number and every per-signal count into its month", () => {
    const { points } = buckets(weeks(12, 2), "all", meta.min_week);
    expect(points[0]).toMatchObject({ messages: 1000, corrections: 40, humbled: 0, continued: 5, landed: 0 });
    expect(points[0]?.signals).toEqual({ spot: 60, proof: 40, done: 30, limits: 15, why: 20 });
    expect(points[0]?.grades).toEqual({ thin: 40, solid: 35, sharp: 25 });
    expect(points[0]?.steering).toEqual({ ai_wrong: 15, added: 10, clarified: 5, redo: 0 });
    expect(points[0]?.steered).toEqual({ ai_wrong: 10, added: 10, clarified: 5, redo: 0 });
  });

  it("stays weekly with 8 weeks or fewer, however thin", () => {
    expect(buckets(weeks(8, 1), "all", meta.min_week).monthly).toBe(false);
    expect(buckets(weeks(9, 1), "all", meta.min_week).monthly).toBe(true);
  });

  it("stays weekly when the median week is full, even with a few thin ones", () => {
    const w = weeks(12, 20).map((x, i) => (i < 3 ? { ...x, tasks: 1 } : x));
    expect(buckets(w, "all", meta.min_week).monthly).toBe(false);
  });

  it("decides from the weeks in range, not from all history", () => {
    const old = weeks(20, 1); // thin long ago...
    const recent = weeks(12, 20).map((x, i) => ({ ...x, week: `2027-0${1 + Math.floor(i / 5)}-${String(1 + (i % 5) * 7).padStart(2, "0")}` }));
    expect(buckets([...old, ...recent], "12w", meta.min_week).monthly).toBe(false);
    expect(buckets([...old, ...recent], "all", meta.min_week).monthly).toBe(true); // the median of all 32 is a thin week
  });
});

describe("goalTitle", () => {
  it("says what each kind of goal asks", () => {
    expect(goalTitle({ kind: "habit", signal: "why", target: 40 }, meta)).toBe("Include a purpose in 40% of your prompts each week");
    expect(goalTitle({ kind: "steer", steer: "redo", target: 10 }, meta)).toBe("Start over in 10% of tasks or fewer each week");
    expect(goalTitle({ kind: "tier", tier: "coworker", target: 0 }, meta)).toBe("Reach Coworker");
  });

  it("words each follow-up kind naturally, with and without a target", () => {
    const t = (steer: string, target: number) => goalTitle({ kind: "steer", steer, target }, meta);
    expect(t("added", 15)).toBe("Add more after sending in 15% of tasks or fewer each week");
    expect(t("added", 0)).toBe("Go a whole week without adding more after sending");
    expect(t("clarified", 20)).toBe("Clarify after sending in 20% of tasks or fewer each week");
    expect(t("redo", 0)).toBe("Go a whole week without starting over");
    expect(t("ai_wrong", 10)).toBe("Have the AI get it wrong in 10% of tasks or fewer each week");
    expect(t("ai_wrong", 0)).toBe("Go a whole week without the AI getting it wrong");
    expect(t("mystery", 10)).toBe("Have “mystery” in 10% of tasks or fewer each week");
  });

  it("falls back to the id for anything unknown", () => {
    expect(goalTitle({ kind: "habit", signal: "new_one", target: 50 }, meta)).toContain("Include new_one in 50% of your prompts each week");
    expect(goalTitle({ kind: "tier", tier: "legend", target: 0 }, meta)).toBe("Reach legend");
  });
});

describe("goal picker", () => {
  it("lists the options Rust sent, preselects the suggestion, and offers them as form values", () => {
    const options: Goal[] = [
      { kind: "habit", signal: "why", target: 40, id: 0, start: "2026-10-28" },
      { kind: "habit", signal: "limits", target: 30, id: 0, start: "2026-10-28" },
      { kind: "steer", steer: "added", target: 15, id: 0, start: "2026-10-28" },
    ];
    const m = view(progress({ options }));
    expect(count(m, "<option ")).toBe(3);
    expect(m).toMatch(/<option value="[^"]*limits[^"]*" selected>/);
  });

  it("asks before switching away from a goal under way", () => {
    const m = view(progress({ goal: status(), options: [goal()] }));
    expect(m).toContain('data-replaces="yes"');
    expect(m).toContain(">Switch<");
  });
});

const ctx = (p: Progress, over: Partial<Parameters<typeof improveView>[0]> = {}) => ({
  meta,
  stats: stats(),
  progress: p,
  range: "12w" as const,
  subject: "AI",
  filtered: false,
  recent: [],
  canGrade: false,
  hintDismissed: false,
  cardsOn: null,
  ...over,
});
const view = (p: Progress, over: Partial<Parameters<typeof improveView>[0]> = {}) => improveView(ctx(p, over)).value;

const goal = (over: Partial<Goal> = {}): Goal => ({ kind: "habit", signal: "why", id: 1, target: 40, start: "2026-10-05", ...over }) as Goal;
const gw = (w: string, state: GoalWeek["state"], value = 45, count = 20): GoalWeek => ({ week: w, count, value, state });
const status = (over: Partial<GoalStatus> = {}): GoalStatus => ({
  goal: goal(),
  weeks: [gw("2026-10-05", "met"), gw("2026-10-12", "missed", 30), gw("2026-10-19", "paused", 20, 2), gw("2026-10-26", "open", 30, 12)],
  met: 1,
  streak: 0,
  needed: 4,
  done: false,
  need: 2,
  score: null,
  ...over,
});

describe("improveView goal", () => {
  it("suggests a goal with a Start button when there is none", () => {
    const m = view(progress());
    expect(m).toContain("Pick one thing to get better at");
    expect(m).toContain("Try this:");
    expect(m).toContain("Include guardrails in 30% of your prompts each week");
    expect(m).toContain(
      'data-action="start-goal" data-goal="{&quot;kind&quot;:&quot;habit&quot;,&quot;signal&quot;:&quot;limits&quot;,&quot;target&quot;:30}">Start this goal',
    );
    expect(m).toContain("Hit it 4 weeks to win a trophy.");
    expect(m).not.toContain('data-action="clear-goal"');
    expect(m).toContain("Or pick another");
  });

  it("preselects the suggested goal in the picker and lists every option", () => {
    const m = view(progress());
    expect(m).toContain('<select id="goal-pick">');
    expect(count(m, "<option")).toBe(3);
    expect(count(m, " selected")).toBe(1);
    expect(m).toMatch(/<option value="[^"]*limits[^"]*" selected>Include guardrails in 30%/);
  });

  it("shows a goal under way with a mark and spoken text for each week", () => {
    const m = view(progress({ goal: status() }));
    expect(m).toContain("Your goal · since");
    expect(m).toContain("Include a purpose in 40% of your prompts each week");
    expect(m).toContain("<b>1</b> of 4 weeks met");
    expect(m).toContain('<ol class="goal-weeks" aria-label="Weeks so far">');
    for (const [state, mark] of [
      ["met", "✓"],
      ["missed", "✗"],
      ["paused", "–"],
      ["open", "•"],
    ]) {
      expect(m).toContain(`class="gw ${state}"`);
      expect(m).toContain(`<span aria-hidden="true">${mark}</span><span class="sr-only">Week of`);
    }
    expect(m).toMatch(/<span class="sr-only">Week of [^<]+: 45% of 20 prompts, met\.<\/span>/);
    expect(m).toMatch(/<span class="sr-only">Week of [^<]+: 20% of 2 prompts, too few tasks\.<\/span>/);
    expect(m).toContain("Weeks with fewer than 5 prompts pause the goal instead of breaking the streak.");
    expect(m).not.toContain("Try this:");
    expect(m).toContain("Drop this goal");
    expect(m).toContain("Change goal");
    expect(m).toContain("Switch to");
  });

  it("says how many more prompts would hit a habit goal this week", () => {
    expect(view(progress({ goal: status({ need: 2 }) }))).toContain("12 prompts so far. 2 more prompts with a purpose would hit it.");
    expect(view(progress({ goal: status({ need: 1 }) }))).toContain("1 more prompt with a purpose would hit it.");
    expect(view(progress({ goal: status({ need: 0 }) }))).toContain("12 prompts so far. On track.");
  });

  it("mentions a streak only past one week", () => {
    expect(view(progress({ goal: status({ streak: 3 }) }))).toContain("3 in a row");
    expect(view(progress({ goal: status({ streak: 1 }) }))).not.toContain("in a row");
  });

  it("measures a steering goal in tasks, and is on track under its ceiling", () => {
    const st = status({ goal: goal({ kind: "steer", steer: "redo", target: 10 } as Partial<Goal>), weeks: [gw("2026-10-26", "open", 5, 20)], need: null });
    const m = view(progress({ goal: st }));
    expect(m).toContain("Start over in 10% of tasks or fewer each week");
    expect(m).toContain("20 tasks so far. On track.");
    const over = view(progress({ goal: status({ goal: st.goal, weeks: [gw("2026-10-26", "open", 25, 20)], need: null }) }));
    expect(over).toContain("20 tasks so far.");
    expect(over).not.toContain("On track.");
    // Too few tasks to say yet: not "on track" either.
    expect(view(progress({ goal: status({ goal: st.goal, weeks: [gw("2026-10-26", "open", 0, 2)], need: null }) }))).not.toContain("On track.");
  });

  it("shows a tier goal by score, with no weeks", () => {
    const st = status({ goal: goal({ kind: "tier", tier: "coworker", target: 0 } as Partial<Goal>), weeks: [], needed: 0, score: 70, need: null });
    const m = view(progress({ goal: st }));
    expect(m).toContain("Reach Coworker");
    expect(m).not.toContain("Your score is"); // the meter already says "70 of 65 to reach …"
    expect(m).not.toContain("goal-weeks");
    expect(m).not.toContain("weeks met");
    expect(view(progress({ goal: { ...st, score: null } }))).toContain("Not scored yet: you need more tasks with a follow-up.");
  });

  it("says a done goal is done, opens the picker for the next one", () => {
    const m = view(progress({ goal: status({ done: true, met: 4, streak: 4 }) }));
    expect(m).toContain("Goal done 🏆");
    expect(m).toContain("It&#39;s on your trophy shelf");
    expect(m).toContain("<details open>");
    expect(m).toContain("Next goal");
    expect(m).not.toContain("Your goal · since");
    expect(view(progress({ goal: status() }))).not.toContain("<details open>");
  });
});

describe("improveView first run", () => {
  it("leads with Choose sources when none is allowed", () => {
    const m = view(progress({ weeks: [] }), { noSource: true });
    expect(m).toContain('data-action="go-settings" data-id="sources">Choose sources</button>');
    expect(view(progress({ weeks: [] }))).not.toContain("Choose sources");
  });
});

describe("improveView history", () => {
  it("says there is no history yet, and draws no chart", () => {
    const m = view(progress({ weeks: [] }));
    expect(m).toContain("No history yet. Your charts start with your first tasks.");
    expect(m).not.toContain("<svg");
    expect(m).toContain("Your weekly recap shows up once a full week has 5+ tasks.");
  });

  it("draws every chart with a label and a summary once there is history", () => {
    const m = view(progress({ weeks: weeks(10) }));
    expect(count(m, 'aria-describedby="')).toBe(4);
    for (const id of ["landed", "grades", "steering", "rate"]) {
      expect(m).toContain(`aria-describedby="${id}-summary"`);
      expect(m).toContain(`id="${id}-summary"`);
    }
    expect(count(m, 'class="spark"')).toBe(5);
    expect(m).toContain("Per week.");
    expect(m).toContain("How AI and you are trending");
  });

  it("names the charts as regions, one tab stop each, and states their latest values", () => {
    const m = view(progress({ weeks: weeks(10) }));
    expect(count(m, 'class="chart" role="region"')).toBe(4);
    expect(m).not.toContain('class="chart" tabindex');
    expect(m).toMatch(/Prompts by grade over 10 weeks\. Latest week \(.+\): .*Specific \d+/);
    expect(m).toMatch(/Corrections per 100 messages: [\d.]+ in the latest week \(.+\), [\d.]+ on average\./);
    expect(m).toContain("Hollow dots had fewer than 50 messages");
  });

  it("has an h1 for the page", () => {
    expect(view(progress())).toContain('<h1 class="eyebrow">Are you getting better?</h1>');
  });

  it("makes each goal week focusable", () => {
    expect(view(progress({ goal: status() }))).toMatch(/<li class="gw [a-z]+" tabindex="0" data-tip=/);
  });

  it("switches to months when weeks are thin", () => {
    const m = view(progress({ weeks: weeks(12, 2) }));
    expect(m).toContain("Per month.");
    expect(m).toContain("per month");
  });

  it("summarises the landed trend, or says there isn't enough to show one", () => {
    expect(view(progress({ weeks: weeks(10) }))).toMatch(/Clear the first time went from 60% \(.+\) to 60% \(.+\)\./);
    const noFollowUps = weeks(10, 1).map((w) => ({ ...w, continued: 0, landed: 0 }));
    expect(view(progress({ weeks: noFollowUps }))).toContain("Not enough tasks yet to show a trend.");
  });

  it("marks the chosen range", () => {
    const m = view(progress(), { range: "6m" });
    expect(m).toContain('aria-label="Time range"');
    expect(m).toContain('data-action="range" data-id="6m" aria-current="true">6 months');
    expect(m).toContain('data-action="range" data-id="12w">12 weeks');
    expect(m).toContain('data-action="range" data-id="all">All time');
    expect(count(m, 'aria-current="true"')).toBe(1);
  });
});

describe("improveView recap, bests and trophies", () => {
  it("compares the last full week with the week before, leaving out the week under way", () => {
    // Last full week is 2026-10-19 (9 of 12 landed: 75%); the one before is 10-12 (11 of 12: 91%).
    const m = view(progress());
    expect(m).toContain("Last week · ");
    expect(m).toContain("<span>Clear the first time</span><b>75%</b>");
    expect(m).toContain('<span aria-hidden="true">▼</span><span class="sr-only">down </span> 16');
    expect(m).toContain("<span>Specific prompts</span><b>25%</b>");
    expect(m).toContain("<span>Corrections</span><b>8</b>");
  });

  it("marks a rise with an up arrow", () => {
    const p = progress({ weeks: [week("2026-10-05", { landed: 6 }), week("2026-10-12", { landed: 9 }), week("2026-10-19")] });
    expect(view(p)).toContain('<span aria-hidden="true">▲</span><span class="sr-only">up </span> 25');
  });

  it("waits for a full week of tasks before comparing", () => {
    const p = progress({ weeks: [week("2026-10-05"), week("2026-10-12", { tasks: 2, continued: 1, landed: 1 }), week("2026-10-19")] });
    expect(view(p)).toContain("Your weekly recap shows up once a full week has 5+ tasks.");
    expect(view(progress({ weeks: [week("2026-10-05"), week("2026-10-12")] }))).toContain("Your weekly recap shows up");
  });

  it("shows the bests, with dashes before there are any", () => {
    const m = view(progress());
    expect(m).toContain('<span class="v">91%</span><span class="l">best 4 weeks clear the first time · to ');
    expect(m).toContain('<span class="v">40%</span><span class="l">most specific week · ');
    expect(m).toContain('<span class="v">0 weeks</span><span class="l">longest goal streak</span>');
    const none = view(progress({ bests: { landed: null, sharp: null, goal_streak: 1, report_card: null } }));
    expect(none).toContain('<span class="v">–</span><span class="l">best 4 weeks clear the first time</span>');
    expect(none).toContain('<span class="v">1 week</span>');
  });

  it("shelves trophies by their goal, and counts them", () => {
    const t = { goal: goal(), done: "2026-10-01" };
    expect(view(progress())).not.toContain("Trophy shelf");
    expect(view(progress())).toContain('<span class="l">trophies</span>');
    const m = view(progress({ trophies: [t] }));
    expect(m).toContain("Trophy shelf");
    expect(m).toContain("<b>Include a purpose in 40% of your prompts each week</b>");
    expect(m).toContain('<span class="v">1</span><span class="l">trophy</span>');
  });
});

describe("improveView insights", () => {
  it("says there isn't enough history when there is nothing to report", () => {
    expect(view(progress())).toContain("Not enough history yet.");
    expect(view(progress())).not.toContain('class="insights"');
  });

  it("reports a signal only when its history backs the with/without rates, and links its lesson", () => {
    const signals = [
      { signal: "why", pct_of_prompts: 20, landed_with_without: [93.3, 80] as [number, number] },
      { signal: "limits", pct_of_prompts: 10, landed_with_without: null },
    ];
    const m = view(progress({ insights: { signals, steer: null, models: [], report_cards: null, top_tip: null, tip_trend: null } }));
    expect(m).toContain('<ul class="insights">');
    expect(m).toContain("Prompts with a purpose were clear the first time <b>93%</b> of the time, vs 80% without.");
    expect(m).toContain('data-action="open-lesson" data-id="say-why"');
    expect(count(m, "<li>Prompts with")).toBe(1);
    expect(m).not.toContain("Not enough history yet.");
  });

  it("reports the steering that was on you, with its lesson", () => {
    const m = view(progress({ insights: { signals: [], steer: ["added", 60], models: [], report_cards: null, top_tip: null, tip_trend: null } }));
    expect(m).toContain("Your most common follow-up is <b>“You added more”</b> (60%).");
    expect(m).toContain('data-action="open-lesson" data-id="say-it-all-up-front"');
  });

  it("reports how models compare only when there are two", () => {
    const models = [
      { family: "Claude Opus", landed: 90, continued: 40 },
      { family: "GPT Astra", landed: 70, continued: 30 },
    ];
    expect(view(progress({ insights: { signals: [], steer: null, models, report_cards: null, top_tip: null, tip_trend: null } }))).toContain(
      "Your prompts are clear the first time <b>90%</b> of the time with Claude Opus, and 70% with GPT Astra.",
    );
    expect(
      view(progress({ insights: { signals: [], steer: null, models: models.slice(0, 1), report_cards: null, top_tip: null, tip_trend: null } })),
    ).toContain("Not enough history yet.");
  });
});

describe("improveView escaping", () => {
  it("escapes everything that came from a name, a model or the subject", () => {
    const bad = {
      ...meta,
      signals: meta.signals.map((s) => ({ ...s, title: HOSTILE, hint: HOSTILE })),
      steers: meta.steers.map((s) => ({ ...s, title: HOSTILE })),
      tiers: meta.tiers.map((t) => ({ ...t, name: HOSTILE })),
      grades: meta.grades.map((g) => ({ ...g, title: HOSTILE })),
      prompt_tiers: meta.prompt_tiers.map((t) => ({ ...t, name: HOSTILE })),
    };
    const p = progress({
      weeks: weeks(10),
      goal: status(),
      trophies: [{ goal: goal(), done: "2026-10-01" }],
      insights: {
        signals: [{ signal: HOSTILE, pct_of_prompts: 1, landed_with_without: [1, 2] }],
        steer: [HOSTILE, 50],
        models: [
          { family: HOSTILE, landed: 1, continued: 1 },
          { family: HOSTILE, landed: 1, continued: 1 },
        ],
        report_cards: { first_ab: 10, last_ab: 20, habit: { habit: HOSTILE, first: 6, last: 2 } },
        top_tip: { habit: HOSTILE, tips: 5, of: 5 },
        tip_trend: null,
      },
      suggestion: { kind: "habit", signal: HOSTILE, target: 30, id: 0, start: "2026-10-28" },
    });
    const more = { recent: [recentCard({ source: HOSTILE, grade: HOSTILE, missing: HOSTILE })], canGrade: true, hintDismissed: false, cardsOn: null };
    safe(improveView({ meta: bad, stats: stats(), progress: p, range: "12w", subject: HOSTILE, filtered: true, ...more }).value);
    safe(improveView({ meta: bad, stats: stats(), progress: { ...p, goal: null }, range: "all", subject: HOSTILE, filtered: false, ...more }).value);
    // With report cards in range, so the block, the habit rows and the recent list are drawn too.
    const graded = { ...p, weeks: weeks(10, 20, () => cards(5, { grades: { A: 5 }, missing: { target: 1, context: 2 } })) };
    safe(improveView({ meta: bad, stats: stats(), progress: graded, range: "12w", subject: HOSTILE, filtered: false, ...more }).value);
  });
});

describe("goal meter", () => {
  it("shows the score against the tier for a tier goal, and this week against the target for a habit", () => {
    const tierGoal = { kind: "tier", tier: "irreplaceable", id: 1, target: 0, start: "2026-10-29" } as Goal;
    const tier = view(progress({ goal: status({ goal: tierGoal, needed: 0, weeks: [], score: 87 }) }));
    expect(tier).toContain('<span class="big">87</span><span class="of">of 90 to reach Irreplaceable</span>');
    expect(tier).toContain('aria-label="Score 87 of 90"');
    const open = { week: "2026-10-26", count: 3, value: 33, state: "open" } as GoalWeek;
    const habit = view(progress({ goal: status({ weeks: [open] }) }));
    expect(habit).toContain('<span class="big">33%</span>');
    expect(habit).toContain("aim for at least");
    expect(habit).toContain('class="card mini goal"'); // a plain card, like the rest
  });
});

describe("buckets and report cards", () => {
  // 12 Mondays from Aug 3 with 2 tasks each: five weeks in August, four in September, three in October.
  const monthly = (report: (i: number) => ReportCardWeek) => buckets(weeks(12, 2, report), "all", meta.min_week).points;

  it("carries each week's report cards when it stays weekly", () => {
    const { points } = buckets(
      weeks(3, 20, () => cards(4, { grades: { A: 4 } })),
      "12w",
      meta.min_week,
    );
    expect(points[0]?.report_cards).toEqual(cards(4, { grades: { A: 4 } }));
  });

  it("sums every report card number into its month", () => {
    const m = monthly(() =>
      cards(2, {
        grades: { A: 1, C: 1 },
        avoidable: 2,
        extra_tokens: 1000,
        extra_cost: 0.5,
        priced: 1,
        extra_minutes: 3,
        missing: { finish: 1, context: 2 },
        gradable_tasks: 4,
      }),
    );
    expect(m[0]?.report_cards).toEqual(
      cards(10, {
        grades: { A: 5, C: 5 },
        avoidable: 10,
        extra_tokens: 5000,
        extra_cost: 2.5,
        priced: 5,
        extra_minutes: 15,
        missing: { finish: 5, context: 10 },
        gradable_tasks: 20,
      }),
    );
    expect(m.map((p) => p.report_cards.graded)).toEqual([10, 8, 6]);
    expect(m[2]?.report_cards.grades).toEqual({ A: 3, B: 0, C: 3, D: 0, F: 0 });
  });

  it("leaves a month's cost unknown only while none of its weeks had a price", () => {
    const m = monthly((i) => cards(2, { extra_cost: i === 4 ? 0.5 : null }));
    // August's last week was priced; September had none.
    expect(m.map((p) => p.report_cards.extra_cost)).toEqual([0.5, null, null]);
  });

  it("doesn't change the weeks it was given", () => {
    const w = weeks(12, 2, () => cards(2, { grades: { A: 2 } }));
    buckets(w, "all", meta.min_week);
    expect(w[0]?.report_cards.grades.A).toBe(2);
    expect(w[0]?.report_cards.graded).toBe(2);
  });
});

describe("improveView recap report cards", () => {
  const recap = (before: ReportCardWeek, latest: ReportCardWeek, over: Partial<HistoryWeek> = {}) =>
    view(
      progress({
        weeks: [week("2026-10-05"), week("2026-10-12", { report_cards: before }), week("2026-10-19", { report_cards: latest, ...over }), week("2026-10-26")],
      }),
    );
  const before = cards(5, { grades: { A: 1, B: 1, C: 3 } });
  const latest = cards(6, { grades: { A: 3, B: 1, C: 2 }, extra_cost: 0.9, extra_minutes: 12 });

  it("adds one line when last week and the week before each had enough graded tasks, with minutes per task", () => {
    const m = recap(before, latest);
    expect(m).toContain('<p class="recap-cards">Report cards: 4 of 6 tasks A or B (up from 2 of 5) · about 2 extra minutes per task.</p>');
    expect(count(m, "Report cards: ")).toBe(1);
  });

  it("leads with minutes and never puts dollars in the line, priced or not", () => {
    for (const over of [{ extra_cost: null }, { extra_cost: 0 }, { extra_cost: 6 }, { extra_cost: 0.6, priced: 2 }, { extra_cost: 0.6, priced: 0 }]) {
      const m = recap(before, { ...latest, ...over });
      expect(m, JSON.stringify(over)).toContain("· about 2 extra minutes per task.");
      expect(m.match(/<p class="recap-cards">.*?<\/p>/)?.[0], JSON.stringify(over)).not.toContain("$");
    }
  });

  it("leaves the minutes out when nothing was redone, and says minute for one", () => {
    expect(recap(before, { ...latest, extra_minutes: 0 })).toContain("(up from 2 of 5).</p>");
    expect(recap(before, { ...latest, extra_minutes: 2 })).toContain("(up from 2 of 5).</p>"); // 2 over 6 tasks: 0.33
    expect(recap(before, { ...latest, extra_minutes: 3 })).toContain("· about 1 extra minute per task.</p>"); // 0.5 rounds up
  });

  it("says under a minute, not nothing, when follow-ups redid work but it rounds to no minute", () => {
    expect(recap(before, { ...latest, avoidable: 2, extra_minutes: 2 })).toContain("(up from 2 of 5) · under a minute extra per task.</p>");
    expect(recap(before, { ...latest, avoidable: 2, extra_minutes: 0, extra_tokens: 9_000 })).toContain("· under a minute extra per task.</p>");
    expect(recap(before, { ...latest, avoidable: 2, extra_minutes: 12 })).toContain("· about 2 extra minutes per task.</p>");
    expect(recap(before, { ...latest, avoidable: 0, extra_minutes: 2 })).toContain("(up from 2 of 5).</p>");
  });

  it("compares shares, not counts", () => {
    expect(recap(cards(4, { grades: { A: 3, B: 1 } }), cards(6, { grades: { A: 4 }, extra_cost: null }))).toContain("(down from 4 of 4)");
    expect(recap(cards(4, { grades: { A: 2 } }), cards(6, { grades: { A: 3 } }))).toContain("(same as 2 of 4)");
    expect(recap(cards(10, { grades: { A: 3 } }), cards(3, { grades: { A: 3 } }))).toContain("(up from 3 of 10)");
  });

  it("needs min_card_week graded tasks in both weeks", () => {
    expect(recap(before, cards(2, { grades: { A: 2 } }))).not.toContain("Report cards: ");
    expect(recap(cards(2, { grades: { A: 2 } }), latest)).not.toContain("Report cards: ");
    expect(recap(before, cards(3, { grades: { A: 3 } }))).toContain("Report cards: 3 of 3 tasks A or B");
    expect(recap(noCards(), noCards())).not.toContain("Report cards: ");
  });

  it("is about the last full week, not the one under way", () => {
    const p = progress({
      weeks: [
        week("2026-10-05", { report_cards: before }),
        week("2026-10-12", { report_cards: latest }),
        week("2026-10-19", { report_cards: cards(3, { grades: { F: 3 } }) }),
      ],
    });
    expect(view(p)).toContain("Report cards: 4 of 6 tasks A or B (up from 2 of 5)");
    expect(view(p)).not.toContain("0 of 3");
  });

  it("still shows the line when the week had too few tasks for the other rows", () => {
    const m = recap(before, latest, { tasks: 2, continued: 1, landed: 1 });
    expect(m).toContain("Report cards: 4 of 6 tasks A or B");
    expect(m).not.toContain("Clear the first time</span>");
    expect(m).not.toContain("Your weekly recap shows up");
  });
});

describe("improveView report cards", () => {
  const graded = (n = 10, report: (i: number) => ReportCardWeek = () => cards(5, { grades: { A: 2, B: 2, C: 1 }, avoidable: 3 })) =>
    progress({ weeks: weeks(n, 20, report) });
  const block = (m: string) => {
    const at = m.indexOf('<section class="cards-block">');
    return at < 0 ? "" : m.slice(at, m.indexOf("</section>", at));
  };

  it("is left out when the range has no graded task", () => {
    expect(view(progress({ weeks: weeks(10) }))).not.toContain("Your report cards");
    expect(view(progress())).not.toContain("cards-block");
    // Report cards before the range don't count: 40 weeks, graded only in the first.
    const old = progress({ weeks: weeks(40, 20, (i) => (i === 0 ? cards(5, { grades: { A: 5 } }) : noCards())) });
    expect(view(old)).not.toContain("Your report cards");
    expect(view(old, { range: "all" })).toContain("Your report cards");
  });

  it("explains itself under its heading", () => {
    const m = view(graded());
    expect(m).toContain("<h2>Your report cards</h2>");
    expect(m).toContain("A means nothing had to be redone. A letter grade looks at the whole task; a prompt grade looks at your first message only.");
  });

  it("draws the grades as a stacked bar per week, A to F, with a label and a summary", () => {
    const m = view(graded());
    expect(m).toContain('aria-describedby="cards-summary"');
    expect(m).toContain('id="cards-summary"');
    expect(m).toContain('aria-label="Report cards by letter grade, per week"');
    expect(m).toContain('class="chart" role="region" aria-label="Report card grades chart, scrolls sideways"');
    expect(m).toContain("50 report cards over 10 weeks. Latest week (");
    expect(m).toMatch(/: A 2, B 2, C 1, D 0, F 0\./);
    const key = block(m).match(/<ul class="chart-key">.*?<\/ul>/s)?.[0] ?? "";
    expect(key.match(/<\/i>[A-F]</g)).toEqual(["</i>A<", "</i>B<", "</i>C<", "</i>D<", "</i>F<"]);
    expect(block(m)).toContain('data-tip="Aug 3: A 2"');
  });

  it("draws the follow-ups that redid work per graded task (the CLI's words): avoidable rounds over graded tasks", () => {
    const m = view(graded());
    expect(m).toContain("<h3>Follow-ups that redid work, per task</h3>");
    expect(m).toContain('aria-label="Follow-ups that redid work, per task, chart, scrolls sideways"');
    expect(m).toContain('aria-describedby="card-rounds-summary"');
    expect(m).toContain("Follow-ups that redid work per graded task went from 0.6 (Aug 3) to 0.6 (Oct 5).");
    expect(m).toContain('data-tip="Aug 3: 0.6 follow-ups that redid work per task · 5 report cards · under a minute extra per task"');
    expect(m).toContain("Hollow dots had fewer than 3 report cards");
    expect(m).not.toContain("Extra back-and-forth");
    expect(m).not.toContain("could have skipped");
  });

  it("puts the minutes per graded task in the tooltip and the summary, and dollars only with a price, labeled", () => {
    const m = view(graded(10, () => cards(5, { grades: { A: 5 }, avoidable: 1, extra_cost: 1, extra_minutes: 10 })));
    expect(m).toContain(
      'data-tip="Aug 3: 0.2 follow-ups that redid work per task · 5 report cards · about 2 extra minutes per task · about $0.20 per task at pay-as-you-go prices"',
    );
    expect(m).toContain("About 2 extra minutes per task. About $0.20 per task at pay-as-you-go prices.");
    expect(m).not.toContain("subscription");
    // No price (Codex, GPT): minutes, and never tokens or dollars.
    const unpriced = view(graded(10, () => cards(5, { grades: { A: 5 }, avoidable: 1, extra_minutes: 10, extra_tokens: 99_999 })));
    expect(unpriced).toContain('data-tip="Aug 3: 0.2 follow-ups that redid work per task · 5 report cards · about 2 extra minutes per task"');
    expect(unpriced).toContain("About 2 extra minutes per task.");
    for (const bit of ["$", "pay-as-you-go", "99,999", "token"]) expect(block(unpriced), bit).not.toContain(bit);
  });

  it("works the cost per task out over the priced tasks only", () => {
    // Five graded, two priced at $1 in all: 50 cents each.
    const m = view(graded(10, () => cards(5, { grades: { A: 5 }, avoidable: 1, extra_cost: 1, priced: 2 })));
    expect(m).toContain("5 report cards · under a minute extra per task · about $0.50 per task at pay-as-you-go prices");
  });

  it("says follow-up, not follow-ups, for exactly one", () => {
    const m = view(graded(10, () => cards(5, { grades: { A: 5 }, avoidable: 5, extra_minutes: 5 })));
    expect(m).toContain('data-tip="Aug 3: 1 follow-up that redid work per task · 5 report cards · about 1 extra minute per task"');
  });

  it("leaves the minutes out of the tooltip when a week redid nothing", () => {
    const m = view(graded(10, () => cards(5, { grades: { A: 5 } })));
    expect(m).toContain('data-tip="Aug 3: 0 follow-ups that redid work per task · 5 report cards"');
    expect(block(m)).not.toContain("under a minute");
  });

  it("says under a minute, not nothing, when follow-ups redid work but it rounds to no minute", () => {
    // 5 tasks, 2 follow-ups, 2 minutes in all: 0.4 minutes each.
    const m = view(graded(10, () => cards(5, { grades: { A: 5 }, avoidable: 2, extra_minutes: 2, extra_tokens: 40_000 })));
    expect(m).toContain('data-tip="Aug 3: 0.4 follow-ups that redid work per task · 5 report cards · under a minute extra per task"');
    expect(m).toContain("Under a minute extra per task.");
    expect(block(m)).not.toContain("about 0");
    // It comes after the dollars rule too: a price adds a sentence, it doesn't replace this one.
    const priced = view(graded(10, () => cards(5, { grades: { A: 5 }, avoidable: 2, extra_minutes: 0, extra_cost: 1 })));
    expect(priced).toContain("5 report cards · under a minute extra per task · about $0.20 per task at pay-as-you-go prices");
  });

  it("draws weeks with fewer than min_card_week graded tasks as hollow dots", () => {
    const m = view(graded(10, (i) => (i < 4 ? cards(2, { grades: { A: 2 }, avoidable: 1 }) : cards(3, { grades: { A: 3 }, avoidable: 3 }))));
    const dots = block(m);
    expect(count(dots, 'class="dot-weak"')).toBe(4);
    expect(count(dots, 'class="dot-strong"')).toBe(6);
    expect(dots).toContain("0.5 follow-ups that redid work per task · 2 report cards · under a minute extra per task (fewer than 3 report cards)");
    // The line starts at the first solid week.
    expect(m).toContain("went from 1 (Aug 31) to 1 (Oct 5).");
  });

  it("says when there aren't enough report cards for a trend", () => {
    const m = view(graded(10, (i) => (i === 9 ? cards(1, { grades: { A: 1 } }) : noCards())));
    expect(m).toContain("Not enough report cards yet to show a trend.");
    expect(count(block(m), 'class="dot-weak"')).toBe(1);
  });

  it("follows the months when weeks are thin", () => {
    const m = view(progress({ weeks: weeks(12, 2, () => cards(2, { grades: { A: 1, B: 1 }, avoidable: 1 })) }));
    expect(m).toContain('aria-label="Report cards by letter grade, per month"');
    expect(m).toContain("24 report cards over 3 months.");
    expect(m).toContain(`data-tip="${fmtDay("2026-08-01", { month: "short", year: "2-digit" })}: A 5"`);
  });

  it("lists the latest report cards: day graded, tool, grade, what was left out, what it took", () => {
    const recent = [
      recentCard(),
      recentCard({ day: "2026-10-26", graded_day: "2026-10-26", source: "codex", grade: "A", missing: null, extra_cost: 0, extra_tokens: 0, extra_minutes: 0 }),
      recentCard({ day: "2026-10-25", graded_day: "2026-10-25", grade: "D", missing: "context", extra_cost: null, extra_tokens: 12_345, extra_minutes: 9 }),
    ];
    const m = view(graded(), { recent });
    expect(m).toContain("<h3>Recent report cards</h3>");
    const list = m.match(/<ul class="recent-cards">.*?<\/ul>/s)?.[0] ?? "";
    expect(count(list, "<li>")).toBe(3);
    expect(list).toContain(
      `<li><span class="rc-day">${fmtDay("2026-10-27")}</span><span>Claude Code</span><span>Grade <b>B</b></span><span>Left out: finish line</span><span class="muted">about 6 extra minutes</span><span class="muted">about $0.40</span></li>`,
    );
    expect(list).toContain('<span>Codex</span><span>Grade <b>A</b></span><span class="muted">Nothing left out</span><span class="muted">Nothing redone</span>');
    // No price: minutes, and never a count of tokens.
    expect(list).toContain('<span>Left out: context</span><span class="muted">about 9 extra minutes</span></li>');
    expect(list).not.toContain("12,345");
    expect(list).not.toContain("pay-as-you-go");
    expect(list).not.toContain("title=");
    // Said once, under the list, because a row shows dollars.
    const note = "Dollars are at pay-as-you-go prices. On a subscription this came out of your usage limit, not your wallet.";
    expect(count(m, note)).toBe(1);
    expect(m).toContain(`</ul><p class="muted small">${note}</p>`);
  });

  it("says nothing about dollars when no recent card shows any", () => {
    const none = [recentCard({ extra_cost: null }), recentCard({ extra_cost: 0.001 }), recentCard({ extra_cost: 0, extra_minutes: 0, extra_tokens: 0 })];
    const m = view(graded(), { recent: none });
    expect(m).toContain("<h3>Recent report cards</h3>");
    expect(m).not.toContain("Dollars are");
    expect(m).not.toContain("subscription");
  });

  it("dates a card by the day it was graded, not the day its task began", () => {
    const list =
      view(graded(), { recent: [recentCard({ day: "2026-10-20", graded_day: "2026-10-27" })] }).match(/<ul class="recent-cards">.*?<\/ul>/s)?.[0] ?? "";
    expect(list).toContain(`<span class="rc-day">${fmtDay("2026-10-27")}</span>`);
    expect(list).not.toContain(fmtDay("2026-10-20"));
  });

  it("says under a minute, not nothing, when a card redid work that came to no whole minute", () => {
    const list =
      view(graded(), {
        recent: [
          recentCard({ extra_minutes: 0, extra_tokens: 3_000, extra_cost: null }),
          recentCard({ extra_minutes: 0, extra_tokens: 3_000, extra_cost: 0.02 }),
        ],
      }).match(/<ul class="recent-cards">.*?<\/ul>/s)?.[0] ?? "";
    expect(list).toContain('<span>Left out: finish line</span><span class="muted">under a minute extra</span></li>');
    // Priced: the dollars follow, as they do after minutes.
    expect(list).toContain('<span class="muted">under a minute extra</span><span class="muted">about $0.02</span>');
    expect(list).not.toContain("Nothing redone");
    expect(list).not.toContain("3,000");
  });

  it("says minute for one, and leaves a price that rounds to nothing out", () => {
    const list = view(graded(), { recent: [recentCard({ extra_minutes: 1, extra_cost: 0.001 })] }).match(/<ul class="recent-cards">.*?<\/ul>/s)?.[0] ?? "";
    expect(list).toContain('<span class="muted">about 1 extra minute</span></li>');
    expect(list).not.toContain("$");
  });

  it("leaves the list out when there are no recent cards", () => {
    expect(view(graded(), { recent: [] })).not.toContain("Recent report cards");
  });

  it("ends with one muted line: how much the report cards cover, and why they may not be typical", () => {
    const m = view(graded(10, () => cards(4, { grades: { A: 4 }, gradable_tasks: 10 })));
    const notes = block(m).match(/<p class="muted small">/g) ?? [];
    expect(notes).toHaveLength(1);
    expect(block(m)).toContain(
      '<p class="muted small">From 40 of 100 tasks in tools that support report cards. You choose which tasks to grade, so these may not be typical.</p>',
    );
    // Never more graded than there are tasks to grade.
    expect(view(graded(10, () => cards(4, { grades: { A: 4 }, gradable_tasks: 1 })))).toContain("From 40 of 40 tasks in tools that support report cards.");
  });

  it("says these are the tasks you chose, once", () => {
    const m = view(graded(10, () => cards(4, { grades: { A: 4 } })));
    expect(m).toContain("You choose which tasks to grade, so these may not be typical.");
    // The grade explanations are under the heading once, not repeated at the foot.
    expect(count(m, "looks at the whole task")).toBe(1);
  });

  it("counts only the weeks in range", () => {
    const p = progress({ weeks: weeks(40, 20, (i) => (i < 28 ? cards(100, { grades: { A: 100 } }) : cards(4, { grades: { A: 4 } }))) });
    expect(view(p)).toContain("From 48 of 96 tasks");
  });

  it("uses grade colors that step from A to F, one class each", () => {
    const m = block(view(graded()));
    const segs = ["ga", "gb", "gc", "gd", "gf"].map((g) => count(m, `class="seg ${g}"`));
    expect(segs.every((n) => n > 0)).toBe(true);
    expect(m).not.toMatch(/class="seg s[45]"/);
  });
});

describe("improveView habit rows", () => {
  const withCards = progress({ weeks: weeks(10, 20, () => cards(5, { grades: { A: 5 }, missing: { target: 1, finish: 3, context: 2 } })) });

  it("is just the five prompt habits, with no report card counts and no Context row", () => {
    for (const m of [view(withCards), view(progress({ weeks: weeks(10) }))]) {
      expect(m).not.toContain("missing in ");
      expect(m).not.toContain("Context");
      expect(m).not.toContain("Only a report card can tell");
      expect(m).not.toContain("Not measured from prompts");
      expect(count(m, 'class="spark"')).toBe(5);
      expect(m).toContain('Target<small class="muted">a file, function, page or link</small></span>');
    }
  });
});

describe("improveView report card insights", () => {
  const trend = (over: Partial<NonNullable<Progress["insights"]["report_cards"]>> = {}) =>
    progress({
      insights: {
        signals: [],
        steer: null,
        models: [],
        report_cards: { first_ab: 35, last_ab: 70, habit: { habit: "finish", first: 6, last: 2 }, ...over },
        top_tip: null,
        tip_trend: null,
      },
    });

  it("says how your grades moved, and which habit you dropped", () => {
    const m = view(trend());
    expect(m).toContain("<li>Report cards A or B: <b>35% → 70%</b> since you started.</li>");
    expect(m).toContain("<li>You used to leave out the finish line in <b>6 of 20</b> graded tasks. Now 2 of 20.</li>");
    expect(m).not.toContain("Not enough history yet.");
  });

  it("names the habit in plain words, Context included", () => {
    expect(view(trend({ habit: { habit: "context", first: 9, last: 4 } }))).toContain("leave out the context in <b>9 of 20</b> graded tasks. Now 4 of 20.");
    expect(view(trend({ habit: { habit: "guardrails", first: 9, last: 4 } }))).toContain("leave out the guardrails in");
  });

  it("leaves the habit out when none dropped", () => {
    const m = view(trend({ habit: null }));
    expect(m).toContain("Report cards A or B: <b>35% → 70%</b>");
    expect(m).not.toContain("leave out");
  });

  it("says nothing without a comparison", () => {
    expect(view(progress())).not.toContain("since you started");
  });
});

describe("improveView bests", () => {
  const best = progress({ bests: { landed: [91, "2026-10-19"], sharp: [40, "2026-10-05"], goal_streak: 0, report_card: [90, "2026-10-19"] } });

  it("shows four bests, with no report card tile even when there is a best report-card week", () => {
    const m = view(best);
    expect(m).toContain('<section class="stats">');
    expect(count(m, 'class="stat"')).toBe(4);
    expect(m).not.toContain("report-card week");
    expect(m).not.toContain("stats five");
    expect(view(progress())).toContain('<section class="stats">');
  });
});

describe("improveView report card tip", () => {
  const hint = (over: Partial<Parameters<typeof improveView>[0]> = {}, p: Progress = progress()) => view(p, { canGrade: true, ...over });

  it("suggests a report card, with a way to set it up and to dismiss the tip", () => {
    const m = hint();
    expect(m).toContain('<p class="card-hint">Get a letter grade on how you asked, in Claude Code or Codex. ');
    expect(m).toContain('<button class="link" data-action="go-settings" data-id="report-cards">Set up</button>');
    expect(m).toContain('data-action="dismiss-card-hint" aria-label="Dismiss this tip about report cards"');
    // It sits in the trends area, above the charts.
    expect(m.indexOf("card-hint")).toBeGreaterThan(m.indexOf("are trending"));
    expect(m.indexOf("card-hint")).toBeLessThan(m.indexOf("Prompts clear the first time"));
    expect(m).toContain('<h2 id="trends-heading">How ');
  });

  it("says how to ask, with no Set up, once a tool is on", () => {
    const m = hint({ cardsOn: "claude_code" });
    expect(m).toContain('<p class="card-hint">Type /grade:me after your next task.');
    expect(m).not.toContain("Set up");
    expect(m).toContain('data-action="dismiss-card-hint"');
    expect(hint({ cardsOn: "codex" })).toContain("Type $grade-me after your next task.");
  });

  it("stays away once dismissed", () => {
    expect(hint({ hintDismissed: true })).not.toContain("card-hint");
  });

  it("stays away without a source that can carry a report card", () => {
    expect(hint({ canGrade: false })).not.toContain("card-hint");
  });

  it("stays away when any report card exists", () => {
    expect(hint({}, progress({ weeks: weeks(10, 20, (i) => (i === 0 ? cards(1, { grades: { A: 1 } }) : noCards())) }))).not.toContain("card-hint");
    expect(hint({}, progress({ weeks: weeks(40, 20, (i) => (i === 0 ? cards(1, { grades: { A: 1 } }) : noCards())) }))).not.toContain("card-hint");
  });

  it("stays away while a model is picked, since the cards shown would only be that model's", () => {
    expect(hint({ filtered: true })).not.toContain("card-hint");
  });

  it("stays away with no history at all", () => {
    expect(hint({}, progress({ weeks: [] }))).not.toContain("card-hint");
  });
});

describe("canGrade", () => {
  const state = { id: "x", allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] };
  const src = (id: string, allowed: boolean | null): SourceInfo => source({ id, state: allowed == null ? null : { ...state, id, allowed } });

  it("is true when Claude Code, Cowork or Codex is allowed", () => {
    expect(canGrade(meta, [src("claude-code", true)])).toBe(true);
    expect(canGrade(meta, [src("chatgpt-export", true), src("codex", true)])).toBe(true);
  });

  it("is false for sources that can't carry a report card, or aren't allowed", () => {
    expect(canGrade(meta, [src("chatgpt-export", true)])).toBe(false);
    expect(canGrade(meta, [src("claude-code", false), src("codex", null)])).toBe(false);
    expect(canGrade(meta, [])).toBe(false);
  });
});

describe("the grade colors", () => {
  const css = Object.values(import.meta.glob<string>("../style.css", { query: "?raw", import: "default", eager: true })).join("");
  const light = css.slice(0, css.indexOf("@media (prefers-color-scheme: dark)"));
  const dark = css.slice(css.indexOf("@media (prefers-color-scheme: dark)"), css.indexOf("* { box-sizing"));
  const grab = (block: string, name: string) => {
    const hex = block.match(new RegExp(`--${name}:\\s*#([0-9a-f]{6})`))?.[1];
    if (!hex) throw new Error(`no --${name}`);
    return [0, 2, 4].map((i) => Number.parseInt(hex.slice(i, i + 2), 16));
  };
  const lum = ([r, g, b]: number[]) => {
    const lin = (v = 0) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  };
  const contrast = (a: number[], b: number[]) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
  const GRADES = ["grade-a", "grade-b", "grade-c", "grade-d", "grade-f"];

  for (const [theme, block] of [
    ["light", light],
    ["dark", dark],
  ] as const) {
    it(`meets 3:1 against the chart surface in the ${theme} theme, and steps evenly`, () => {
      const surface = grab(theme === "light" ? light : dark, "surface");
      const lums = GRADES.map((g) => lum(grab(block, g)));
      for (const g of GRADES) expect(contrast(grab(block, g), surface), `${g} in ${theme}`).toBeGreaterThanOrEqual(3);
      const steps = lums.slice(1).map((l, i) => Math.abs(l - (lums[i] ?? 0)));
      const [min, max] = [Math.min(...steps), Math.max(...steps)];
      expect(max - min, `${theme} steps ${steps.join(", ")}`).toBeLessThan(0.01);
      expect(
        steps.every((s) => s > 0.03),
        `${theme}: each step can be told apart`,
      ).toBe(true);
    });
  }

  it("has a class for each letter, and none fades its fill", () => {
    for (const g of ["ga", "gb", "gc", "gd", "gf"]) expect(css).toMatch(new RegExp(`\\.seg\\.${g} \\{ fill: var\\(--grade-[a-f]\\)`));
    expect(css).not.toMatch(/\.seg\.g[a-f][^}]*opacity/);
  });
});

describe("improveView tips", () => {
  const empty = { signals: [], steer: null, models: [], report_cards: null };
  const week = (tips: number, tasks: number) => ({ tips, tasks });
  const withTips = (tip_trend: Progress["insights"]["tip_trend"], top_tip: Progress["insights"]["top_tip"], report = noCards) =>
    view(progress({ weeks: weeks(10, 20, report), insights: { ...empty, tip_trend, top_tip } }));

  it("shows how many tasks needed a tip and the tip you get most, even with no report cards", () => {
    const m = withTips({ latest: week(6, 40), before: week(9, 38) }, { habit: "finish", tips: 6, of: 20 });
    expect(m).toContain("Your tips");
    expect(m).toContain("Last week, 6 of 40 tasks got a tip (down from 9 of 38 the week before).");
    expect(m).toContain("Your most common tip: say what done looks like (6 of your last 20).");
  });

  it("compares shares, not counts", () => {
    expect(withTips({ latest: week(3, 30), before: week(2, 20) }, null)).toContain("(same as 2 of 20 the week before)");
    expect(withTips({ latest: week(9, 30), before: week(2, 20) }, null)).toContain("(up from 2 of 20 the week before)");
  });

  it("shows no block with no tips and no report cards", () => {
    expect(withTips(null, null)).not.toContain("Your tips");
  });
});
