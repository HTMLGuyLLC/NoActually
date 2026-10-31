// Small pure wording helpers shared by the views.
import type { Filter, Meta } from "./api";
import { num } from "./html";
import { sourceName } from "./meta";

/** "just now", "12 min ago", or a clock time; "never" when there was no sync. */
export function ago(iso: string | null, now = Date.now()): string {
  if (!iso) return "never";
  const s = Math.round((now - new Date(iso).getTime()) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  return new Date(iso).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** "ChatGPT (export)" reads as just "ChatGPT" in a sentence. */
export const shortSourceName = (name: string): string => name.replace(/ \(export\)$/, "");

/** What the scorecard is narrowed to: "Claude Opus 5.5", "OpenAI", "Codex", or "AI". */
export function filterLabel(filter: Filter, meta: Meta): string {
  if (filter.version) return `${filter.family ?? ""} ${filter.version}`.trim();
  if (filter.family) return filter.family;
  if (filter.provider) return filter.provider;
  if (filter.source) return shortSourceName(sourceName(meta, filter.source));
  return "AI";
}

export const isFiltered = (filter: Filter): boolean => Object.values(filter).some((v) => v != null);

/** The headline's subject: "All AI" until a model is picked. */
export const subjectLabel = (filter: Filter, meta: Meta): string => (isFiltered(filter) ? filterLabel(filter, meta) : "All AI");

/** Who the questions ask about, in plain words: "AI", or the model the picker narrowed to. */
export const subjectText = (filter: Filter, meta: Meta): string => (isFiltered(filter) ? filterLabel(filter, meta) : "AI");

/** US dollars as a person reads them: "$0.15", "$4.20", "$120", "$1,234". */
export const money = (n: number): string => (n >= 99.995 ? `$${num(Math.round(n))}` : `$${n.toFixed(2)}`);

/** "1 task", "37 times": a count with its noun. */
export const plural = (n: number, one: string, many = `${one}s`): string => `${num(n)} ${n === 1 ? one : many}`;

/** The nth item from the end (1 is the last). Not Array.at: the webview may be Safari 15.0. */
export const last = <T>(xs: readonly T[], n = 1): T | undefined => xs[xs.length - n];

/** "Claude Code", "Claude Code and Codex", "Claude Code, Codex and ChatGPT". */
export function listNames(names: readonly string[]): string {
  const end = last(names);
  if (end === undefined || names.length < 2) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${end}`;
}

/** The local calendar date as YYYY-MM-DD, matching how the store buckets days (toISOString is UTC). */
export function localIsoDate(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** "Oct 2": a day (YYYY-MM-DD) as the person reads it. `opts` changes which parts show. */
export const fmtDay = (iso: string, opts: Intl.DateTimeFormatOptions = { month: "short", day: "numeric" }): string =>
  new Date(`${iso}T00:00`).toLocaleDateString(undefined, opts);

/** An error from Rust (a string) or JS (an Error) as one readable line. */
export const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));
