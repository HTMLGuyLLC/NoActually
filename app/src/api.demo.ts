// The browser demo's stand-in for the Rust side: sample numbers from public/demo.json, read-only.
import type { ArgsOf, Command, Filter, Goal, GoalStatus, Meta, Overall, Progress, Prompts, RecentReportCard, ResultOf, SourceInfo, Stats } from "./api";
import { isFiltered, localIsoDate } from "./format";
import { modelName } from "./picker/picker";
import { periodSince } from "./state";

export type DemoData = { meta: Meta; stats: Stats; progress: Progress; recent_report_cards: RecentReportCard[] };

const isRecord = (x: unknown): x is Record<string, unknown> => typeof x === "object" && x !== null && !Array.isArray(x);

/** A light check that demo.json has the shape the UI reads, so a stale file fails loudly, not oddly. */
export function isDemoData(x: unknown): x is DemoData {
  if (!isRecord(x) || !isRecord(x.meta) || !isRecord(x.stats) || !isRecord(x.progress) || !Array.isArray(x.recent_report_cards)) return false;
  const { meta, stats, progress } = x;
  return (
    Array.isArray(meta.tiers) &&
    Array.isArray(meta.categories) &&
    Array.isArray(meta.sources) &&
    Array.isArray(meta.habits) &&
    typeof meta.min_card_week === "number" &&
    isRecord(stats.overall) &&
    (stats.overall.verdict === "no" || stats.overall.verdict === "hmm") &&
    Array.isArray(progress.weeks) &&
    progress.weeks.every((w) => isRecord(w) && isRecord(w.report_cards)) &&
    isRecord(progress.bests) &&
    isRecord(progress.insights) &&
    typeof stats.corrections === "number" &&
    Array.isArray(stats.models) &&
    Array.isArray(stats.weekly) &&
    Array.isArray(stats.hall_of_fame) &&
    isRecord(stats.categories) &&
    isRecord(stats.prompts)
  );
}

let loaded: Promise<DemoData> | null = null;

function demoData(): Promise<DemoData> {
  loaded ??= fetch("/demo.json")
    .then((r) => r.json())
    .then((json: unknown) => {
      if (!isDemoData(json)) throw new Error("demo.json doesn't match the app's data shape");
      return json;
    });
  return loaded;
}

/** Corrections that fully show you check its work over `days`, as Rust's `correction_proof`. */
function correctionProof(meta: Meta, days: number): number {
  const proof = Math.floor((meta.correction_proof * days + meta.prompt_window_days - 1) / meta.prompt_window_days);
  return Math.min(Math.max(proof, 1), meta.correction_proof);
}

/** What Rust's `Score::of` says for a window of `days` days (engine/src/tier.rs): the demo's stand-in for it. */
export function judge(meta: Meta, w: { continued: number; landed: number; corrections: number; humbled: number; days: number }): Overall {
  const base = { corrections: w.corrections, humbled: w.humbled, to_rank: Math.max(0, meta.min_prompts - w.continued) };
  if (w.continued < meta.min_prompts)
    return {
      ...base,
      score: null,
      direction: null,
      judgment: null,
      tier: null,
      next_tier: null,
      verdict: "hmm",
      correction_tier: null,
      next_correction_tier: null,
    };
  const direction = Math.floor((1000 * Math.min(w.landed, w.continued)) / w.continued);
  const right = Math.max(0, 1000 - Math.floor((meta.humbled_cost * 1000 * Math.max(0, w.humbled)) / Math.max(w.continued, meta.humbled_base)));
  const proof = correctionProof(meta, w.days);
  const judgment = Math.floor((right * Math.min(Math.max(0, w.corrections), proof)) / proof);
  const score = Math.floor((meta.direction_weight * direction + (100 - meta.direction_weight) * judgment) / 100 / 10);
  const up = [...meta.tiers].sort((a, b) => a.floor - b.floor);
  const tier = up.filter((t) => score >= t.floor).pop();
  const next = up.find((t) => t.floor > score);
  const coworker = up.find((t) => t.id === "coworker")?.floor ?? Number.POSITIVE_INFINITY;
  const j = Math.floor(judgment / 10);
  const correctionUp = [...meta.correction_tiers].sort((a, b) => a.floor - b.floor);
  return {
    ...base,
    score,
    direction: Math.floor(direction / 10),
    judgment: j,
    tier: tier?.id ?? null,
    next_tier: next?.id ?? null,
    verdict: score >= coworker ? "no" : "hmm",
    correction_tier: correctionUp.filter((t) => j >= t.floor).pop()?.id ?? null,
    next_correction_tier: correctionUp.find((t) => t.floor > j)?.id ?? null,
  };
}

/** The demo narrows its sample numbers by model so the picker visibly does something. */
export function demoStats({ meta, stats: base }: DemoData, filter: Filter): Stats {
  if (!isFiltered(filter)) return base;
  const keep = base.models.filter(
    (m) =>
      (!filter.provider || m.provider === filter.provider) &&
      (!filter.family || m.family === filter.family) &&
      (!filter.version || m.version === filter.version),
  );
  const corrections = keep.reduce((a, m) => a + m.corrections, 0);
  const share = base.corrections ? corrections / base.corrections : 0;
  const scale = (n: number) => Math.round(n * share);
  const corrections_last_7d = scale(base.corrections_last_7d);
  const sum = (k: "tasks" | "continued" | "landed") => keep.reduce((a, m) => a + m[k], 0);
  const taskShare = base.prompts.continued ? sum("continued") / base.prompts.continued : 0;
  const windowTasks = Math.round(base.prompts.window_tasks * taskShare);
  const windowLanded = Math.round(base.prompts.window_landed * taskShare);
  const categories: Record<string, number> = {};
  for (const [k, v] of Object.entries(base.categories)) if (scale(v) > 0) categories[k] = scale(v);
  return {
    ...base,
    overall: judge(meta, {
      continued: windowTasks,
      landed: windowLanded,
      corrections: scale(base.overall.corrections),
      humbled: Math.round(base.overall.humbled * share),
      days: meta.prompt_window_days,
    }),
    prompts: {
      ...base.prompts,
      tasks: sum("tasks"),
      continued: sum("continued"),
      landed: sum("landed"),
      window_tasks: windowTasks,
      window_landed: windowLanded,
    },
    models: keep,
    corrections,
    messages: keep.reduce((a, m) => a + m.messages, 0),
    humbled: keep.reduce((a, m) => a + m.humbled, 0),
    corrections_last_7d,
    weekly: base.weekly.map((w) => ({ ...w, corrections: scale(w.corrections), messages: scale(w.messages) })),
    categories,
    hall_of_fame: base.hall_of_fame.filter((h) => keep.some((m) => h.model === modelName(m))),
  };
}

const dayNumber = (iso: string) => Date.parse(`${iso}T00:00Z`) / 86_400_000;
const isoOfDay = (n: number) => new Date(n * 86_400_000).toISOString().slice(0, 10);

/**
 * The demo's "today" is the sample's last day, not the clock, so its score never ages. A period start
 * the UI worked out from the real clock moves back by the same gap, keeping its length.
 */
export function demoSince(stats: Stats, since: string): string {
  if (!stats.last_day) return since;
  return isoOfDay(dayNumber(since) - (dayNumber(localIsoDate(new Date())) - dayNumber(stats.last_day)));
}

/**
 * The demo's stand-in for a period: counts shrink to the share of weekly corrections since `since`,
 * and the score is worked out again over them, as in Rust.
 */
export function withSince(meta: Meta, s: Stats, since: string | null): Stats {
  if (!since) return s;
  const total = s.weekly.reduce((t, w) => t + w.corrections, 0);
  // A week counts when any of its days is on or after `since`.
  const weekly = s.weekly.filter((w) => new Date(`${w.week}T00:00`).getTime() + 6 * 86_400_000 >= new Date(`${since}T00:00`).getTime());
  const share = total ? weekly.reduce((t, w) => t + w.corrections, 0) / total : 0;
  const scale = (n: number) => Math.round(n * share);
  // The period runs from `since` to the sample's last day; a window with no last day counts as the default.
  const days = s.last_day ? Math.max(1, dayNumber(s.last_day) - dayNumber(since) + 1) : meta.prompt_window_days;
  const each = (r: Record<string, number>) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, scale(v)]));
  const prompts: Prompts = {
    ...s.prompts,
    tasks: scale(s.prompts.tasks),
    continued: scale(s.prompts.continued),
    landed: scale(s.prompts.landed),
    window_tasks: scale(s.prompts.continued),
    window_landed: scale(s.prompts.landed),
    grades: each(s.prompts.grades),
    signals: each(s.prompts.signals),
    steering: each(s.prompts.steering),
  };
  return {
    ...s,
    first_day: since > (s.first_day ?? "") ? since : s.first_day,
    messages: scale(s.messages),
    corrections: scale(s.corrections),
    humbled: scale(s.humbled),
    categories: each(s.categories),
    weekly,
    models: s.models.map((m) => ({
      ...m,
      corrections: scale(m.corrections),
      messages: scale(m.messages),
      humbled: scale(m.humbled),
      tasks: scale(m.tasks),
      continued: scale(m.continued),
      landed: scale(m.landed),
    })),
    prompts,
    overall: judge(meta, {
      continued: prompts.window_tasks,
      landed: prompts.window_landed,
      corrections: scale(s.corrections),
      humbled: scale(s.humbled),
      days,
    }),
  };
}

const sourceState = (id: string) => ({ id, allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] });

const demoSources: SourceInfo[] = [
  { id: "claude-code", name: "Claude Code", kind: "live", roots: [["~/.claude/projects", true]], state: sourceState("claude-code") },
  { id: "codex", name: "Codex", kind: "live", roots: [["~/.codex/sessions", true]], state: sourceState("codex") },
  { id: "chatgpt-export", name: "ChatGPT (export)", kind: "import", roots: [], state: null },
];

type Handlers = { [C in Command]: (args: ArgsOf<C>) => ResultOf<C> | Promise<ResultOf<C>> };

/** The demo keeps what you set in Learn and Improve for the visit, so trying them feels real. */
const visit: {
  learned: string[];
  goal: GoalStatus | null | undefined;
  menuBar: boolean;
  cards: { claude: boolean; tips: boolean; reminders: boolean };
  popups: boolean;
} = {
  learned: [],
  popups: true,
  goal: undefined,
  menuBar: true,
  // Report cards: on in Claude Code, and Codex isn't here.
  cards: { claude: true, tips: false, reminders: true },
};

const fresh = (goal: Goal): GoalStatus => ({ goal, weeks: [], met: 0, streak: 0, needed: goal.kind === "tier" ? 0 : 4, done: false, need: null, score: null });

/** Every command has a demo answer; changes are accepted and ignored. */
const handlers: Handlers = {
  meta: async () => (await demoData()).meta,
  stats: async ({ filter: { since, ...filter } }) => {
    const data = await demoData();
    // demo.json's counts cover all its history (its `overall` is the default 28 days, as with no date in Rust).
    // A period scales them down by weeks, the same for the 28-day default, so a page's counts agree.
    const all = demoStats(data, filter);
    if (since) return withSince(data.meta, all, demoSince(all, since));
    // No date: counts cover everything, and the score covers the last 28 days, as in Rust.
    const today = all.last_day ? new Date(`${all.last_day}T00:00`) : new Date();
    return { ...all, overall: withSince(data.meta, all, periodSince("28d", today)).overall };
  },
  list_sources: () => demoSources,
  allow_source: () => null,
  revoke_source: () => null,
  forget_source: () => null,
  pause_source: () => null,
  import_export: () => null,
  get_retention: () => ({ level: "snippet", full_days: 30, snippet_days: 365 }),
  set_retention: () => null,
  pin: () => null,
  sync_now: () => null,
  sync_status: () => ({ syncing: false, last_sync: new Date().toISOString(), last_error: null, files_with_errors: 0, rebuilding: false }),
  get_autostart: () => false,
  set_autostart: () => null,
  save_share_card: () => null,
  open_share: ({ url }) => {
    window.open(url, "_blank", "noopener");
    return null;
  },
  open_main: () => null,
  update_status: () => ({
    current: "0.1.0",
    auto: true,
    channel: "stable",
    checking: false,
    installing: false,
    available: null,
    last_check: null,
    last_error: null,
  }),
  set_auto_update: () => null,
  set_update_channel: () => null,
  check_for_updates: () => null,
  install_update: () => null,
  progress: async () => {
    const p = (await demoData()).progress;
    return visit.goal === undefined ? p : { ...p, goal: visit.goal };
  },
  recent_report_cards: async () => (await demoData()).recent_report_cards,
  set_goal: ({ kind, target }) => {
    const now = new Date();
    const local = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
    const goal: Goal = { ...kind, id: Date.now(), target, start: local };
    visit.goal = fresh(goal);
    return goal;
  },
  clear_goal: () => {
    visit.goal = null;
    return null;
  },
  // The rules run in Rust; the browser demo can't grade a draft, and says so.
  check_prompt: () => {
    throw new Error("The prompt checker runs in the app.");
  },
  learned: () => visit.learned,
  set_learned: ({ id, done }) => {
    visit.learned = [...visit.learned.filter((x) => x !== id), ...(done ? [id] : [])];
    return visit.learned;
  },
  get_menu_bar: () => ({ enabled: visit.menuBar, available: true }),
  set_menu_bar: ({ enabled }) => {
    visit.menuBar = enabled;
    return null;
  },
  report_cards_status: async () => {
    const days = (await demoData()).recent_report_cards.filter((c) => c.source === "claude-code").map((c) => c.graded_day);
    return {
      claude_code: { installed_tool: true, on: visit.cards.claude, last_card_day: days.sort().pop() ?? null, problem: null, left_out: 0, unavailable: null },
      codex: { installed_tool: false, on: false, last_card_day: null, problem: null, left_out: 0, unavailable: null },
      tips: visit.cards.tips,
      tip_activity: { active: visit.cards.tips ? 6 : 0, tips: visit.cards.tips ? 14 : 0, blocked: false },
      reminders: visit.cards.reminders,
      translocated: false,
    };
  },
  // The demo changes nothing on your computer; it only remembers the choice for the visit.
  report_cards_turn_on: ({ tool }) => {
    if (tool === "claude_code") visit.cards.claude = true;
    return null;
  },
  report_cards_turn_off: ({ tool }) => {
    if (tool === "claude_code") visit.cards.claude = false;
    return null;
  },
  report_cards_set_tips: ({ on }) => {
    visit.cards.tips = on;
    return null;
  },
  report_cards_set_reminders: ({ on }) => {
    visit.cards.reminders = on;
    return null;
  },
  report_cards_delete_history: () => 0,
  log_error: () => null,
  open_log_folder: () => null,
  // Remembered for the visit only, like the other choices.
  get_popups: () => visit.popups,
  set_popups: ({ enabled }) => {
    visit.popups = enabled;
    return null;
  },
  rescored_notice: () => null,
  open_notices: () => null,
};

export async function demoCall<C extends Command>(cmd: C, args: ArgsOf<C>): Promise<ResultOf<C>> {
  const handler: Handlers[C] = handlers[cmd];
  return handler(args);
}
