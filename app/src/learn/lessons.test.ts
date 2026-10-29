import { describe, expect, it } from "vitest";
import { briefs, meta } from "../test/fixtures";
import { answerIndex, LESSONS, lessonById, recommended, SIGNAL_HINTS, TRACKS, trackOf } from "./lessons";

const ids = LESSONS.map((l) => l.id);
const signalIds = meta.signals.map((s) => s.id);

describe("lessons data", () => {
  it("has three tracks, and every lesson belongs to one", () => {
    expect(TRACKS.map((t) => t.id)).toEqual(["habits", "steer", "catch"]);
    expect(LESSONS).toHaveLength(TRACKS.reduce((n, t) => n + t.lessons.length, 0));
    for (const l of LESSONS) expect(trackOf(l.id)?.lessons).toContain(l);
  });

  it("gives every lesson a unique id", () => {
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[a-z0-9-]+$/);
  });

  it("has a hint for every signal", () => {
    for (const s of signalIds) expect(SIGNAL_HINTS[s], s).toBeTruthy();
  });

  it("teaches each signal in exactly one habit lesson", () => {
    for (const s of signalIds) {
      const taught = LESSONS.filter((l) => "signal" in l.teaches && l.teaches.signal === s);
      expect(taught, s).toHaveLength(1);
    }
  });

  it("only teaches signals and steers that exist", () => {
    const steers = meta.steers.map((s) => s.id);
    for (const l of LESSONS) {
      if ("signal" in l.teaches) expect(signalIds, l.id).toContain(l.teaches.signal);
      if ("steer" in l.teaches) expect(steers, l.id).toContain(l.teaches.steer);
    }
  });

  it("only names signals that exist in its before and after examples", () => {
    for (const l of LESSONS) for (const s of [...l.before.signals, ...l.after.signals]) expect(signalIds, l.id).toContain(s);
  });

  it("has a quiz for every lesson, with a real answer in each question", () => {
    for (const l of LESSONS) {
      expect(l.quiz.length, l.id).toBeGreaterThan(0);
      for (const q of l.quiz) {
        expect(q.options.length, `${l.id}: ${q.question}`).toBeGreaterThanOrEqual(2);
        const a = answerIndex(q);
        expect(Number.isInteger(a) && a >= 0 && a < q.options.length, `${l.id}: ${q.question}`).toBe(true);
      }
    }
  });

  it("keeps every 'missing' answer among its options, and every option a real signal", () => {
    for (const l of LESSONS) {
      for (const q of l.quiz) {
        if (q.kind !== "missing") continue;
        expect(q.options, l.id).toContain(q.answer);
        for (const o of q.options) expect(signalIds, l.id).toContain(o);
      }
    }
  });
});

describe("answerIndex", () => {
  it("reads the number as is, and finds a 'missing' answer's place in its options", () => {
    expect(answerIndex({ kind: "sharper", question: "q", options: ["a", "b"], answer: 1, explain: "" })).toBe(1);
    expect(answerIndex({ kind: "pick", question: "q", options: ["a", "b"], answer: 0, explain: "" })).toBe(0);
    expect(answerIndex({ kind: "missing", question: "q", brief: "b", options: ["spot", "done", "why"], answer: "why", explain: "" })).toBe(2);
  });
});

describe("lessonById / trackOf", () => {
  it("finds a lesson and its track, or nothing", () => {
    expect(lessonById("say-why")?.title).toBeTruthy();
    expect(trackOf("say-why")?.id).toBe("habits");
    expect(lessonById("nope")).toBeUndefined();
    expect(lessonById(null)).toBeUndefined();
    expect(trackOf("nope")).toBeUndefined();
  });
});

describe("recommended", () => {
  // 100 tasks. Carried: spot 90, proof 50, done 90, limits 20, why 40; steering is mostly "clarified".
  const b = (over: Parameters<typeof briefs>[0] = {}) =>
    briefs({
      tasks: 100,
      signals: { spot: 90, proof: 50, done: 90, limits: 20, why: 40 },
      steering: { ai_wrong: 50, added: 3, clarified: 9, redo: 0 },
      ...over,
    });
  const pick = (br: ReturnType<typeof b> | null, done: string[] = []) => recommended(br, meta, done)?.id;

  it("starts with the habit you carry least", () => {
    expect(pick(b())).toBe("set-limits");
  });

  it("moves to the next least-carried habit once a lesson is done", () => {
    expect(pick(b(), ["set-limits"])).toBe("say-why");
    expect(pick(b(), ["set-limits", "say-why"])).toBe("show-it");
  });

  it("treats a signal carried in more than 60% of prompts as a habit you have", () => {
    const strong = { spot: 90, proof: 90, done: 90, limits: 61, why: 90 };
    expect(pick(b({ signals: strong }))).not.toBe("set-limits");
    expect(pick(b({ signals: { ...strong, limits: 60 } }))).toBe("set-limits");
  });

  it("falls to the steering you do most, and never recommends against the AI's own mistakes", () => {
    const all = { spot: 90, proof: 90, done: 90, limits: 90, why: 90 };
    expect(pick(b({ signals: all }))).toBe("say-exactly-what-you-mean"); // clarified 9 beats added 3
    expect(pick(b({ signals: all }), ["say-exactly-what-you-mean"])).toBe("let-it-ask-first");
    expect(pick(b({ signals: all }), ["say-exactly-what-you-mean", "let-it-ask-first"])).toBe("say-it-all-up-front");
    expect(pick(b({ signals: all, steering: { ai_wrong: 99, added: 0, clarified: 0, redo: 0 } }))).toBe("point-at-it"); // nothing on you: first left
  });

  it("returns the first lesson that's left when there's nothing to go on", () => {
    expect(pick(null)).toBe(LESSONS[0]?.id);
    expect(pick(b({ tasks: 0 }))).toBe(LESSONS[0]?.id);
    expect(pick(null, [LESSONS[0]?.id ?? ""])).toBe(LESSONS[1]?.id);
  });

  it("returns nothing when every lesson is done", () => {
    expect(pick(b(), ids)).toBeUndefined();
    expect(pick(null, ids)).toBeUndefined();
  });

  it("skips lessons already done even when they match best", () => {
    const done = ["set-limits", "say-why", "show-it"];
    expect(pick(b(), done)).not.toBeUndefined();
    expect(done).not.toContain(pick(b(), done));
  });
});
