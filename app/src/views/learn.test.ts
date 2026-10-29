// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import type { Check } from "../api";
import { LESSONS, TRACKS } from "../learn/lessons";
import { briefs, HOSTILE, meta } from "../test/fixtures";
import { checkResultView, judgeQuiz, type LearnContext, learnView } from "./learn";

const ctx = (over: Partial<LearnContext> = {}): LearnContext => ({ meta, briefs: null, done: [], lesson: null, draft: "", demo: false, ...over });
const home = (over: Partial<LearnContext> = {}) => learnView(ctx(over)).value;
const lesson = (id = "point-at-it", over: Partial<LearnContext> = {}) => learnView(ctx({ lesson: id, ...over })).value;

const safe = (markup: string) => {
  expect(markup).not.toContain("<img");
  expect(markup).not.toContain('onerror="');
};

describe("learnView home", () => {
  it("lists every track with its lessons and the lesson count", () => {
    const m = home();
    expect(m).toContain(`0 of ${LESSONS.length} lessons`);
    for (const t of TRACKS) {
      expect(m).toContain(`<h2>${t.title}</h2>`);
      expect(m).toContain(`0 of ${t.lessons.length} done`);
      for (const l of t.lessons) expect(m).toContain(`data-action="lesson" data-id="${l.id}"`);
    }
    expect(m.match(/class="lesson-row/g)).toHaveLength(LESSONS.length);
  });

  it("shows the checker with the draft, escaped, and says nothing is saved", () => {
    const m = home({ draft: HOSTILE });
    expect(m).toContain('id="draft" data-action="draft"');
    expect(m).toContain("Nothing you type here is saved.");
    expect(m).toContain('data-action="copy-draft"');
    expect(m).toContain('data-action="clear-draft"');
    safe(m);
  });

  it("explains the checker only works in the app when it's the demo", () => {
    expect(home({ demo: true })).toContain("In the app, the checker grades as you type");
    expect(home()).not.toContain("In the app, the checker grades");
  });

  it("marks lessons done, counts them per track, and ignores ids it doesn't know", () => {
    const m = home({ done: ["point-at-it", "show-it", "gone"] });
    expect(m).toContain(`2 of ${LESSONS.length} lessons`);
    expect(m).toContain("2 of 5 done");
    expect(m).toContain('lesson-row done" data-action="lesson" data-id="point-at-it"');
    expect(m).toContain('<span class="sr-only"> (done)</span>');
  });

  it("starts at the first lesson with no data, and picks one from your briefs with it", () => {
    expect(home()).toContain("Start here");
    expect(home()).toContain('class="lesson-next" data-action="lesson" data-id="point-at-it"');
    const m = home({ briefs: briefs({ tasks: 100, signals: { spot: 90, proof: 90, done: 90, limits: 20, why: 90 } }) });
    expect(m).toContain("Picked for you, from your briefs");
    expect(m).toContain('class="lesson-next" data-action="lesson" data-id="set-limits"');
  });

  it("marks a finished track and graduates when every lesson is done", () => {
    const habits = TRACKS[0]?.lessons.map((l) => l.id) ?? [];
    expect(home({ done: habits })).toContain("Track done");
    const m = home({ done: LESSONS.map((l) => l.id) });
    expect(m).toContain("Graduate");
    expect(m).not.toContain("lesson-next");
  });
});

describe("learnView lesson", () => {
  const l = LESSONS[0];
  if (!l) throw new Error("no lessons");

  it("shows before and after with the signals each carries", () => {
    const m = lesson(l.id);
    expect(m).toContain(`<h1 class="q">${l.title}</h1>`);
    expect(m).toContain('<div class="eyebrow">Before</div>');
    expect(m).toContain('<div class="eyebrow">After</div>');
    expect(m).toContain("Nothing specific"); // the before example carries nothing
    expect(m).toContain('<span class="chip sig-on">Points at it</span>');
    expect(m).toContain('<span class="chip sig-on">Says what done is</span>');
    expect(m).toContain("The date is wrong on the invoice page.");
  });

  it("shows the template, escaped, with a way to try it", () => {
    const m = lesson(l.id);
    expect(m).toContain('<p class="template mono">In &lt;file, function or doc&gt;');
    expect(m).toContain(`data-action="try-template" data-id="${l.id}"`);
  });

  it("renders each quiz question as a fieldset whose data-answer is the right option", () => {
    const m = lesson(l.id);
    const sets = m.split('<fieldset class="quiz-q"').slice(1);
    expect(sets).toHaveLength(l.quiz.length);
    expect(sets[0]).toContain('data-answer="1"'); // the sharper question: the second brief
    expect(sets[1]).toContain('data-answer="0"'); // "missing": spot is the first option
    // Radios are grouped per question, valued by option index.
    expect(sets[0]).toContain('name="q-0-0" value="0" data-action="quiz"');
    expect(sets[0]).toContain('name="q-0-0" value="1" data-action="quiz"');
    expect(sets[1]).toContain('name="q-0-1" value="2" data-action="quiz"');
    expect(m).toContain('role="status" aria-live="polite" data-explain="');
  });

  it("titles a 'missing' question's options by signal and quotes the brief", () => {
    const m = lesson(l.id);
    expect(m).toContain("<blockquote");
    expect(m).toContain("<span>Points at it</span></label>");
    expect(m).toContain("<span>Sets limits</span></label>");
  });

  it("numbers the questions by lesson position, so two lessons never share a radio group", () => {
    const second = LESSONS[1];
    expect(lesson(second?.id).includes('name="q-1-0"')).toBe(true);
    expect(lesson(second?.id).includes('name="q-0-0"')).toBe(false);
  });

  it("offers Mark done or Mark not done, and the next lesson", () => {
    expect(lesson(l.id)).toContain('data-action="lesson-done" data-id="point-at-it" data-done="true">Mark done');
    expect(lesson(l.id, { done: [l.id] })).toContain('data-done="false">Mark not done');
    expect(lesson(l.id)).toContain(`Next: ${LESSONS[1]?.title}`);
    const last = LESSONS.at(-1);
    expect(lesson(last?.id)).not.toContain("Next:");
  });

  it("names the track and links back to all lessons", () => {
    const m = lesson(l.id);
    expect(m).toContain(`<span class="eyebrow">${TRACKS[0]?.title}</span>`);
    expect(m).toContain('data-action="lesson" data-id="">← All lessons');
  });

  it("falls back to the home page for an unknown lesson", () => {
    expect(lesson("nope")).toContain("Learn to brief it");
  });

  it("escapes signal titles from meta", () => {
    const bad = { ...meta, signals: meta.signals.map((s) => ({ ...s, title: HOSTILE })) };
    safe(learnView(ctx({ meta: bad, lesson: l.id })).value);
  });
});

describe("checkResultView", () => {
  const check = (over: Partial<Check> = {}): Check => ({ signals: 0, grade: "thin", rules: [], not_a_brief: false, ...over });

  it("asks for a brief when empty, or when there's no result", () => {
    expect(checkResultView(meta, null, true).value).toContain("Type or paste a brief.");
    expect(checkResultView(meta, check(), true).value).toContain("Type or paste a brief.");
    expect(checkResultView(meta, null, false).value).toContain("Type or paste a brief.");
  });

  it("lights up the signals in a sharp check, and says there's nothing missing at five", () => {
    const m = checkResultView(meta, check({ signals: 0b111, grade: "sharp" }), false).value;
    expect(m).toContain('class="check-grade is-sharp"><b>Sharp</b> · carries 3 of 5');
    expect(m.match(/<li class="on">/g)).toHaveLength(3);
    expect(m).toContain('<li class="on"><span aria-hidden="true">✓</span> Points at it<span class="sr-only">: yes</span>');
    expect(m).toContain('<li class=""><span aria-hidden="true">·</span> Sets limits<span class="sr-only">: not yet</span>');
    expect(checkResultView(meta, check({ signals: 0b11111, grade: "sharp" }), false).value).toContain("Nothing left to guess.");
  });

  it("nudges at most the first two missing signals, from the hints", () => {
    const m = checkResultView(meta, check({ signals: 0b00011 }), false).value; // spot and proof; done, limits and why missing
    expect(m).toContain("Say what done looks like");
    expect(m).toContain("Set limits");
    expect(m).not.toContain("Say why:");
    expect(m.match(/<li>Say|<li>Set/g)).toHaveLength(2);
    expect(m).not.toContain("Nothing left to guess.");
  });

  it("says so when it isn't a brief", () => {
    expect(checkResultView(meta, check({ not_a_brief: true }), false).value).toContain("This reads as a question or a hello.");
    expect(checkResultView(meta, check({ signals: 1 }), false).value).not.toContain("reads as a question");
  });

  it("falls back to the id for a grade it doesn't know, and escapes it", () => {
    expect(checkResultView(meta, check({ grade: "legend" }), false).value).toContain("<b>legend</b>");
    safe(checkResultView(meta, check({ grade: HOSTILE }), false).value);
  });
});

describe("judgeQuiz", () => {
  const pick = (value: string): HTMLInputElement => {
    const el = document.querySelector<HTMLInputElement>(`fieldset input[value="${value}"]`);
    if (!el) throw new Error(`no option ${value}`);
    return el;
  };
  const first = () => document.querySelector<HTMLElement>("fieldset");
  const opt = (input: HTMLInputElement) => input.closest(".quiz-opt");
  const feedback = () => document.querySelector(".quiz-feedback")?.textContent;

  beforeEach(() => {
    document.body.innerHTML = lesson("point-at-it");
  });

  it("marks the right answer and says why", () => {
    const right = pick("1");
    judgeQuiz(right);
    expect(opt(right)?.classList.contains("right")).toBe(true);
    expect(feedback()).toBe(`Right. ${first()?.querySelector<HTMLElement>(".quiz-feedback")?.dataset.explain}`);
    expect(feedback()).toContain("It names the exact function");
  });

  it("marks a wrong answer and keeps the explanation", () => {
    const wrong = pick("0");
    judgeQuiz(wrong);
    expect(opt(wrong)?.classList.contains("wrong")).toBe(true);
    expect(feedback()).toMatch(/^Not quite\. It names the exact function/);
  });

  it("clears the earlier mark when the person changes their answer", () => {
    const wrong = pick("0");
    const right = pick("1");
    judgeQuiz(wrong);
    judgeQuiz(right);
    expect(opt(wrong)?.classList.contains("wrong")).toBe(false);
    expect(opt(right)?.classList.contains("right")).toBe(true);
    expect(first()?.querySelectorAll(".right, .wrong")).toHaveLength(1);
  });

  it("judges a 'missing' question by the option's place in the list", () => {
    const set = document.querySelectorAll("fieldset")[1] as HTMLElement;
    const inputs = set.querySelectorAll<HTMLInputElement>("input");
    judgeQuiz(inputs[0] as HTMLInputElement); // spot is the answer
    expect(opt(inputs[0] as HTMLInputElement)?.classList.contains("right")).toBe(true);
    judgeQuiz(inputs[1] as HTMLInputElement);
    expect(opt(inputs[1] as HTMLInputElement)?.classList.contains("wrong")).toBe(true);
    expect(first()?.querySelectorAll(".right, .wrong")).toHaveLength(0); // the first question is untouched
  });

  it("does nothing for an input outside a quiz", () => {
    document.body.innerHTML = `<input value="1"/>`;
    expect(() => judgeQuiz(document.querySelector("input") as HTMLInputElement)).not.toThrow();
  });
});
