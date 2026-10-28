// The browser demo's stand-in for the Rust side: sample numbers from public/demo.json, read-only.
import type { ArgsOf, Command, Filter, Meta, ResultOf, SourceInfo, Stats } from "./api";
import { modelName } from "./picker/picker";

export type DemoData = { meta: Meta; stats: Stats };

const isRecord = (x: unknown): x is Record<string, unknown> => typeof x === "object" && x !== null && !Array.isArray(x);

/** A light check that demo.json has the shape the UI reads, so a stale file fails loudly, not oddly. */
export function isDemoData(x: unknown): x is DemoData {
  if (!isRecord(x) || !isRecord(x.meta) || !isRecord(x.stats)) return false;
  const { meta, stats } = x;
  return (
    Array.isArray(meta.tiers) &&
    Array.isArray(meta.categories) &&
    Array.isArray(meta.sources) &&
    typeof stats.tier === "string" &&
    (stats.verdict === "no" || stats.verdict === "hmm") &&
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

/** What Rust would say for this many catches this week: the tier reached, the next one, the verdict. */
export function judge(meta: Meta, catches7d: number): Pick<Stats, "tier" | "next_tier" | "verdict"> {
  const up = [...meta.tiers].sort((a, b) => a.floor - b.floor);
  const reached = up.filter((t) => catches7d >= t.floor).pop();
  const next = up.find((t) => t.floor > catches7d);
  const coworker = up.find((t) => t.id === "coworker")?.floor ?? Number.POSITIVE_INFINITY;
  return {
    tier: reached?.id ?? up[0]?.id ?? "",
    next_tier: next ? [next.id, next.floor - catches7d] : null,
    verdict: catches7d >= coworker ? "no" : "hmm",
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
  const categories: Record<string, number> = {};
  for (const [k, v] of Object.entries(base.categories)) if (scale(v) > 0) categories[k] = scale(v);
  return {
    ...base,
    ...judge(meta, catches_last_7d),
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

const sourceState = (id: string) => ({ id, allowed: true, paused: false, backfill_since: null, granted_at: null, start_after: null, paths: [] });

const demoSources: SourceInfo[] = [
  { id: "claude-code", name: "Claude Code", kind: "live", roots: [["~/.claude/projects", true]], state: sourceState("claude-code") },
  { id: "codex", name: "Codex", kind: "live", roots: [["~/.codex/sessions", true]], state: sourceState("codex") },
  { id: "chatgpt-export", name: "ChatGPT (export)", kind: "import", roots: [], state: null },
];

type Handlers = { [C in Command]: (args: ArgsOf<C>) => ResultOf<C> | Promise<ResultOf<C>> };

/** Every command has a demo answer; changes are accepted and ignored. */
const handlers: Handlers = {
  meta: async () => (await demoData()).meta,
  stats: async ({ filter }) => demoStats(await demoData(), filter),
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
};

export async function demoCall<C extends Command>(cmd: C, args: ArgsOf<C>): Promise<ResultOf<C>> {
  const handler: Handlers[C] = handlers[cmd];
  return handler(args);
}
