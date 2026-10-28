// The UI's state: one object, read anywhere, changed only through the named setters below.
import type { Filter, Meta, ModelRow, Stats, SyncStatus, UpdateStatus, Verdict } from "./api";

export const TABS = ["score", "sources", "settings"] as const;
export type Tab = (typeof TABS)[number];
export const isTab = (x: unknown): x is Tab => TABS.some((t) => t === x);

export type AppState = {
  tab: Tab;
  filter: Filter;
  /** Every model, from the last unfiltered stats; the picker lists these even while narrowed. */
  allModels: ModelRow[];
  status: SyncStatus;
  /** The stats last drawn. */
  current: Stats | null;
  update: UpdateStatus | null;
  /** The version the person said "Later" to; its banner stays hidden until a newer one. */
  laterFor: string | null;
  /** The verdict last drawn, so the scrawl only plays when it changes, not on every background redraw. */
  lastVerdict: Verdict | null;
  meta: Meta | null;
};

const initial = (): AppState => ({
  tab: "score",
  filter: {},
  allModels: [],
  status: { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 },
  current: null,
  update: null,
  laterFor: null,
  lastVerdict: null,
  meta: null,
});

const s: AppState = initial();
export const state: Readonly<AppState> = s;

/** The small window under the menu bar icon. */
export const isGlance = typeof location !== "undefined" && location.hash === "#glance";

export function setTab(tab: Tab): void {
  s.tab = tab;
}
export function setFilter(filter: Filter): void {
  s.filter = filter;
}
export function setAllModels(models: ModelRow[]): void {
  s.allModels = models;
}
export function setStatus(status: SyncStatus): void {
  s.status = status;
}
/** A check started: only the status line changes. */
export function setSyncing(): void {
  s.status = { ...s.status, syncing: true };
}
export function setCurrent(stats: Stats): void {
  s.current = stats;
}
export function setUpdate(update: UpdateStatus): void {
  s.update = update;
}
export function setAutoUpdate(auto: boolean): void {
  if (s.update) s.update = { ...s.update, auto };
}
export function setLaterFor(version: string | null): void {
  s.laterFor = version;
}
export function setLastVerdict(verdict: Verdict): void {
  s.lastVerdict = verdict;
}
export function setMeta(meta: Meta): Meta {
  s.meta = meta;
  return meta;
}

/** Tests only: back to a fresh launch. */
export function resetState(): void {
  Object.assign(s, initial());
}
