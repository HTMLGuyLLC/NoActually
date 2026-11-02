// The UI's state: one object, read anywhere, changed only through the named setters below.
import type { Filter, Meta, ModelRow, ReportCardsStatus, ReportCardTool, Stats, SyncStatus, UpdateChannel, UpdateStatus, Verdict } from "./api";
import { localIsoDate } from "./format";

export const TABS = ["score", "improve", "learn", "settings"] as const;
export type Tab = (typeof TABS)[number];
export const isTab = (x: unknown): x is Tab => TABS.some((t) => t === x);

/** The scorecard's sections, each its own sub-tab. */
export const SCORE_VIEWS = ["overview", "prompts", "corrections", "hall"] as const;
export type ScoreView = (typeof SCORE_VIEWS)[number];
export const isScoreView = (x: unknown): x is ScoreView => SCORE_VIEWS.some((v) => v === x);
export const SETTINGS_PANES = ["general", "sources", "report-cards", "privacy"] as const;
export type SettingsPane = (typeof SETTINGS_PANES)[number];
export const isSettingsPane = (x: unknown): x is SettingsPane => SETTINGS_PANES.some((v) => v === x);

/** A report cards control that runs a command: a tool's Turn on or Turn off, or Delete report card and tip history. */
export type ReportCardsBusy = ReportCardTool | "delete";

/** Settings → Report cards: what was last read, and what's happening now. */
export type ReportCardsState = {
  /** Where each tool stands, as last read; null until the first read finishes. */
  status: ReportCardsStatus | null;
  /** Why the last read failed, in plain words ("" when it didn't). */
  statusError: string;
  /** The control whose command is running. */
  busy: ReportCardsBusy | null;
  /** What went wrong with a control, in plain words, shown beside it. */
  error: { control: ReportCardsBusy | "tips" | "reminders"; text: string } | null;
  /** What happened last ("Done. Start a new Claude Code chat…"), shown beside the control that did it. */
  note: { control: ReportCardsBusy; text: string } | null;
};

/** Nothing read yet and nothing running. */
export const emptyReportCards = (): ReportCardsState => ({ status: null, statusError: "", busy: null, error: null, note: null });

/** Settings → Privacy → Delete everything. There's no "done": it ends with the app quitting. */
export type DeleteAllState = {
  /** It's running. */
  busy: boolean;
  /** Why it stopped, in the app's words ("" when it didn't). */
  error: string;
};

/** How far back the scorecard's counts look. The score and tiers follow the chosen period too. */
export const PERIODS = ["7d", "28d", "90d", "all"] as const;
export type Period = (typeof PERIODS)[number];
export const isPeriod = (x: unknown): x is Period => PERIODS.some((p) => p === x);
const PERIOD_DAYS: Record<Period, number | null> = { "7d": 7, "28d": 28, "90d": 90, all: null };

/** "in the last 7 days", "across all time": a period, for the end of a sentence. */
export const PERIOD_WHEN: Record<Period, string> = {
  "7d": "in the last 7 days",
  "28d": "in the last 28 days",
  "90d": "in the last 90 days",
  all: "across all time",
};

/** Before any history: "all time" as a start date, so the score covers it too (no date means 28 days). */
const ALL_TIME = "1970-01-01";

/** The first day a period covers, as a local date ("2026-10-02"). */
export function periodSince(p: Period, today = new Date()): string {
  const days = PERIOD_DAYS[p];
  if (days == null) return ALL_TIME;
  const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - (days - 1));
  return localIsoDate(d);
}

/** How far back the Improve charts look. */
export const RANGES = ["12w", "6m", "all"] as const;
export type Range = (typeof RANGES)[number];
export const isRange = (x: unknown): x is Range => RANGES.some((r) => r === x);

/**
 * The first read of a history, after the first Allow: "reading" while the app takes it in, "done" once a check
 * has ended and until the person has seen the scorecard, "off" otherwise.
 */
export type Reading = "off" | "reading" | "done";

export type AppState = {
  tab: Tab;
  /** Which scorecard section is showing. */
  view: ScoreView;
  /** Settings: which sub-tab is open. */
  settingsPane: SettingsPane;
  reportCards: ReportCardsState;
  deleteAll: DeleteAllState;
  filter: Filter;
  /** Every model, from the last unfiltered stats; the picker lists these even while narrowed. */
  allModels: ModelRow[];
  /** The window `allModels` counts cover: a start date on the scorecard, "" (all history) elsewhere. */
  allModelsScope: string;
  status: SyncStatus;
  reading: Reading;
  /** The stats last drawn. */
  current: Stats | null;
  update: UpdateStatus | null;
  /** The version the person said "Later" to; its banner stays hidden until a newer one. */
  laterFor: string | null;
  /** The verdict last drawn, so the scrawl only plays when it changes, not on every background redraw. */
  lastVerdict: Verdict | null;
  /** The prompt tier last drawn (undefined: none drawn yet), for the same reason. */
  lastPromptTier: string | null | undefined;
  /** The correction tier last drawn, the same way. */
  lastCorrectionTier: string | null | undefined;
  meta: Meta | null;
  /** Scorecard: how far back the counts look. */
  period: Period;
  /** Improve: how far back the charts look. */
  range: Range;
  /** Learn: the lesson open, or null for the list. */
  lesson: string | null;
  /** Learn: what's typed in the prompt checker. Never saved. */
  draft: string;
  /** Hall of fame: the entry id each award shows, by award key. An id, not a position, so you stay
   * on the same entry when pinning moves it or a redraw re-sorts the list. */
  awardSlide: Record<string, number>;
};

const initial = (): AppState => ({
  tab: "score",
  view: "overview",
  settingsPane: "general",
  reportCards: emptyReportCards(),
  deleteAll: { busy: false, error: "" },
  filter: {},
  allModels: [],
  allModelsScope: "",
  status: { syncing: false, last_sync: null, last_error: null, files_with_errors: 0, rebuilding: false },
  reading: "off",
  current: null,
  update: null,
  laterFor: null,
  lastVerdict: null,
  lastPromptTier: undefined,
  lastCorrectionTier: undefined,
  meta: null,
  period: "28d",
  range: "12w",
  lesson: null,
  draft: "",
  awardSlide: {},
});

const s: AppState = initial();
export const state: Readonly<AppState> = s;

/** The small window under the menu bar icon. */
export const isGlance = typeof location !== "undefined" && location.hash === "#glance";
/** macOS draws the main window's buttons over the page (`titleBarStyle` in tauri.conf.json). Other systems ignore that setting and keep their own bar. */
export const hasOverlayTitleBar = (inApp: boolean, glance: boolean, userAgent: string): boolean => inApp && !glance && userAgent.includes("Macintosh");

export function setTab(tab: Tab): void {
  s.tab = tab;
}
export function setSettingsPane(pane: SettingsPane): void {
  s.settingsPane = pane;
}
export function setReportCards(patch: Partial<ReportCardsState>): void {
  s.reportCards = { ...s.reportCards, ...patch };
}
export function setDeleteAll(next: DeleteAllState): void {
  s.deleteAll = next;
}
export function setView(view: ScoreView): void {
  s.view = view;
}
export function setFilter(filter: Filter): void {
  s.filter = filter;
}
export function setAllModels(models: ModelRow[], scope = ""): void {
  s.allModels = models;
  s.allModelsScope = scope;
}
/** A check can finish in a blink, and "Checking…" has to stay long enough to read, or people keep clicking. */
const MIN_CHECKING_MS = 2000;
let checkingSince = 0;

/** How long "Checking…" should stay up yet, in milliseconds. */
export const checkingLeft = (now = Date.now()): number => Math.max(0, checkingSince + MIN_CHECKING_MS - now);

export function setStatus(status: SyncStatus): void {
  s.status = status;
}
/** A check started: only the status line changes. */
export function setSyncing(now = Date.now()): void {
  checkingSince = now;
  s.status = { ...s.status, syncing: true };
}
export function setCurrent(stats: Stats): void {
  s.current = stats;
}
export function setUpdate(update: UpdateStatus): void {
  s.update = update;
}
export function setReading(reading: Reading): void {
  s.reading = reading;
}
/** An update that failed to install, in the app's words, until the next status replaces it. */
export function setUpdateError(text: string): void {
  if (s.update) s.update = { ...s.update, last_error: text, installing: false };
}
export function setAutoUpdate(auto: boolean): void {
  if (s.update) s.update = { ...s.update, auto };
}
export function setUpdateChannel(channel: UpdateChannel): void {
  if (s.update) s.update = { ...s.update, channel };
}
export function setLaterFor(version: string | null): void {
  s.laterFor = version;
}
export function setLastVerdict(verdict: Verdict): void {
  s.lastVerdict = verdict;
}
export function setLastPromptTier(tier: string | null): void {
  s.lastPromptTier = tier;
}
export function setLastCorrectionTier(tier: string | null): void {
  s.lastCorrectionTier = tier;
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
export function setAwardSlide(key: string, entryId: number): void {
  s.awardSlide = { ...s.awardSlide, [key]: entryId };
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
