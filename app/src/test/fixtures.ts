// Shared test data, shaped like the Rust side's output.
import type { HallEntry, Meta, ModelRow, Overall, Progress, Prompts, RecentReportCard, ReportCardWeek, SourceInfo, Stats } from "../api";

export const meta: Meta = {
  tiers: [
    { id: "rubber_stamp", name: "Rubber stamp", floor: 0 },
    { id: "coworker", name: "Coworker", floor: 65 },
    { id: "sharp_eyed", name: "Sharp-eyed", floor: 80 },
    { id: "irreplaceable", name: "Irreplaceable", floor: 90 },
  ],
  direction_weight: 70,
  correction_proof: 3,
  humbled_cost: 5,
  humbled_base: 30,
  categories: [
    { id: "made_it_up", title: "Made it up" },
    { id: "just_wrong", title: "Flat-out wrong" },
  ],
  sources: [
    { id: "claude-code", name: "Claude Code", gradable: true },
    { id: "codex", name: "Codex", gradable: true },
    { id: "chatgpt-export", name: "ChatGPT (export)", gradable: false },
  ],
  prompt_tiers: [
    { id: "vibes_only", name: "Vibes only", floor: 0, verdict: "Constantly.", blurb: "It's mostly guessing what you want." },
    { id: "ticket_writer", name: "Ticket writer", floor: 70, verdict: "Often.", blurb: "It often fills in the blanks for you." },
    { id: "tech_lead", name: "Tech lead", floor: 80, verdict: "Sometimes.", blurb: "Clear most of the time, with a few gaps." },
    { id: "architect", name: "Architect", floor: 90, verdict: "Rarely.", blurb: "It rarely has to guess what you mean." },
  ],
  correction_tiers: [
    { id: "autopilot", name: "Autopilot", floor: 0, verdict: "Rarely.", blurb: "Most of its slips get past you." },
    { id: "spot_checker", name: "Spot-checker", floor: 50, verdict: "Sometimes.", blurb: "Some of its slips get past you." },
    { id: "reviewer", name: "Reviewer", floor: 75, verdict: "Often.", blurb: "You often catch it slipping." },
    { id: "hawk_eyed", name: "Hawk-eyed", floor: 90, verdict: "Religiously.", blurb: "You catch AI slipping nearly every time." },
  ],
  min_prompts: 10,
  prompt_window_days: 28,
  signals: [
    { id: "spot", title: "Target", hint: "a file, function, page or link" },
    { id: "proof", title: "Evidence", hint: "an error, the output or an example" },
    { id: "done", title: "Finish line", hint: "the expected result, a test or a format" },
    { id: "limits", title: "Guardrails", hint: "don't, only, keep, without" },
    { id: "why", title: "Purpose", hint: "who it's for or what it's part of" },
  ],
  habits: [
    { id: "target", signal: "spot", title: "Target", hint: "a file, function, page or link", plain: "say which file, page or screen" },
    { id: "evidence", signal: "proof", title: "Evidence", hint: "an error, the output or an example", plain: "paste the error or what you saw" },
    { id: "finish", signal: "done", title: "Finish line", hint: "the expected result, a test or a format", plain: "say what done looks like" },
    { id: "guardrails", signal: "limits", title: "Guardrails", hint: "don't, only, keep, without", plain: "say what not to touch" },
    { id: "purpose", signal: "why", title: "Purpose", hint: "who it's for or what it's part of", plain: "say why, or who it's for" },
    { id: "context", signal: null, title: "Context", hint: "what you already know or tried", plain: "say what you already know about your setup or taste" },
  ],
  sharp_signals: 3,
  grades: [
    { id: "thin", title: "Vague" },
    { id: "solid", title: "Decent" },
    { id: "sharp", title: "Specific" },
  ],
  steers: [
    { id: "ai_wrong", title: "It got it wrong" },
    { id: "added", title: "You added more" },
    { id: "clarified", title: "You clarified" },
    { id: "redo", title: "You started over" },
  ],
  min_week: 5,
  goal_weeks: 4,
  min_card_week: 3,
  min_card_half: 20,
  tip_rules: "The rules your AI gets.",
};

export const prompts = (over: Partial<Prompts> = {}): Prompts => ({
  tasks: 200,
  continued: 120,
  landed: 108,
  window_tasks: 40,
  window_landed: 38,
  tier: "architect",
  next_tier: null,
  to_rank: 0,
  grades: { thin: 100, solid: 60, sharp: 40 },
  signals: { spot: 80, proof: 50, done: 90, limits: 20, why: 40 },
  steering: { ai_wrong: 12, added: 9, clarified: 3, redo: 1 },
  tip: { signal: "limits", pct_of_prompts: 10, landed_with_without: null },
  ...over,
});

export const HOSTILE = `<img src=x onerror="alert(1)">&'"`;

export const row = (provider: string, family: string, version: string, corrections: number, messages = 100, tasks = 20): ModelRow => ({
  provider,
  family,
  version,
  corrections,
  messages,
  humbled: 0,
  tasks,
  continued: tasks / 2,
  landed: Math.floor((tasks / 2) * 0.8),
});

export const overall = (over: Partial<Overall> = {}): Overall => ({
  score: 86,
  direction: 83,
  judgment: 95,
  tier: "sharp_eyed",
  next_tier: "irreplaceable",
  verdict: "no",
  correction_tier: "hawk_eyed",
  next_correction_tier: null,
  corrections: 41,
  humbled: 2,
  to_rank: 0,
  ...over,
});

/** A week with no report cards, as Rust sends it: empty maps until something is graded. */
export const noCards = (): ReportCardWeek => ({
  graded: 0,
  grades: {},
  avoidable: 0,
  extra_tokens: 0,
  extra_cost: null,
  priced: 0,
  extra_minutes: 0,
  missing: {},
  gradable_tasks: 0,
});

/** A week of `graded` report cards. `grades` and `missing` give the counts they name (the rest are 0). A cost means every task was priced, unless `priced` says fewer. */
export const cards = (graded: number, over: Partial<ReportCardWeek> = {}): ReportCardWeek => ({
  graded,
  avoidable: 0,
  extra_tokens: 0,
  extra_cost: null,
  priced: over.extra_cost == null ? 0 : graded,
  extra_minutes: 0,
  gradable_tasks: graded * 2,
  ...over,
  grades: { A: 0, B: 0, C: 0, D: 0, F: 0, ...over.grades },
  missing: { target: 0, evidence: 0, finish: 0, guardrails: 0, purpose: 0, context: 0, ...over.missing },
});

export const recentCard = (over: Partial<RecentReportCard> = {}): RecentReportCard => ({
  day: "2026-10-27",
  graded_day: "2026-10-27",
  source: "claude-code",
  grade: "B",
  missing: "finish",
  extra_cost: 0.4,
  extra_tokens: 120_000,
  extra_minutes: 6,
  ...over,
});

/** An in-memory `localStorage`: the test DOM has none. */
export const memoryStorage = (): Storage => {
  const m = new Map<string, string>();
  return {
    getItem: (k) => m.get(k) ?? null,
    setItem: (k, v) => void m.set(k, v),
    removeItem: (k) => void m.delete(k),
    clear: () => m.clear(),
    key: (i) => [...m.keys()][i] ?? null,
    get length() {
      return m.size;
    },
  };
};

export const week = (day: string, over: Partial<Progress["weeks"][number]> = {}): Progress["weeks"][number] => ({
  week: day,
  tips: { tips: 0, tasks: 0 },
  messages: 200,
  corrections: 8,
  humbled: 0,
  tasks: 20,
  continued: 12,
  landed: 10,
  signals: { spot: 12, proof: 8, done: 6, limits: 3, why: 4 },
  grades: { thin: 8, solid: 7, sharp: 5 },
  steering: { ai_wrong: 3, added: 2, clarified: 1, redo: 0 },
  steered: { ai_wrong: 2, added: 2, clarified: 1, redo: 0 },
  report_cards: noCards(),
  ...over,
});

export const progress = (over: Partial<Progress> = {}): Progress => ({
  weeks: [week("2026-10-05"), week("2026-10-12", { landed: 11 }), week("2026-10-19", { landed: 9 }), week("2026-10-26", { tasks: 4, continued: 2, landed: 2 })],
  goal: null,
  suggestion: { kind: "habit", signal: "limits", target: 30, id: 0, start: "2026-10-28" },
  options: [
    { kind: "habit", signal: "limits", target: 30, id: 0, start: "2026-10-28" },
    { kind: "steer", steer: "added", target: 15, id: 0, start: "2026-10-28" },
    { kind: "tier", tier: "irreplaceable", target: 0, id: 0, start: "2026-10-28" },
  ],
  trophies: [],
  bests: { landed: [91, "2026-10-19"], sharp: [40, "2026-10-05"], goal_streak: 0, report_card: null },
  insights: { signals: [], steer: null, models: [], report_cards: null, top_tip: null, tip_trend: null },
  ...over,
});

export const hallEntry = (over: Partial<HallEntry> = {}): HallEntry => ({
  id: 1,
  day: "2026-10-01",
  source: "claude-code",
  source_name: "Claude Code",
  model: "Claude Opus 5",
  label: "confirmed",
  category: "made_it_up",
  human: "that's not a real page",
  ai: "You're right.",
  pinned: false,
  ...over,
});

export const stats = (over: Partial<Stats> = {}): Stats => ({
  first_day: "2026-09-01",
  last_day: "2026-10-27",
  messages: 1000,
  corrections: 177,
  humbled: 3,
  labels: {},
  corrections_last_7d: 17,
  overall: overall(),
  messages_per_correction: 47,
  real_chats: 100,
  pct_real_chats_with_correction: 37.6,
  best_streak_days: 14,
  current_streak_days: 3,
  peak_hour: 21,
  categories: { just_wrong: 10, made_it_up: 2 },
  weekly: [
    { week: "2026-10-12", messages: 100, corrections: 4, continued: 10, landed: 8, humbled: 0, categories: { just_wrong: 4 }, steering: { added: 2 } },
    {
      week: "2026-10-19",
      messages: 200,
      corrections: 16,
      continued: 20,
      landed: 17,
      humbled: 1,
      categories: { just_wrong: 10, made_it_up: 6 },
      steering: { ai_wrong: 4, added: 2, clarified: 1, redo: 0 },
    },
  ],
  models: [row("Anthropic", "Claude Opus", "5", 120), row("OpenAI", "GPT Astra", "6", 57)],
  hall_of_fame: [hallEntry()],
  prompts: prompts(),
  ...over,
});

export const source = (over: Partial<SourceInfo> = {}): SourceInfo => ({
  id: "claude-code",
  name: "Claude Code",
  kind: "live",
  roots: [["~/.claude/projects", true]],
  state: null,
  ...over,
});
