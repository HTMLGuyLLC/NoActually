// Display names and ranges from the Rust side's `meta` command. Nothing here hard-codes Rust's tables.
import type { Meta, Tier } from "./api";

/** A tier with the range shown on the ladder: "0–2", "7–14", "15+ catches/wk". */
export type TierRange = Tier & { range: string };

/** Tiers top-down (as the ladder shows them), each with a range derived from the next floor up. */
export function tierRanges(meta: Meta): TierRange[] {
  const up = [...meta.tiers].sort((a, b) => a.floor - b.floor);
  return up
    .map((t, i) => {
      const next = up[i + 1];
      const range = !next ? `${t.floor}+ catches/wk` : next.floor - 1 > t.floor ? `${t.floor}–${next.floor - 1}` : String(t.floor);
      return { ...t, range };
    })
    .reverse();
}

export const tierName = (meta: Meta, id: string): string => meta.tiers.find((t) => t.id === id)?.name ?? id;

/** The highest tier: its floor is the chart's dashed line. */
export const topTier = (meta: Meta): Tier | undefined => meta.tiers.reduce<Tier | undefined>((top, t) => (!top || t.floor > top.floor ? t : top), undefined);

export const categoryTitle = (meta: Meta, id: string): string => meta.categories.find((c) => c.id === id)?.title ?? id;

export const sourceName = (meta: Meta, id: string): string => meta.sources.find((s) => s.id === id)?.name ?? id;
