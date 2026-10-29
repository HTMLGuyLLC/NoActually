// The UI's state: one object, read anywhere, changed only through the named setters below.
import type { Filter, Meta, ModelRow, Stats, SyncStatus, UpdateStatus, Verdict } from "./api";

export const TABS = ["score", "improve", "learn", "settings"] as const;
export type Tab = (typeof TABS)[number];
export const isTab = (x: unknown): x is Tab => TABS.some((t) => t === x);

/** The scorecard's sections, each its own sub-tab. */
export const SCORE_VIEWS = ["overview", "briefs", "catches", "hall"] as const;
export type ScoreView = (typeof SCORE_VIEWS)[number];
export const isScoreView = (x: unknown): x is ScoreView => SCORE_VIEWS.some((v) => v === x);

/** How far back the scorecard's counts look. The score and tiers always use their own 28 days. */
export const PERIODS = ["7d", "28d", "90d", "all"] as const;
export type Period = (typeof PERIODS)[number];
export const isPeriod = (x: unknown): x is Period => PERIODS.some((p) => p === x);
const PERIOD_DAYS: Record<Period, number | null> = { "7d": 7, "28d": 28, "90d": 90, all: null };

/** "in the last 7 days", "ever": a period, for the end of a sentence. */
export const PERIOD_WHEN: Record<Period, string> = { "7d": "in the last 7 days", "28d": "in the last 28 days", "90d": "in the last 90 days", all: "ever" };

/** Before any history: "all time" as a start date, so the score covers it too (no date means 28 days). */
const ALL_TIME = "1970-01-01";

/** The first day a period covers, as a local date ("2026-10-02"). */
export function periodSince(p: Period, today = new Date()): string {
  const days = PERIOD_DAYS[p];
  if (days == null) return ALL_TIME;
  const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - (days - 1));
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** How far back the Improve charts look. */
export const RANGES = ["12w", "6m", "all"] as const;
export type Range = (typeof RANGES)[number];
export const isRange = (x: unknown): x is Range => RANGES.some((r) => r === x);

export type AppState = {
  tab: Tab;
  /** Which scorecard section is showing. */
  view: ScoreView;
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
  /** The prompt tier last drawn (undefined: none drawn yet), for the same reason. */
  lastBriefTier: string | null | undefined;
  meta: Meta | null;
  /** Scorecard: how far back the counts look. */
  period: Period;
  /** Improve: how far back the charts look. */
  range: Range;
  /** Learn: the lesson open, or null for the list. */
  lesson: string | null;
  /** Learn: what's typed in the prompt checker. Never saved. */
  draft: string;
};

const initial = (): AppState => ({
  tab: "score",
  view: "overview",
  filter: {},
  allModels: [],
  status: { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 },
  current: null,
  update: null,
  laterFor: null,
  lastVerdict: null,
  lastBriefTier: undefined,
  meta: null,
  period: "28d",
  range: "12w",
  lesson: null,
  draft: "",
});

const s: AppState = initial();
export const state: Readonly<AppState> = s;

/** The small window under the menu bar icon. */
export const isGlance = typeof location !== "undefined" && location.hash === "#glance";

export function setTab(tab: Tab): void {
  s.tab = tab;
}
export function setView(view: ScoreView): void {
  s.view = view;
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
export function setLastBriefTier(tier: string | null): void {
  s.lastBriefTier = tier;
}
export function setPeriod(period: Period): void {
  s.period = period;
}
export function setRange(range: Range): void {
  s.range = range;
}
export function setLesson(id: string | null): void {
  s.lesson = id;
}
export function setDraft(text: string): void {
  s.draft = text;
}
export function setMeta(meta: Meta): Meta {
  s.meta = meta;
  return meta;
}

/** Tests only: back to a fresh launch. */
export function resetState(): void {
  Object.assign(s, initial());
}
