// Shared test data, shaped like the Rust side's output.
import type { HallEntry, Meta, ModelRow, SourceInfo, Stats } from "../api";

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
};

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
