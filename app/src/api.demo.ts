// The browser demo's stand-in for the Rust side: sample numbers from public/demo.json, read-only.
import type { ArgsOf, Briefs, Command, Filter, Goal, GoalStatus, Meta, Overall, Progress, ResultOf, SourceInfo, Stats } from "./api";
import { modelName } from "./picker/picker";
import { periodSince } from "./state";

export type DemoData = { meta: Meta; stats: Stats; progress: Progress };

const isRecord = (x: unknown): x is Record<string, unknown> => typeof x === "object" && x !== null && !Array.isArray(x);

/** A light check that demo.json has the shape the UI reads, so a stale file fails loudly, not oddly. */
export function isDemoData(x: unknown): x is DemoData {
  if (!isRecord(x) || !isRecord(x.meta) || !isRecord(x.stats) || !isRecord(x.progress)) return false;
  const { meta, stats, progress } = x;
  return (
    Array.isArray(meta.tiers) &&
    Array.isArray(meta.categories) &&
    Array.isArray(meta.sources) &&
    isRecord(stats.overall) &&
    (stats.overall.verdict === "no" || stats.overall.verdict === "hmm") &&
    Array.isArray(progress.weeks) &&
    typeof stats.catches === "number" &&
    Array.isArray(stats.models) &&
    Array.isArray(stats.weekly) &&
    Array.isArray(stats.hall_of_fame) &&
    isRecord(stats.categories) &&
    isRecord(stats.briefs)
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

/** What Rust's `Score::of` says for a window (engine/src/tier.rs): the demo's stand-in for it. */
export function judge(meta: Meta, w: { continued: number; landed: number; catches: number; humbled: number }): Overall {
  const base = { catches: w.catches, humbled: w.humbled, to_rank: Math.max(0, meta.min_briefs - w.continued) };
  if (w.continued < meta.min_briefs)
    return { ...base, score: null, direction: null, judgment: null, tier: null, next_tier: null, verdict: "hmm", catch_tier: null, next_catch_tier: null };
  const direction = Math.floor((1000 * Math.min(w.landed, w.continued)) / w.continued);
  const right = Math.max(0, 1000 - Math.floor((meta.humbled_cost * 1000 * Math.max(0, w.humbled)) / Math.max(w.continued, meta.humbled_base)));
  const judgment = Math.floor((right * Math.min(Math.max(0, w.catches), meta.catch_proof)) / meta.catch_proof);
  const score = Math.floor((meta.direction_weight * direction + (100 - meta.direction_weight) * judgment) / 100 / 10);
  const up = [...meta.tiers].sort((a, b) => a.floor - b.floor);
  const tier = up.filter((t) => score >= t.floor).pop();
  const next = up.find((t) => t.floor > score);
  const coworker = up.find((t) => t.id === "coworker")?.floor ?? Number.POSITIVE_INFINITY;
  const j = Math.floor(judgment / 10);
  const catchUp = [...meta.catch_tiers].sort((a, b) => a.floor - b.floor);
  return {
    ...base,
    score,
    direction: Math.floor(direction / 10),
    judgment: j,
    tier: tier?.id ?? null,
    next_tier: next?.id ?? null,
    verdict: score >= coworker ? "no" : "hmm",
    catch_tier: catchUp.filter((t) => j >= t.floor).pop()?.id ?? null,
    next_catch_tier: catchUp.find((t) => t.floor > j)?.id ?? null,
  };
}

/** The demo narrows its sample numbers by model so the picker visibly does something. */
export function demoStats({ meta, stats: base }: DemoData, filter: Filter): Stats {
  if (!Object.values(filter).some((v) => v != null)) return base;
  const keep = base.models.filter(
    (m) =>
      (!filter.provider || m.provider === filter.provider) &&
      (!filter.family || m.family === filter.family) &&
      (!filter.version || m.version === filter.version),
  );
  const catches = keep.reduce((a, m) => a + m.catches, 0);
  const share = base.catches ? catches / base.catches : 0;
  const scale = (n: number) => Math.round(n * share);
  const catches_last_7d = scale(base.catches_last_7d);
  const sum = (k: "tasks" | "continued" | "landed") => keep.reduce((a, m) => a + m[k], 0);
  const taskShare = base.briefs.continued ? sum("continued") / base.briefs.continued : 0;
  const windowTasks = Math.round(base.briefs.window_tasks * taskShare);
  const windowLanded = Math.round(base.briefs.window_landed * taskShare);
  const categories: Record<string, number> = {};
  for (const [k, v] of Object.entries(base.categories)) if (scale(v) > 0) categories[k] = scale(v);
  return {
    ...base,
    overall: judge(meta, {
      continued: windowTasks,
      landed: windowLanded,
      catches: scale(base.overall.catches),
      humbled: Math.round(base.overall.humbled * share),
    }),
    briefs: {
      ...base.briefs,
      tasks: sum("tasks"),
      continued: sum("continued"),
      landed: sum("landed"),
      window_tasks: windowTasks,
      window_landed: windowLanded,
    },
    models: keep,
    catches,
    messages: keep.reduce((a, m) => a + m.messages, 0),
    humbled: keep.reduce((a, m) => a + m.humbled, 0),
    catches_last_7d,
    weekly: base.weekly.map((w) => ({ ...w, catches: scale(w.catches), messages: scale(w.messages) })),
    categories,
    hall_of_fame: base.hall_of_fame.filter((h) => keep.some((m) => h.model === modelName(m))),
  };
}

/**
 * The demo's stand-in for a period: counts shrink to the share of weekly catches since `since`,
 * and the score is worked out again over them, as in Rust.
 */
export function withSince(meta: Meta, s: Stats, since: string | null): Stats {
  if (!since) return s;
  const total = s.weekly.reduce((t, w) => t + w.catches, 0);
  // A week counts when any of its days is on or after `since`.
  const weekly = s.weekly.filter((w) => new Date(`${w.week}T00:00`).getTime() + 6 * 86_400_000 >= new Date(`${since}T00:00`).getTime());
  const share = total ? weekly.reduce((t, w) => t + w.catches, 0) / total : 0;
  const scale = (n: number) => Math.round(n * share);
  const each = (r: Record<string, number>) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, scale(v)]));
  const briefs: Briefs = {
    ...s.briefs,
    tasks: scale(s.briefs.tasks),
    continued: scale(s.briefs.continued),
    landed: scale(s.briefs.landed),
    window_tasks: scale(s.briefs.continued),
    window_landed: scale(s.briefs.landed),
    grades: each(s.briefs.grades),
    signals: each(s.briefs.signals),
    steering: each(s.briefs.steering),
  };
  return {
    ...s,
    first_day: since > (s.first_day ?? "") ? since : s.first_day,
    messages: scale(s.messages),
    catches: scale(s.catches),
    humbled: scale(s.humbled),
    categories: each(s.categories),
    weekly,
    models: s.models.map((m) => ({
      ...m,
      catches: scale(m.catches),
      messages: scale(m.messages),
      humbled: scale(m.humbled),
      tasks: scale(m.tasks),
      continued: scale(m.continued),
      landed: scale(m.landed),
    })),
    briefs,
    overall: judge(meta, { continued: briefs.window_tasks, landed: briefs.window_landed, catches: scale(s.catches), humbled: scale(s.humbled) }),
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
const visit: { learned: string[]; goal: GoalStatus | null | undefined; menuBar: boolean } = { learned: [], goal: undefined, menuBar: true };

const fresh = (goal: Goal): GoalStatus => ({ goal, weeks: [], met: 0, streak: 0, needed: goal.kind === "tier" ? 0 : 4, done: false, need: null, score: null });

/** Every command has a demo answer; changes are accepted and ignored. */
const handlers: Handlers = {
  meta: async () => (await demoData()).meta,
  stats: async ({ filter: { since, ...filter } }) => {
    const data = await demoData();
    // demo.json holds the default 28 days; other periods are scaled from it.
    return withSince(data.meta, demoStats(data, filter), since && since !== periodSince("28d") ? since : null);
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
  sync_status: () => ({ syncing: false, last_sync: new Date().toISOString(), last_error: null, files_with_errors: 0 }),
  get_autostart: () => false,
  set_autostart: () => null,
  save_share_card: () => null,
  open_share: ({ url }) => {
    window.open(url, "_blank", "noopener");
    return null;
  },
  open_main: () => null,
  update_status: () => ({ current: "0.1.0", auto: true, checking: false, installing: false, available: null, last_check: null, last_error: null }),
  set_auto_update: () => null,
  check_for_updates: () => null,
  install_update: () => null,
  progress: async () => {
    const p = (await demoData()).progress;
    return visit.goal === undefined ? p : { ...p, goal: visit.goal };
  },
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
  check_brief: () => {
    throw new Error("The prompt checker runs in the app.");
  },
  learned: () => visit.learned,
  set_learned: ({ id, done }) => {
    visit.learned = [...visit.learned.filter((x) => x !== id), ...(done ? [id] : [])];
    return visit.learned;
  },
  get_menu_bar: () => visit.menuBar,
  set_menu_bar: ({ enabled }) => {
    visit.menuBar = enabled;
    return null;
  },
};

export async function demoCall<C extends Command>(cmd: C, args: ArgsOf<C>): Promise<ResultOf<C>> {
  const handler: Handlers[C] = handlers[cmd];
  return handler(args);
}
