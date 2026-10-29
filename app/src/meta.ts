// Display names and ranges from the Rust side's `meta` command. Nothing here hard-codes Rust's tables.
import type { Briefs, BriefTier, Meta, Tier } from "./api";

/** A tier with the range shown on the ladder: "0–2", "7–14", "15+ corrections/wk". */
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

/** Prompt tiers top-down: "95%+ landed", "90–94%", … */
export const briefTierRanges = (meta: Meta): (BriefTier & { range: string })[] => withRanges(meta.brief_tiers, (f) => `${f}%+ landed`, "%");

export const briefTier = (meta: Meta, id: string | null): BriefTier | undefined => meta.brief_tiers.find((t) => t.id === id);

/** Correction tiers top-down: "90+", "75–89", … (judgment, out of 100). */
export const catchTierRanges = (meta: Meta): (BriefTier & { range: string })[] => withRanges(meta.catch_tiers, (f) => `${f}+`);

export const catchTier = (meta: Meta, id: string | null): BriefTier | undefined => meta.catch_tiers.find((t) => t.id === id);

export const steerTitle = (meta: Meta, id: string): string => meta.steers.find((s) => s.id === id)?.title ?? id;

/** A whole percent of `n` in `total`; 0 when there's nothing to divide. */
export const percent = (n: number, total: number): number => (total > 0 ? Math.round((100 * n) / total) : 0);

/** Percent of the window's tasks that landed, rounded down like the tier floors (89.7% is 89%, Tech lead). */
export const landedPct = (b: Pick<Briefs, "window_landed" | "window_tasks">): number =>
  b.window_tasks > 0 ? Math.floor((100 * b.window_landed) / b.window_tasks) : 0;

export const tierName = (meta: Meta, id: string | null): string => (id ? (meta.tiers.find((t) => t.id === id)?.name ?? id) : "Not scored yet");

export const tierOf = (meta: Meta, id: string | null): Tier | undefined => meta.tiers.find((t) => t.id === id);

export const signalTitle = (meta: Meta, id: string): string => meta.signals.find((s) => s.id === id)?.title ?? id;

export const categoryTitle = (meta: Meta, id: string): string => meta.categories.find((c) => c.id === id)?.title ?? id;

export const sourceName = (meta: Meta, id: string): string => meta.sources.find((s) => s.id === id)?.name ?? id;
