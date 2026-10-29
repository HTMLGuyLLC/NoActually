// Shared test data, shaped like the Rust side's output.
import type { Briefs, HallEntry, Meta, ModelRow, Overall, Progress, SourceInfo, Stats } from "../api";

export const meta: Meta = {
  tiers: [
    { id: "rubber_stamp", name: "Rubber stamp", floor: 0 },
    { id: "coworker", name: "Coworker", floor: 65 },
    { id: "sharp_eyed", name: "Sharp-eyed", floor: 80 },
    { id: "irreplaceable", name: "Irreplaceable", floor: 90 },
  ],
  direction_weight: 70,
  catch_proof: 3,
  humbled_cost: 5,
  humbled_base: 30,
  categories: [
    { id: "made_it_up", title: "Made it up" },
    { id: "just_wrong", title: "Flat-out wrong" },
  ],
  sources: [
    { id: "claude-code", name: "Claude Code" },
    { id: "chatgpt-export", name: "ChatGPT (export)" },
  ],
  brief_tiers: [
    { id: "vibes_only", name: "Vibes only", floor: 0, verdict: "Constantly." },
    { id: "ticket_writer", name: "Ticket writer", floor: 70, verdict: "Often." },
    { id: "tech_lead", name: "Tech lead", floor: 80, verdict: "Sometimes." },
    { id: "architect", name: "Architect", floor: 90, verdict: "Rarely." },
  ],
  catch_tiers: [
    { id: "autopilot", name: "Autopilot", floor: 0, verdict: "Rarely." },
    { id: "spot_checker", name: "Spot-checker", floor: 50, verdict: "Sometimes." },
    { id: "reviewer", name: "Reviewer", floor: 75, verdict: "Usually." },
    { id: "hawk_eyed", name: "Hawk-eyed", floor: 90, verdict: "Always." },
  ],
  min_briefs: 10,
  brief_window_days: 28,
  signals: [
    { id: "spot", title: "Points at it", hint: "a file, function, link or code" },
    { id: "proof", title: "Shows it", hint: "an error, the output or an example" },
    { id: "done", title: "Says what done is", hint: "the expected result, a test or a format" },
    { id: "limits", title: "Sets limits", hint: "don't, only, keep, without" },
    { id: "why", title: "Says why", hint: "the goal or the context" },
  ],
  sharp_signals: 3,
  grades: [
    { id: "thin", title: "Thin" },
    { id: "solid", title: "Solid" },
    { id: "sharp", title: "Sharp" },
  ],
  steers: [
    { id: "ai_wrong", title: "It got it wrong" },
    { id: "added", title: "You added more" },
    { id: "clarified", title: "You clarified" },
    { id: "redo", title: "You started over" },
  ],
  min_week: 5,
  goal_weeks: 4,
};

export const briefs = (over: Partial<Briefs> = {}): Briefs => ({
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
  tip: { signal: "limits", pct_of_briefs: 10, landed_with_without: null },
  ...over,
});

export const HOSTILE = `<img src=x onerror="alert(1)">&'"`;

export const row = (provider: string, family: string, version: string, catches: number, messages = 100, tasks = 20): ModelRow => ({
  provider,
  family,
  version,
  catches,
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
  catch_tier: "hawk_eyed",
  next_catch_tier: null,
  catches: 41,
  humbled: 2,
  to_rank: 0,
  ...over,
});

export const week = (day: string, over: Partial<Progress["weeks"][number]> = {}): Progress["weeks"][number] => ({
  week: day,
  messages: 200,
  catches: 8,
  humbled: 0,
  tasks: 20,
  continued: 12,
  landed: 10,
  signals: { spot: 12, proof: 8, done: 6, limits: 3, why: 4 },
  grades: { thin: 8, solid: 7, sharp: 5 },
  steering: { ai_wrong: 3, added: 2, clarified: 1, redo: 0 },
  steered: { ai_wrong: 2, added: 2, clarified: 1, redo: 0 },
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
  bests: { landed: [91, "2026-10-19"], sharp: [40, "2026-10-05"], goal_streak: 0 },
  insights: { signals: [], steer: null, models: [] },
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
  category_title: "Made it up",
  human: "that's not a real page",
  ai: "You're right.",
  pinned: false,
  ...over,
});

export const stats = (over: Partial<Stats> = {}): Stats => ({
  first_day: "2026-09-01",
  last_day: "2026-10-27",
  messages: 1000,
  catches: 177,
  humbled: 3,
  labels: {},
  catches_last_7d: 17,
  overall: overall(),
  messages_per_catch: 47,
  real_chats: 100,
  pct_real_chats_with_catch: 37.6,
  best_streak_days: 14,
  current_streak_days: 3,
  peak_hour: 21,
  categories: { just_wrong: 10, made_it_up: 2 },
  weekly: [
    { week: "2026-10-12", messages: 100, catches: 4 },
    { week: "2026-10-19", messages: 200, catches: 16 },
  ],
  models: [row("Anthropic", "Claude Opus", "5", 120), row("OpenAI", "GPT Astra", "6", 57)],
  hall_of_fame: [hallEntry()],
  briefs: briefs(),
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
