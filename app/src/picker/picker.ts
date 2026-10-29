// Model picker logic, kept pure so it can be tested without a window.
import type { Filter, ModelRow } from "../api";
import { floorPercent } from "../meta";

/** What a scope's numbers add up to: corrections and messages, and tasks for the prompt score. */
export type Counts = { corrections: number; messages: number; tasks: number; continued: number; landed: number };
export type PickItem = Counts & { filter: Filter; label: string; path: string; depth: number };
/** Which numbers the picker shows beside each model: the ones the page is about. */
export type PickStat = "corrections" | "prompts" | "both";

export const modelName = (m: { family: string; version: string }) => (m.version ? `${m.family} ${m.version}` : m.family);

const total = (rows: ModelRow[]): Counts =>
  rows.reduce(
    (a, m) => ({
      corrections: a.corrections + m.corrections,
      messages: a.messages + m.messages,
      tasks: a.tasks + m.tasks,
      continued: a.continued + m.continued,
      landed: a.landed + m.landed,
    }),
    { corrections: 0, messages: 0, tasks: 0, continued: 0, landed: 0 },
  );

/** Every pickable scope, most-corrected first: all models, each provider, each family, each version. */
export function buildPickItems(models: ModelRow[]): PickItem[] {
  const all = total(models);
  const items: PickItem[] = [{ filter: {}, label: "All AI", path: "Every model", depth: 0, ...all }];
  const byCorrections = (rows: ModelRow[]) => (a: string, b: string) =>
    total(rows.filter((m) => m.provider === b || m.family === b)).corrections - total(rows.filter((m) => m.provider === a || m.family === a)).corrections;
  const providers = [...new Set(models.map((m) => m.provider))].sort(byCorrections(models));
  for (const p of providers) {
    const pRows = models.filter((m) => m.provider === p);
    items.push({ filter: { provider: p }, label: p, path: "All models", depth: 0, ...total(pRows) });
    const families = [...new Set(pRows.map((m) => m.family))].sort(byCorrections(pRows));
    for (const fam of families) {
      const fRows = pRows.filter((m) => m.family === fam);
      const versions = fRows.filter((m) => m.version).sort((a, b) => b.corrections - a.corrections);
      // A provider whose only family has its own name ("No model recorded") is already listed.
      if (families.length === 1 && fam === p) {
        for (const v of versions) {
          items.push({
            filter: { provider: p, family: fam, version: v.version },
            label: modelName(v),
            path: p,
            depth: 1,
            ...total([v]),
          });
        }
        continue;
      }
      // A family with one version and nothing else is just that model: no extra level.
      const v = versions[0];
      if (v && versions.length === 1 && fRows.length === 1) {
        items.push({
          filter: { provider: p, family: fam, version: v.version },
          label: modelName(v),
          path: p,
          depth: 1,
          ...total([v]),
        });
        continue;
      }
      items.push({
        filter: { provider: p, family: fam },
        label: fam,
        path: versions.length ? `${p} · all versions` : p,
        depth: 1,
        ...total(fRows),
      });
      for (const v of versions) {
        items.push({
          filter: { provider: p, family: fam, version: v.version },
          label: modelName(v),
          path: `${p} · ${fam}`,
          depth: 2,
          ...total([v]),
        });
      }
    }
  }
  return items;
}

/** Every word must appear in the label or its path: "opus 5.5", "gpt mini", "anthropic". */
export function matchPickItems(items: PickItem[], query: string): PickItem[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return items;
  return items.filter((i) => words.every((w) => `${i.label} ${i.path}`.toLowerCase().includes(w)));
}

export const sameFilter = (a: Filter, b: Filter) => JSON.stringify(a) === JSON.stringify(b);

/** "83%" of continued tasks that landed (rounded down, like the tier floors), or "–" with none. */
export const landedRate = (landed: number, continued: number) => (continued ? `${floorPercent(landed, continued)}%` : "–");

/** "12.5%" of messages that were a correction, or "–" when there were none. */
export const correctionRate = (corrections: number, messages: number) => (messages ? `${((100 * corrections) / messages).toFixed(1)}%` : "–");
