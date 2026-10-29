import { describe, expect, it } from "vitest";
import type { Goal, GoalStatus, GoalWeek, HistoryWeek, Progress } from "../api";
import { HOSTILE, meta, progress, stats, week } from "../test/fixtures";
import { buckets, goalTitle, improveView } from "./improve";

const safe = (markup: string) => {
  expect(markup).not.toContain("<img");
  expect(markup).not.toContain('onerror="');
};
const count = (m: string, needle: string) => m.split(needle).length - 1;

/** `n` Mondays starting 2026-08-03, each with `tasks` tasks. */
const weeks = (n: number, tasks = 20): HistoryWeek[] => {
  const start = Date.UTC(2026, 7, 3);
  return Array.from({ length: n }, (_, i) =>
    week(new Date(start + i * 7 * 86_400_000).toISOString().slice(0, 10), { tasks, continued: Math.ceil(tasks / 2), landed: Math.floor(tasks / 3) }),
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
    expect(goalTitle({ kind: "habit", signal: "why", target: 40 }, meta)).toBe("Say why in 40% of your prompts each week");
    expect(goalTitle({ kind: "steer", steer: "redo", target: 10 }, meta)).toBe("Keep “you started over” to 10% of your tasks or fewer each week");
    expect(goalTitle({ kind: "tier", tier: "coworker", target: 0 }, meta)).toBe("Reach Coworker");
  });

  it("falls back to the id for anything unknown", () => {
    expect(goalTitle({ kind: "habit", signal: "new_one", target: 50 }, meta)).toContain("New_one in 50% of your prompts each week");
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
    expect(m).toContain("Set limits in 30% of your prompts each week");
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
    expect(m).toMatch(/<option value="[^"]*limits[^"]*" selected>Set limits in 30%/);
  });

  it("shows a goal under way with a mark and spoken text for each week", () => {
    const m = view(progress({ goal: status() }));
    expect(m).toContain("Your goal · since");
    expect(m).toContain("Say why in 40% of your prompts each week");
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
    expect(view(progress({ goal: status({ need: 2 }) }))).toContain("12 prompts so far. 2 more prompts that say why would hit it.");
    expect(view(progress({ goal: status({ need: 1 }) }))).toContain("1 more prompt that say why would hit it.");
    expect(view(progress({ goal: status({ need: 0 }) }))).toContain("12 prompts so far. On track.");
  });

  it("mentions a streak only past one week", () => {
    expect(view(progress({ goal: status({ streak: 3 }) }))).toContain("3 in a row");
    expect(view(progress({ goal: status({ streak: 1 }) }))).not.toContain("in a row");
  });

  it("measures a steering goal in tasks, and is on track under its ceiling", () => {
    const st = status({ goal: goal({ kind: "steer", steer: "redo", target: 10 } as Partial<Goal>), weeks: [gw("2026-10-26", "open", 5, 20)], need: null });
    const m = view(progress({ goal: st }));
    expect(m).toContain("Keep “you started over” to 10% of your tasks or fewer each week");
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
    expect(m).toContain("Your score is 70. Coworker starts at 65.");
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

  it("switches to months when weeks are thin", () => {
    const m = view(progress({ weeks: weeks(12, 2) }));
    expect(m).toContain("Per month.");
    expect(m).toContain("per month");
  });

  it("summarises the landed trend, or says there isn't enough to show one", () => {
    expect(view(progress({ weeks: weeks(10) }))).toMatch(/Tasks landed went from 60% \(.+\) to 60% \(.+\)\./);
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
    expect(m).toContain("<span>Tasks landed</span><b>75%</b>");
    expect(m).toContain("▼ 16");
    expect(m).toContain("<span>Sharp prompts</span><b>25%</b>");
    expect(m).toContain("<span>Corrections</span><b>8</b>");
  });

  it("marks a rise with an up arrow", () => {
    const p = progress({ weeks: [week("2026-10-05", { landed: 6 }), week("2026-10-12", { landed: 9 }), week("2026-10-19")] });
    expect(view(p)).toContain("▲ 25");
  });

  it("waits for a full week of tasks before comparing", () => {
    const p = progress({ weeks: [week("2026-10-05"), week("2026-10-12", { tasks: 2, continued: 1, landed: 1 }), week("2026-10-19")] });
    expect(view(p)).toContain("Your weekly recap shows up once a full week has 5+ tasks.");
    expect(view(progress({ weeks: [week("2026-10-05"), week("2026-10-12")] }))).toContain("Your weekly recap shows up");
  });

  it("shows the bests, with dashes before there are any", () => {
    const m = view(progress());
    expect(m).toContain('<span class="v">91%</span><span class="l">best 4 weeks landed · to ');
    expect(m).toContain('<span class="v">40%</span><span class="l">sharpest week · ');
    expect(m).toContain('<span class="v">0 weeks</span><span class="l">longest goal streak</span>');
    const none = view(progress({ bests: { landed: null, sharp: null, goal_streak: 1 } }));
    expect(none).toContain('<span class="v">–</span><span class="l">best 4 weeks landed</span>');
    expect(none).toContain('<span class="v">1 week</span>');
  });

  it("shelves trophies by their goal, and counts them", () => {
    const t = { goal: goal(), done: "2026-10-01" };
    expect(view(progress())).not.toContain("Trophy shelf");
    expect(view(progress())).toContain('<span class="l">trophies</span>');
    const m = view(progress({ trophies: [t] }));
    expect(m).toContain("Trophy shelf");
    expect(m).toContain("<b>Say why in 40% of your prompts each week</b>");
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
    const m = view(progress({ insights: { signals, steer: null, models: [] } }));
    expect(m).toContain('<ul class="insights">');
    expect(m).toContain("Prompts that say why landed <b>93%</b> of the time, vs 80% without.");
    expect(m).toContain('data-action="open-lesson" data-id="say-why"');
    expect(count(m, "<li>Prompts that")).toBe(1);
    expect(m).not.toContain("Not enough history yet.");
  });

  it("reports the steering that was on you, with its lesson", () => {
    const m = view(progress({ insights: { signals: [], steer: ["added", 60], models: [] } }));
    expect(m).toContain("Most of the steering that was on you is <b>“You added more”</b> (60%).");
    expect(m).toContain('data-action="open-lesson" data-id="say-it-all-up-front"');
  });

  it("reports how models compare only when there are two", () => {
    const models = [
      { family: "Claude Opus", landed: 90, continued: 40 },
      { family: "GPT Astra", landed: 70, continued: 30 },
    ];
    expect(view(progress({ insights: { signals: [], steer: null, models } }))).toContain(
      "Your tasks land <b>90%</b> of the time with Claude Opus, and 70% with GPT Astra.",
    );
    expect(view(progress({ insights: { signals: [], steer: null, models: models.slice(0, 1) } }))).toContain("Not enough history yet.");
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
      },
      suggestion: { kind: "habit", signal: HOSTILE, target: 30, id: 0, start: "2026-10-28" },
    });
    safe(improveView({ meta: bad, stats: stats(), progress: p, range: "12w", subject: HOSTILE, filtered: true }).value);
    safe(improveView({ meta: bad, stats: stats(), progress: { ...p, goal: null }, range: "all", subject: HOSTILE, filtered: false }).value);
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
    expect(habit).toContain("goal-hero");
  });
});
