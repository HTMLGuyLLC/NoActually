// Display names and ranges from the Rust side's `meta` command. Nothing here hard-codes Rust's tables.
import type { Meta, Prompts, PromptTier, Tier } from "./api";

/** A tier with the range shown on the ladder: "0–49", "50–74", "90+". */
export type TierRange = Tier & { range: string };

/** Tiers top-down (as a ladder shows them), each with a range up to the next floor; `top` labels the last. */
function withRanges<T extends Tier>(tiers: T[], top: (floor: number) => string, unit = ""): (T & { range: string })[] {
  const up = [...tiers].sort((a, b) => a.floor - b.floor);
  return up
    .map((t, i) => {
      const next = up[i + 1];
      const range = !next ? top(t.floor) : next.floor - 1 > t.floor ? `${t.floor}–${next.floor - 1}${unit}` : `${t.floor}${unit}`;
      return { ...t, range };
    })
    .reverse();
}

/** Overall tiers top-down: "90+", "80–89", … */
export const tierRanges = (meta: Meta): TierRange[] => withRanges(meta.tiers, (f) => `${f}+`);

/** Prompt tiers top-down: "90%+ clear", "80–89%", … */
export const promptTierRanges = (meta: Meta): (PromptTier & { range: string })[] => withRanges(meta.prompt_tiers, (f) => `${f}%+ clear`, "%");

export const promptTier = (meta: Meta, id: string | null): PromptTier | undefined => meta.prompt_tiers.find((t) => t.id === id);

/** Correction tiers top-down: "90+", "75–89", … (judgment, out of 100). */
export const correctionTierRanges = (meta: Meta): (PromptTier & { range: string })[] => withRanges(meta.correction_tiers, (f) => `${f}+`);

export const correctionTier = (meta: Meta, id: string | null): PromptTier | undefined => meta.correction_tiers.find((t) => t.id === id);

export const steerTitle = (meta: Meta, id: string): string => meta.steers.find((s) => s.id === id)?.title ?? id;

/** A whole percent of `n` in `total`; 0 when there's nothing to divide. */
export const percent = (n: number, total: number): number => (total > 0 ? Math.round((100 * n) / total) : 0);

/** The same, rounded down like the tier floors (89.7% is 89%, Tech lead). */
export const floorPercent = (n: number, total: number): number => (total > 0 ? Math.floor((100 * n) / total) : 0);

/** Percent of the window's prompts that were clear the first time, rounded down. */
export const landedPct = (b: Pick<Prompts, "window_landed" | "window_tasks">): number => floorPercent(b.window_landed, b.window_tasks);

export const tierOf = (meta: Meta, id: string | null): Tier | undefined => meta.tiers.find((t) => t.id === id);

export const tierName = (meta: Meta, id: string | null): string => (id ? (tierOf(meta, id)?.name ?? id) : "Not scored yet");

export const gradeTitle = (meta: Meta, id: string): string => meta.grades.find((g) => g.id === id)?.title ?? id;

/** Whether the checker found signal number `i` (the nth in `meta.signals`) in a prompt: bit `i` of `bits`. */
export const hasSignal = (bits: number, i: number): boolean => (bits & (1 << i)) !== 0;

export const signalTitle = (meta: Meta, id: string): string => meta.signals.find((s) => s.id === id)?.title ?? id;

/** A signal as an object: "a target", "evidence", "a purpose", for "Include a purpose" and "prompts with a purpose". */
const SIGNAL_NOUN: Record<string, string> = { spot: "a target", proof: "evidence", done: "a finish line", limits: "guardrails", why: "a purpose" };
export const signalNoun = (meta: Meta, id: string): string => SIGNAL_NOUN[id] ?? signalTitle(meta, id).toLowerCase();

export const categoryTitle = (meta: Meta, id: string): string => meta.categories.find((c) => c.id === id)?.title ?? id;

export const sourceName = (meta: Meta, id: string): string => meta.sources.find((s) => s.id === id)?.name ?? id;
