// Small pure wording helpers shared by the views.
import type { Filter, Meta } from "./api";
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

/** "Claude Code", "Claude Code and Codex", "Claude Code, Codex and ChatGPT". */
export function listNames(names: readonly string[]): string {
  const last = names[names.length - 1]; // not .at(): the webview may be Safari 15.0
  if (last === undefined || names.length < 2) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${last}`;
}

/** The local calendar date as YYYY-MM-DD, matching how the store buckets days (toISOString is UTC). */
export function localIsoDate(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** An error from Rust (a string) or JS (an Error) as one readable line. */
export const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));
