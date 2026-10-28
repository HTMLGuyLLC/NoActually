// Shared test data, shaped like the Rust side's output.
import type { Briefs, HallEntry, Meta, ModelRow, SourceInfo, Stats } from "../api";

export const meta: Meta = {
  tiers: [
    { id: "rubber_stamp", name: "Rubber stamp", floor: 0 },
    { id: "coworker", name: "Coworker", floor: 3 },
    { id: "sharp_eyed", name: "Sharp-eyed", floor: 7 },
    { id: "irreplaceable", name: "Irreplaceable", floor: 15 },
  ],
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
};

export const briefs = (over: Partial<Briefs> = {}): Briefs => ({
  tasks: 200,
  continued: 120,
  landed: 108,
  tasks_last_28d: 40,
  landed_last_28d: 38,
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

export const row = (provider: string, family: string, version: string, catches: number, messages = 100): ModelRow => ({
  provider,
  family,
  version,
  catches,
  messages,
  humbled: 0,
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
  tier: "irreplaceable",
  next_tier: null,
  verdict: "no",
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
