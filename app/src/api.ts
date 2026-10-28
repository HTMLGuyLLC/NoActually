// Typed wrappers over the Rust commands in src-tauri/src/lib.rs.
// Outside Tauri (a plain browser) the UI runs as a read-only demo on public/demo.json.
import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";

export const demo = !isTauri();

const demoSources: SourceInfo[] = [
  { id: "claude-code", name: "Claude Code", kind: "live", roots: [["~/.claude/projects", true]], state: { id: "claude-code", allowed: true, paused: false, backfill_since: null, granted_at: null, paths: [] } },
  { id: "codex", name: "Codex", kind: "live", roots: [["~/.codex/sessions", true]], state: { id: "codex", allowed: true, paused: false, backfill_since: null, granted_at: null, paths: [] } },
  { id: "chatgpt-export", name: "ChatGPT (export)", kind: "import", roots: [], state: null },
];

/** The demo narrows its sample numbers by model so the picker visibly does something. */
async function demoStats(filter: Filter): Promise<Stats> {
  const base: Stats = await (await fetch("/demo.json")).json();
  if (!Object.keys(filter).length) return base;
  const keep = base.models.filter(
    (m) => (!filter.provider || m.provider === filter.provider) && (!filter.family || m.family === filter.family) && (!filter.version || m.version === filter.version),
  );
  const catches = keep.reduce((a, m) => a + m.catches, 0);
  const share = base.catches ? catches / base.catches : 0;
  const scale = (n: number) => Math.round(n * share);
  return {
    ...base,
    models: keep,
    catches,
    messages: keep.reduce((a, m) => a + m.messages, 0),
    humbled: keep.reduce((a, m) => a + m.humbled, 0),
    catches_last_7d: scale(base.catches_last_7d),
    weekly: base.weekly.map((w) => ({ ...w, catches: scale(w.catches), messages: scale(w.messages) })),
    categories: Object.fromEntries(Object.entries(base.categories).map(([k, v]) => [k, scale(v)]).filter(([, v]) => (v as number) > 0)),
    hall_of_fame: base.hall_of_fame.filter((h) => keep.some((m) => h.model === (m.version ? `${m.family} ${m.version}` : m.family))),
  };
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!demo) return tauriInvoke<T>(cmd, args);
  if (cmd === "stats") return demoStats((args?.filter ?? {}) as Filter) as Promise<T>;
  if (cmd === "sync_status") return { syncing: false, last_sync: new Date().toISOString(), last_error: null, files_with_errors: 0 } as T;
  if (cmd === "get_autostart") return false as T;
  if (cmd === "list_sources") return demoSources as T;
  if (cmd === "get_retention") return { level: "snippet", full_days: 30, snippet_days: 365 } as T;
  return undefined as T;
}

export type Filter = { source?: string | null; provider?: string | null; family?: string | null; version?: string | null };
export type Level = "full" | "snippet" | "category";
export type Retention = { level: Level; full_days: number; snippet_days: number };

export type ModelRow = { provider: string; family: string; version: string; messages: number; catches: number; humbled: number };
export type HallEntry = {
  id: number; day: string; source: string; model: string; label: string; category: string;
  human: string | null; ai: string | null; pinned: boolean;
};
export type Stats = {
  first_day: string | null; last_day: string | null;
  messages: number; catches: number; humbled: number;
  labels: Record<string, number>;
  catches_last_7d: number; tier: string; next_tier: [string, number] | null;
  messages_per_catch: number | null;
  real_chats: number; pct_real_chats_with_catch: number | null;
  best_streak_days: number; current_streak_days: number; peak_hour: number | null;
  categories: Record<string, number>;
  weekly: { week: string; messages: number; catches: number }[];
  models: ModelRow[];
  hall_of_fame: HallEntry[];
};
export type SourceState = {
  id: string; allowed: boolean; paused: boolean; backfill_since: string | null; granted_at: string | null; paths: string[];
};
export type SyncStatus = { syncing: boolean; last_sync: string | null; last_error: string | null; files_with_errors: number };
export type SourceInfo = { id: string; name: string; kind: "live" | "import"; roots: [string, boolean][]; state: SourceState | null };

export const api = {
  stats: (filter: Filter) => invoke<Stats>("stats", { filter }),
  sources: () => invoke<SourceInfo[]>("list_sources"),
  allow: (id: string, since: string | null, fromNow = false) => invoke<void>("allow_source", { id, since, fromNow }),
  revoke: (id: string) => invoke<void>("revoke_source", { id }),
  forget: (id: string) => invoke<void>("forget_source", { id }),
  pause: (id: string, paused: boolean) => invoke<void>("pause_source", { id, paused }),
  importExport: (id: string) => invoke<string | null>("import_export", { id }),
  retention: () => invoke<Retention>("get_retention"),
  setRetention: (retention: Retention) => invoke<void>("set_retention", { retention }),
  pin: (id: number, pinned: boolean) => invoke<void>("pin", { id, pinned }),
  syncNow: () => invoke<void>("sync_now"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  saveShareCard: (png: number[]) => invoke<string | null>("save_share_card", { png }),
  openShare: (url: string) => (demo ? Promise.resolve(void window.open(url, "_blank", "noopener")) : invoke<void>("open_share", { url })),
  openMain: () => invoke<void>("open_main"),
};
