// Quick glance: the small window under the menu bar icon. The three answers you'd open the app
// for (could it replace you, do you make it guess, do you correct it) and your goal, nothing more.
import type { GoalStatus, Meta, Stats } from "../api";
import { last, plural } from "../format";
import { type Html, html } from "../html";
import { correctionTier, landedPct, promptTier, tierName, tierOf } from "../meta";
import { TAB_ICONS, wordmarkView } from "./chrome";
import { goalTitle } from "./improve";

const topView = () =>
  html`<div class="g-top">${wordmarkView("brand small")}<button class="g-gear" data-action="open-main" data-id="settings" aria-label="Settings">${TAB_ICONS.settings}</button></div>`;

const openView = (label = "Open app") => html`<div class="g-actions"><button data-action="open-main">${label}</button></div>`;

/** One question, its one-word answer, and a line on why. `word` is null until it's ranked. */
const answerView = (ask: string, word: string | null, why: string) =>
  html`<div class="g-ans"><span class="g-ask">${ask}</span><div class="g-line"><span class="g-word${word ? "" : " maybe"}">${word ?? "Hmm."}</span><span class="muted small">${why}</span></div></div>`;

/** The three answers the app gives, as the scorecard words them, over the last 28 days. */
function answersView(s: Stats, meta: Meta): Html {
  const o = s.overall;
  const replace = o.score == null ? `${plural(o.to_rank, "more task")} to get a score` : `Score ${o.score} · ${tierName(meta, o.tier)}`;
  const prompt = promptTier(meta, s.prompts.tier);
  const guess = prompt ? `${landedPct(s.prompts)}% clear · ${prompt.name}` : `${plural(s.prompts.to_rank, "more task")} to get ranked`;
  const corrected = correctionTier(meta, o.correction_tier);
  const check = corrected ? `${plural(o.corrections, "correction")} · ${corrected.name}` : `${plural(o.to_rank, "more task")} to get ranked`;
  return html`<div class="g-answers">
    ${answerView("Could AI replace you?", o.score == null ? null : o.verdict === "no" ? "No." : "Hmm.", replace)}
    ${answerView("Do you make it guess?", prompt?.verdict ?? null, guess)}
    ${answerView("Do you correct it?", corrected?.verdict ?? null, check)}
  </div>`;
}

/** Your goal and this week's number, or nothing when there's no goal. */
function goalView(st: GoalStatus | null, meta: Meta): Html | "" {
  if (!st) return "";
  const g = st.goal;
  const w = last(st.weeks);
  const line = st.done
    ? "Done 🏆"
    : g.kind === "tier"
      ? st.score == null
        ? "Not scored yet"
        : `Score ${st.score} of ${tierOf(meta, g.tier)?.floor ?? "?"}`
      : w?.state === "open"
        ? `${w.value}% this week · aim for ${g.kind === "habit" ? "at least" : "at most"} ${g.target}%`
        : `${st.met} of ${st.needed} weeks met`;
  return html`<div class="g-goal"><span class="eyebrow">Your goal</span><span class="g-goal-title">${goalTitle(g, meta)}</span><span class="muted small">${line}</span></div>`;
}

export function glanceView(s: Stats, meta: Meta, goal: GoalStatus | null = null): Html {
  return html`<div class="glance">
    ${topView()}
    ${answersView(s, meta)}
    ${goalView(goal, meta)}
    ${openView()}
  </div>`;
}

/** Before the first correction. `watching` names the sources being read right now (empty: none allowed yet). */
export function emptyGlanceView(watching: string): Html {
  return watching
    ? html`<div class="glance empty-glance">${topView()}<span class="eyebrow"><i class="dot live"></i> Watching ${watching}</span><div class="g-q">No corrections yet.</div><p class="muted small">Your score shows up after your first correction.</p>${openView()}</div>`
    : html`<div class="glance empty-glance">${topView()}<div class="g-q">Could AI replace you?</div><p class="muted small">Pick which AI tools to count, then find out.</p>${openView("Set up")}</div>`;
}
