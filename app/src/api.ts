// Typed wrappers over the Rust commands in src-tauri/src/commands.rs.
// Outside Tauri (a plain browser) the UI runs as a read-only demo on public/demo.json.
import { type InvokeArgs, isTauri, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { demoCall } from "./api.demo";

export const demo = !isTauri();

export type Filter = { source?: string | null; provider?: string | null; family?: string | null; version?: string | null };
export type Level = "full" | "snippet" | "category";
export const LEVELS: readonly Level[] = ["full", "snippet", "category"];
export const isLevel = (x: unknown): x is Level => LEVELS.some((l) => l === x);
export type Retention = { level: Level; full_days: number; snippet_days: number };

/** Static app data from Rust: the one place tiers, category titles and source names are defined. */
export type Tier = { id: string; name: string; floor: number };
export type Meta = {
  /** Ascending by floor. */
  tiers: Tier[];
  categories: { id: string; title: string }[];
  sources: { id: string; name: string }[];
};

export type Verdict = "no" | "hmm";
export type ModelRow = { provider: string; family: string; version: string; messages: number; catches: number; humbled: number };
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
export type WeekRow = { week: string; messages: number; catches: number };
export type Stats = {
  first_day: string | null;
  last_day: string | null;
  messages: number;
  catches: number;
  humbled: number;
  labels: Record<string, number>;
  catches_last_7d: number;
  /** A tier id from `Meta.tiers`. */
  tier: string;
  /** The next tier's id and how many more catches this week reach it. */
  next_tier: [string, number] | null;
  verdict: Verdict;
  messages_per_catch: number | null;
  real_chats: number;
  pct_real_chats_with_catch: number | null;
  best_streak_days: number;
  current_streak_days: number;
  peak_hour: number | null;
  /** Keyed by category id. */
  categories: Record<string, number>;
  weekly: WeekRow[];
  models: ModelRow[];
  hall_of_fame: HallEntry[];
};
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
  stats: { args: { filter: Filter }; result: Stats };
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
  open_main: { args: undefined; result: null };
  update_status: { args: undefined; result: UpdateStatus };
  set_auto_update: { args: { enabled: boolean }; result: null };
  check_for_updates: { args: undefined; result: null };
  install_update: { args: undefined; result: null };
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
  stats: (filter: Filter) => call("stats", { filter }),
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
  openMain: () => call("open_main", undefined),
  updateStatus: () => call("update_status", undefined),
  setAutoUpdate: (enabled: boolean) => call("set_auto_update", { enabled }),
  checkForUpdates: () => call("check_for_updates", undefined),
  installUpdate: () => call("install_update", undefined),
};
