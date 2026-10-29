// Typed wrappers over the Rust commands in src-tauri/src/commands.rs.
// Outside Tauri (a plain browser) the UI runs as a read-only demo on public/demo.json.
import { type InvokeArgs, isTauri, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { demoCall } from "./api.demo";
import type { Tab } from "./state";

export const demo = !isTauri();

export type Filter = { source?: string | null; provider?: string | null; family?: string | null; version?: string | null };
/** A filter plus a window: only days from `since` (local YYYY-MM-DD) on. Counts follow it; the score doesn't. */
export type StatsFilter = Filter & { since?: string | null };
export type Level = "full" | "snippet" | "category";
export const LEVELS: readonly Level[] = ["full", "snippet", "category"];
export const isLevel = (x: unknown): x is Level => LEVELS.some((l) => l === x);
export type Retention = { level: Level; full_days: number; snippet_days: number };

/** Static app data from Rust: the one place tiers, category titles and source names are defined. */
export type Tier = { id: string; name: string; floor: number };
/** A prompt tier: `floor` is the percent of the week's tasks that must land. */
export type PromptTier = Tier & { verdict: string };
export type Titled = { id: string; title: string };
export type Meta = {
  /** Overall tiers, ascending by floor (an overall score, 0 to 100). */
  tiers: Tier[];
  /** Percent of the overall score from landed tasks; the rest is who corrects whom. */
  direction_weight: number;
  /** Corrections in the window that fully show you check its work. */
  correction_proof: number;
  /** Judgment points lost per 1% of tasks the AI had to correct you on. */
  humbled_cost: number;
  /** Fewest tasks corrections are counted against (so a thin month isn't wiped out). */
  humbled_base: number;
  categories: Titled[];
  sources: { id: string; name: string }[];
  /** Ascending by floor. */
  prompt_tiers: PromptTier[];
  /** "Do you correct it?" tiers, by judgment, ascending. */
  correction_tiers: PromptTier[];
  /** Tasks with a follow-up, in the window, before a prompt tier is given. */
  min_prompts: number;
  /** Days of tasks a prompt tier looks at. */
  prompt_window_days: number;
  /** What a prompt can carry, in display order. */
  signals: (Titled & { hint: string })[];
  /** Signals a prompt needs to be sharp. */
  sharp_signals: number;
  /** Lowest first. */
  grades: Titled[];
  /** Why a task was steered, in display order. */
  steers: Titled[];
  /** Prompts a week needs before it counts toward a chart point, goal or best. */
  min_week: number;
  /** Weeks a habit or steering goal must be met. */
  goal_weeks: number;
};

export type Verdict = "no" | "hmm";
export type ModelRow = {
  provider: string;
  family: string;
  version: string;
  messages: number;
  corrections: number;
  humbled: number;
  /** Tasks given to it, the ones that continued, and of those the ones that landed. */
  tasks: number;
  continued: number;
  landed: number;
};
export type HallEntry = {
  id: number;
  day: string;
  source: string;
  source_name: string;
  model: string;
  label: string;
  category: string;
  category_title: string;
  human: string | null;
  ai: string | null;
  pinned: boolean;
};
/** One week: messages and corrections, the tasks started that week, and the breakdowns behind both. */
export type WeekRow = {
  week: string;
  messages: number;
  corrections: number;
  continued: number;
  landed: number;
  humbled: number;
  /** Corrections by category id. */
  categories: Record<string, number>;
  /** Steering on the week's tasks by cause id. */
  steering: Record<string, number>;
};
/** The signal your prompts carry least; the with/without rates only when your history backs them. */
export type Tip = { signal: string; pct_of_prompts: number; landed_with_without: [number, number] | null };
/**
 * How you prompt it. A task "landed" when you never had to add to, clarify or redo it. Only tasks
 * with a follow-up (`continued`) count toward landing: a one-shot task can't need steering.
 */
export type Prompts = {
  tasks: number;
  continued: number;
  landed: number;
  /** Continued tasks in the last `Meta.prompt_window_days` days, and how many of them landed. */
  window_tasks: number;
  window_landed: number;
  /** A prompt tier id from `Meta.prompt_tiers`, or null until there are enough tasks. */
  tier: string | null;
  /** The tier above it; its floor is the target to show. */
  next_tier: string | null;
  /** More tasks with a follow-up before a tier is given (0 once ranked). */
  to_rank: number;
  /** Keyed by grade id. */
  grades: Record<string, number>;
  /** Prompts carrying each signal, keyed by signal id. */
  signals: Record<string, number>;
  /** Steering messages, keyed by steer id. */
  steering: Record<string, number>;
  tip: Tip | null;
};
/**
 * "Could AI replace you?": an overall score over the last `Meta.prompt_window_days` days, mixing how
 * often your tasks land (`direction`) with who corrects whom (`judgment`). Score fields are null
 * until there are enough tasks with a follow-up.
 */
export type Overall = {
  score: number | null;
  direction: number | null;
  judgment: number | null;
  /** A tier id from `Meta.tiers`. */
  tier: string | null;
  next_tier: string | null;
  verdict: Verdict;
  /** A correction tier id from `Meta.correction_tiers`, from judgment. */
  correction_tier: string | null;
  next_correction_tier: string | null;
  /** Corrections and times it corrected you, in the window. */
  corrections: number;
  humbled: number;
  to_rank: number;
};
export type Stats = {
  first_day: string | null;
  last_day: string | null;
  messages: number;
  corrections: number;
  humbled: number;
  labels: Record<string, number>;
  corrections_last_7d: number;
  overall: Overall;
  messages_per_correction: number | null;
  real_chats: number;
  pct_real_chats_with_correction: number | null;
  best_streak_days: number;
  current_streak_days: number;
  peak_hour: number | null;
  /** Keyed by category id. */
  categories: Record<string, number>;
  weekly: WeekRow[];
  models: ModelRow[];
  hall_of_fame: HallEntry[];
  prompts: Prompts;
};
/** One week of history (the week's Monday, local time). */
export type HistoryWeek = {
  week: string;
  messages: number;
  corrections: number;
  humbled: number;
  tasks: number;
  continued: number;
  landed: number;
  /** Prompts carrying each signal; prompts per grade; steering messages and steered tasks per cause. */
  signals: Record<string, number>;
  grades: Record<string, number>;
  steering: Record<string, number>;
  steered: Record<string, number>;
};
export type GoalKind = { kind: "habit"; signal: string } | { kind: "steer"; steer: string } | { kind: "tier"; tier: string };
/** `target` is a percent: a floor for a habit, a ceiling for steering, unused for a tier. */
export type Goal = GoalKind & { id: number; target: number; start: string };
export type WeekState = "open" | "met" | "missed" | "paused";
export type GoalWeek = { week: string; count: number; value: number; state: WeekState };
export type GoalStatus = {
  goal: Goal;
  weeks: GoalWeek[];
  met: number;
  streak: number;
  /** Weeks it takes (0 for a tier goal). */
  needed: number;
  done: boolean;
  /** Habit goals: more prompts carrying the signal this week would reach the target (null when out of reach this week). */
  need: number | null;
  /** Tier goals: the overall score now. */
  score: number | null;
};
export type Trophy = { goal: Goal; done: string };
export type Progress = {
  weeks: HistoryWeek[];
  goal: GoalStatus | null;
  suggestion: Goal;
  /** Every goal you could pick now, with targets from your own numbers (all AI). */
  options: Goal[];
  trophies: Trophy[];
  bests: { landed: [number, string] | null; sharp: [number, string] | null; goal_streak: number };
  insights: {
    signals: Tip[];
    /** [steer id, its share of the steering that was on you]. */
    steer: [string, number] | null;
    models: { family: string; landed: number; continued: number }[];
  };
};
/** A draft prompt, graded like your history. `signals` is a bit mask, in `Meta.signals` order. */
export type Check = { signals: number; grade: string; rules: string[]; not_a_prompt: boolean };
export type SourceState = {
  id: string;
  allowed: boolean;
  paused: boolean;
  backfill_since: string | null;
  granted_at: string | null;
  /** "From now on": messages written before this moment are ignored. */
  start_after: string | null;
  paths: string[];
};
export type SyncStatus = { syncing: boolean; last_sync: string | null; last_error: string | null; files_with_errors: number };
export type UpdateStatus = {
  current: string;
  auto: boolean;
  checking: boolean;
  installing: boolean;
  available: string | null;
  last_check: string | null;
  last_error: string | null;
};
export type SourceInfo = { id: string; name: string; kind: "live" | "import"; roots: [string, boolean][]; state: SourceState | null };

/** Every command the UI calls: its arguments (camelCase, as Tauri expects) and what it returns. */
export type Commands = {
  meta: { args: undefined; result: Meta };
  stats: { args: { filter: StatsFilter }; result: Stats };
  list_sources: { args: undefined; result: SourceInfo[] };
  allow_source: { args: { id: string; since: string | null; fromNow: boolean }; result: null };
  revoke_source: { args: { id: string }; result: null };
  forget_source: { args: { id: string }; result: null };
  pause_source: { args: { id: string; paused: boolean }; result: null };
  import_export: { args: { id: string }; result: string | null };
  get_retention: { args: undefined; result: Retention };
  set_retention: { args: { retention: Retention }; result: null };
  pin: { args: { id: number; pinned: boolean }; result: null };
  sync_now: { args: undefined; result: null };
  sync_status: { args: undefined; result: SyncStatus };
  get_autostart: { args: undefined; result: boolean };
  set_autostart: { args: { enabled: boolean }; result: null };
  save_share_card: { args: { png: number[] }; result: string | null };
  open_share: { args: { url: string }; result: null };
  open_main: { args: { tab: Tab | null }; result: null };
  update_status: { args: undefined; result: UpdateStatus };
  set_auto_update: { args: { enabled: boolean }; result: null };
  check_for_updates: { args: undefined; result: null };
  install_update: { args: undefined; result: null };
  progress: { args: { filter: Filter }; result: Progress };
  set_goal: { args: { kind: GoalKind; target: number }; result: Goal };
  clear_goal: { args: undefined; result: null };
  check_prompt: { args: { text: string }; result: Check };
  learned: { args: undefined; result: string[] };
  set_learned: { args: { id: string; done: boolean }; result: string[] };
  get_menu_bar: { args: undefined; result: boolean };
  set_menu_bar: { args: { enabled: boolean }; result: null };
};
export type Command = keyof Commands;
export type ArgsOf<C extends Command> = Commands[C]["args"];
export type ResultOf<C extends Command> = Commands[C]["result"];

function call<C extends Command>(cmd: C, args: ArgsOf<C>): Promise<ResultOf<C>> {
  if (demo) return demoCall(cmd, args);
  const sent: InvokeArgs | undefined = args;
  return tauriInvoke<ResultOf<C>>(cmd, sent);
}

export const api = {
  meta: () => call("meta", undefined),
  stats: (filter: StatsFilter) => call("stats", { filter }),
  sources: () => call("list_sources", undefined),
  allow: (id: string, since: string | null, fromNow: boolean) => call("allow_source", { id, since, fromNow }),
  revoke: (id: string) => call("revoke_source", { id }),
  forget: (id: string) => call("forget_source", { id }),
  pause: (id: string, paused: boolean) => call("pause_source", { id, paused }),
  importExport: (id: string) => call("import_export", { id }),
  retention: () => call("get_retention", undefined),
  setRetention: (retention: Retention) => call("set_retention", { retention }),
  pin: (id: number, pinned: boolean) => call("pin", { id, pinned }),
  syncNow: () => call("sync_now", undefined),
  syncStatus: () => call("sync_status", undefined),
  getAutostart: () => call("get_autostart", undefined),
  setAutostart: (enabled: boolean) => call("set_autostart", { enabled }),
  saveShareCard: (png: number[]) => call("save_share_card", { png }),
  openShare: (url: string) => call("open_share", { url }),
  openMain: (tab: Tab | null = null) => call("open_main", { tab }),
  updateStatus: () => call("update_status", undefined),
  setAutoUpdate: (enabled: boolean) => call("set_auto_update", { enabled }),
  checkForUpdates: () => call("check_for_updates", undefined),
  installUpdate: () => call("install_update", undefined),
  progress: (filter: Filter) => call("progress", { filter }),
  setGoal: (kind: GoalKind, target: number) => call("set_goal", { kind, target }),
  clearGoal: () => call("clear_goal", undefined),
  checkPrompt: (text: string) => call("check_prompt", { text }),
  learned: () => call("learned", undefined),
  setLearned: (id: string, done: boolean) => call("set_learned", { id, done }),
  getMenuBar: () => call("get_menu_bar", undefined),
  setMenuBar: (enabled: boolean) => call("set_menu_bar", { enabled }),
};
