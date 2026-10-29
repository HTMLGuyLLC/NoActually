// Learn: how to prompt any AI well. A live prompt checker, three short tracks of lessons, and a
// quiz in each. Nothing here needs your history; with it, the next lesson is picked for you.
import type { Check, Meta, Prompts } from "../api";
import { type Html, html } from "../html";
import { answerIndex, LESSONS, type Lesson, lessonById, type Question, recommended, SIGNAL_HINTS, TRACKS, trackOf } from "../learn/lessons";
import { gradeTitle, hasSignal, signalTitle } from "../meta";

export type LearnContext = { meta: Meta; prompts: Prompts | null; done: readonly string[]; lesson: string | null; draft: string; demo: boolean };

const chips = (meta: Meta, ids: string[]) =>
  ids.length
    ? html`<span class="sig-chips">${ids.map((id) => html`<span class="chip sig-on">${signalTitle(meta, id)}</span>`)}</span>`
    : html`<span class="sig-chips"><span class="chip">Nothing specific</span></span>`;

/** The checker's verdict for a draft: grade, which signals it has, and a nudge for each missing one. */
export function checkResultView(meta: Meta, c: Check | null, empty: boolean): Html {
  if (empty || !c) return html`<p class="muted small">Type or paste a prompt. Each thing it carries lights up.</p>`;
  const has = (i: number) => hasSignal(c.signals, i);
  const grade = gradeTitle(meta, c.grade);
  const missing = meta.signals.filter((_, i) => !has(i));
  return html`<div class="check-grade is-${c.grade}"><b>${grade}</b> · carries ${meta.signals.filter((_, i) => has(i)).length} of ${meta.signals.length}</div>
    <ul class="check-signals">${meta.signals.map(
      (s, i) =>
        html`<li class="${has(i) ? "on" : ""}"><span aria-hidden="true">${has(i) ? "✓" : "·"}</span> ${s.title}<span class="sr-only">${has(i) ? ": yes" : ": not yet"}</span></li>`,
    )}</ul>
    ${c.not_a_prompt ? html`<p class="muted small">This reads as a question or a hello. Your history only grades requests as prompts.</p>` : ""}
    ${missing.length ? html`<ul class="check-hints">${missing.slice(0, 2).map((s) => html`<li>${SIGNAL_HINTS[s.id] ?? s.hint}</li>`)}</ul>` : html`<p class="hand-note">Nothing left to guess.</p>`}`;
}

function checkerView(ctx: LearnContext): Html {
  const note = ctx.demo ? html`<p class="muted small">In the app, the checker grades as you type with the same rules as your score.</p>` : "";
  return html`<section class="card checker">
    <div>
      <div class="eyebrow">Prompt checker</div>
      <h2 class="q">Will it have to guess?</h2>
      <label class="sr-only" for="draft">Your prompt</label>
      <textarea id="draft" data-action="draft" rows="6" maxlength="8000" spellcheck="true" placeholder="Fix the crash in checkout.ts when the cart is empty. Done when the tests pass. Don't change the API.">${ctx.draft}</textarea>
      <div class="share"><button class="ghost" data-action="copy-draft">Copy</button><button class="ghost" data-action="clear-draft">Clear</button><span class="sr-only" id="draft-status" role="status" aria-live="polite"></span></div>
      <p class="muted small">Nothing you type here is saved.</p>
    </div>
    <div class="check-result" id="check-result">${checkResultView(ctx.meta, null, true)}${note}</div>
    <span class="sr-only" id="check-status" role="status"></span>
  </section>`;
}

function lessonRow(l: Lesson, done: readonly string[]): Html {
  const d = done.includes(l.id);
  return html`<li><button class="lesson-row${d ? " done" : ""}" data-action="lesson" data-id="${l.id}"><span aria-hidden="true">${d ? "✓" : "○"}</span>${l.title}${d ? html`<span class="sr-only"> (done)</span>` : ""}</button></li>`;
}

function trackView(t: (typeof TRACKS)[number], done: readonly string[]): Html {
  const n = t.lessons.filter((l) => done.includes(l.id)).length;
  const all = n === t.lessons.length;
  return html`<article class="card mini track${all ? " finished" : ""}">
    <div class="eyebrow">${all ? "Track done 🏅" : `${n} of ${t.lessons.length} done`}</div>
    <h2>${t.title}</h2>
    <p class="muted">${t.blurb}</p>
    <ul class="lessons">${t.lessons.map((l) => lessonRow(l, done))}</ul>
  </article>`;
}

function homeView(ctx: LearnContext): Html {
  const done = ctx.done.filter((id) => LESSONS.some((l) => l.id === id));
  const next = recommended(ctx.prompts, ctx.meta, done);
  const graduated = done.length === LESSONS.length;
  return html`
    <section class="toolbar"><span class="eyebrow">Learn to prompt it · ${done.length} of ${LESSONS.length} lessons</span></section>
    ${checkerView(ctx)}
    ${
      graduated
        ? html`<section class="card mini graduate"><div class="eyebrow">All three tracks</div><h2 class="q">Graduate 🎓</h2><p>Every lesson done. Keep the checker handy, and watch Improve to see it pay off.</p></section>`
        : next
          ? html`<section class="next-lesson"><span class="eyebrow">${ctx.prompts?.tasks ? "Picked for you, from your prompts" : "Start here"}</span>
            <button class="lesson-next" data-action="lesson" data-id="${next.id}"><b>${next.title}</b><span class="muted">${next.why}</span><span class="go" aria-hidden="true">→</span></button></section>`
          : ""
    }
    <section class="tracks">${TRACKS.map((t) => trackView(t, done))}</section>`;
}

function questionView(q: Question, li: number, qi: number, meta: Meta): Html {
  const name = `q-${li}-${qi}`;
  const label = (o: string) => (q.kind === "missing" ? signalTitle(meta, o) : o);
  return html`<fieldset class="quiz-q" data-answer="${answerIndex(q)}">
    <legend>${q.question}</legend>
    ${q.kind === "missing" ? html`<blockquote class="prompt-text">${q.prompt}</blockquote>` : ""}
    ${q.options.map(
      (o, oi) => html`<label class="quiz-opt"><input type="radio" name="${name}" value="${oi}" data-action="quiz"/><span>${label(o)}</span></label>`,
    )}
    <p class="quiz-feedback" role="status" aria-live="polite" data-explain="${q.explain}"></p>
  </fieldset>`;
}

function lessonView(l: Lesson, ctx: LearnContext): Html {
  const done = ctx.done.includes(l.id);
  const track = trackOf(l.id);
  const i = LESSONS.indexOf(l);
  const next = LESSONS[i + 1];
  return html`
    <section class="toolbar"><button class="ghost small" data-action="lesson" data-id="">← All lessons</button><span class="eyebrow">${track?.title ?? ""}</span></section>
    <article class="lesson">
      <h1 class="q">${l.title}</h1>
      <p class="lead">${l.why}</p>
      <div class="compare">
        <div class="ex before"><div class="eyebrow">Before</div><blockquote class="prompt-text">${l.before.text}</blockquote>${chips(ctx.meta, l.before.signals)}</div>
        <div class="ex after"><div class="eyebrow">After</div><blockquote class="prompt-text">${l.after.text}</blockquote>${chips(ctx.meta, l.after.signals)}</div>
      </div>
      <section><div class="head"><h2>Template</h2></div>
        <p class="template mono">${l.template}</p>
        <button class="ghost" data-action="try-template" data-id="${l.id}">Try it in the checker</button>
      </section>
      <section><div class="head"><h2>Tips</h2></div><ul class="tips">${l.tips.map((t) => html`<li>${t}</li>`)}</ul></section>
      <section class="quiz"><div class="head"><h2>Quick check</h2></div>${l.quiz.map((q, qi) => questionView(q, i, qi, ctx.meta))}</section>
      <div class="share">
        <button data-action="lesson-done" data-id="${l.id}" data-done="${String(!done)}">${done ? "Mark not done" : "Mark done ✓"}</button>
        ${next ? html`<button class="ghost" data-action="lesson" data-id="${next.id}">Next: ${next.title} →</button>` : ""}
      </div>
    </article>`;
}

export function learnView(ctx: LearnContext): Html {
  const l = lessonById(ctx.lesson);
  return l ? lessonView(l, ctx) : homeView(ctx);
}

/** Marks a quiz option right or wrong in place (no redraw, so the page doesn't jump). */
export function judgeQuiz(input: HTMLInputElement): void {
  const set = input.closest("fieldset");
  const out = set?.querySelector<HTMLElement>(".quiz-feedback");
  if (!set || !out) return;
  const right = Number(set.dataset.answer) === Number(input.value);
  for (const l of set.querySelectorAll(".quiz-opt")) l.classList.remove("right", "wrong");
  input.closest(".quiz-opt")?.classList.add(right ? "right" : "wrong");
  out.textContent = `${right ? "Right." : "Not quite."} ${out.dataset.explain ?? ""}`;
}
