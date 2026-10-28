// Model picker logic, kept pure so it can be tested without a window.
import type { Filter, ModelRow } from "../api";

export type PickItem = { filter: Filter; label: string; path: string; depth: number; catches: number; messages: number };

export const modelName = (m: { family: string; version: string }) => (m.version ? `${m.family} ${m.version}` : m.family);

const total = (rows: ModelRow[]) => rows.reduce((a, m) => ({ c: a.c + m.catches, m: a.m + m.messages }), { c: 0, m: 0 });

/** Every pickable scope, most-caught first: all models, each provider, each family, each version. */
export function buildPickItems(models: ModelRow[]): PickItem[] {
  const all = total(models);
  const items: PickItem[] = [{ filter: {}, label: "All AI", path: "Every model", depth: 0, catches: all.c, messages: all.m }];
  const byCatches = (rows: ModelRow[]) => (a: string, b: string) => total(rows.filter((m) => m.provider === b || m.family === b)).c - total(rows.filter((m) => m.provider === a || m.family === a)).c;
  const providers = [...new Set(models.map((m) => m.provider))].sort(byCatches(models));
  for (const p of providers) {
    const pRows = models.filter((m) => m.provider === p);
    const pt = total(pRows);
    items.push({ filter: { provider: p }, label: p, path: "All models", depth: 0, catches: pt.c, messages: pt.m });
    const families = [...new Set(pRows.map((m) => m.family))].sort(byCatches(pRows));
    for (const fam of families) {
      const fRows = pRows.filter((m) => m.family === fam);
      const versions = fRows.filter((m) => m.version).sort((a, b) => b.catches - a.catches);
      // A provider whose only family has its own name ("No model recorded") is already listed.
      if (families.length === 1 && fam === p) {
        for (const v of versions) {
          items.push({ filter: { provider: p, family: fam, version: v.version }, label: modelName(v), path: p, depth: 1, catches: v.catches, messages: v.messages });
        }
        continue;
      }
      // A family with one version and nothing else is just that model: no extra level.
      const v = versions[0];
      if (v && versions.length === 1 && fRows.length === 1) {
        items.push({ filter: { provider: p, family: fam, version: v.version }, label: modelName(v), path: p, depth: 1, catches: v.catches, messages: v.messages });
        continue;
      }
      const ft = total(fRows);
      items.push({ filter: { provider: p, family: fam }, label: fam, path: versions.length ? `${p} · all versions` : p, depth: 1, catches: ft.c, messages: ft.m });
      for (const v of versions) {
        items.push({ filter: { provider: p, family: fam, version: v.version }, label: modelName(v), path: `${p} · ${fam}`, depth: 2, catches: v.catches, messages: v.messages });
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

/** "12.5%" of messages that were a catch, or "–" when there were none. */
export const catchRate = (catches: number, messages: number) => (messages ? `${((100 * catches) / messages).toFixed(1)}%` : "–");
