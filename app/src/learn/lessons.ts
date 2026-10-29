// The Learn tab's lessons, from lessons.json. crates/engine/tests/lessons.rs grades every example
// and quiz answer with the real rules and lints the copy so it never names a tool or goes stale.
import type { Briefs, Meta } from "../api";
import data from "./lessons.json";

export type Example = { text: string; signals: string[] };
export type Teaches = { signal: string } | { steer: string } | { catch: true };
export type Question =
  | { kind: "sharper"; question: string; options: string[]; answer: number; explain: string }
  | { kind: "missing"; question: string; brief: string; options: string[]; answer: string; explain: string }
  | { kind: "pick"; question: string; options: string[]; answer: number; explain: string };
export type Lesson = {
  id: string;
  title: string;
  reviewed: string;
  teaches: Teaches;
  why: string;
  before: Example;
  after: Example;
  template: string;
  tips: string[];
  quiz: Question[];
};
export type Track = { id: string; title: string; blurb: string; lessons: Lesson[] };

export const TRACKS = data.tracks as Track[];
/** One line per signal, shown when a draft is missing it. */
export const SIGNAL_HINTS = data.signal_hints as Record<string, string>;

export const LESSONS: Lesson[] = TRACKS.flatMap((t) => t.lessons);

export const lessonById = (id: string | null): Lesson | undefined => LESSONS.find((l) => l.id === id);

export const trackOf = (id: string): Track | undefined => TRACKS.find((t) => t.lessons.some((l) => l.id === id));

/** The index of the right option, for any kind of question. */
export const answerIndex = (q: Question): number => (q.kind === "missing" ? q.options.indexOf(q.answer) : q.answer);

/** Signals your prompts carry in more than this share are habits you have: no need to push them. */
const HABIT_PCT = 60;

/**
 * The lesson to do next: the habit your prompts carry least (one you don't have yet), then the
 * steering you do most, then anything left in order. Lessons already done are skipped.
 */
export function recommended(b: Briefs | null, meta: Meta, done: readonly string[]): Lesson | undefined {
  const left = LESSONS.filter((l) => !done.includes(l.id));
  if (!b?.tasks) return left[0];
  const share = (signal: string) => (100 * (b.signals[signal] ?? 0)) / b.tasks;
  const habits = meta.signals
    .map((s) => s.id)
    .filter((s) => share(s) <= HABIT_PCT)
    .sort((x, y) => share(x) - share(y));
  const steers = meta.steers
    .map((s) => s.id)
    .filter((s) => s !== "ai_wrong" && (b.steering[s] ?? 0) > 0)
    .sort((x, y) => (b.steering[y] ?? 0) - (b.steering[x] ?? 0));
  const teaches = (l: Lesson, key: "signal" | "steer", id: string) => key in l.teaches && (l.teaches as Record<string, string>)[key] === id;
  for (const s of habits) {
    const l = left.find((x) => teaches(x, "signal", s));
    if (l) return l;
  }
  for (const s of steers) {
    const l = left.find((x) => teaches(x, "steer", s));
    if (l) return l;
  }
  return left[0];
}
