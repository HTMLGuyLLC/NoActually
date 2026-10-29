// Quick glance: the small window under the menu bar icon. The three answers you'd open the app
// for (could it replace you, how you catch it, how you brief it) and your goal, nothing more.
import type { GoalStatus, Meta, Stats } from "../api";
import { type Html, html, num } from "../html";
import { briefTier, landedPct, tierName, tierOf } from "../meta";
import { TAB_ICONS, wordmarkView } from "./chrome";
import { goalTitle } from "./improve";

const topView = () =>
  html`<div class="g-top">${wordmarkView("brand small")}<button class="g-gear" data-action="open-main" data-id="settings" aria-label="Settings">${TAB_ICONS.settings}</button></div>`;

const openView = (label = "Open app") => html`<div class="g-actions"><button data-action="open-main">${label}</button></div>`;

/** The score, its tier, and how far the next tier is. */
function scoreView(s: Stats, meta: Meta): Html {
  const o = s.overall;
  if (o.score == null)
    return html`<div class="g-tier"><span class="muted small">${o.to_rank} more ${o.to_rank === 1 ? "task" : "tasks"} to get a score</span></div>`;
  const next = tierOf(meta, o.next_tier);
  return html`<div class="g-tier">
    <span class="tier-label"><span>Score ${o.score}</span>${tierName(meta, o.tier)}</span>
    ${next ? html`<span class="bar-bg" role="img" aria-label="Score ${o.score} of ${next.floor}"><i data-w="${Math.min(100, Math.round((100 * o.score) / next.floor))}"></i></span>` : ""}
    <span class="muted small">${next ? `${next.name} at ${next.floor}` : "Top tier"}</span>
  </div>`;
}

/** Catches and briefs side by side, each in a word or two. */
function pairView(s: Stats, meta: Meta): Html {
  const tier = briefTier(meta, s.briefs.tier);
  const week = s.catches_last_7d ? `${s.catches_last_7d}× this week` : "None this week";
  return html`<div class="g-pair">
    <div><span class="eyebrow">Catches</span><span class="g-n"><b class="you">${num(s.catches)}</b> <span class="muted">vs</span> <b class="ai">${num(s.humbled)}</b></span><span class="muted small">${week}</span></div>
    <div><span class="eyebrow">Briefs</span><span class="g-n"><b>${tier ? tier.name : "Not ranked"}</b></span><span class="muted small">${s.briefs.tasks ? `${landedPct(s.briefs)}% landed` : "No tasks yet"}</span></div>
  </div>`;
}

/** Your goal and this week's number, or nothing when there's no goal. */
function goalView(st: GoalStatus | null, meta: Meta): Html | "" {
  if (!st) return "";
  const g = st.goal;
  const w = st.weeks.at(-1);
  const line = st.done
    ? "Done 🏆"
    : g.kind === "tier"
      ? st.score == null
        ? "Not scored yet"
        : `Score ${st.score} of ${meta.tiers.find((t) => t.id === g.tier)?.floor ?? "?"}`
      : w?.state === "open"
        ? `${w.value}% this week · aim for ${g.kind === "habit" ? "at least" : "at most"} ${g.target}%`
        : `${st.met} of ${st.needed} weeks met`;
  return html`<div class="g-goal"><span class="eyebrow">Your goal</span><span class="g-goal-title">${goalTitle(g, meta)}</span><span class="muted small">${line}</span></div>`;
}

export function glanceView(s: Stats, meta: Meta, goal: GoalStatus | null = null): Html {
  const no = s.overall.verdict === "no";
  return html`<div class="glance">
    ${topView()}
    <div class="g-q">Could AI replace you?</div>
    <div class="g-row"><div class="no sm${no ? "" : " maybe"}">${no ? "No." : "Hmm."}</div>${scoreView(s, meta)}</div>
    ${pairView(s, meta)}
    ${goalView(goal, meta)}
    ${openView()}
  </div>`;
}

/** Before the first catch. `watching` names the sources being read right now (empty: none allowed yet). */
export function emptyGlanceView(watching: string): Html {
  return watching
    ? html`<div class="glance empty-glance">${topView()}<span class="eyebrow"><i class="dot live"></i> Watching ${watching}</span><div class="g-q">No catches yet.</div><p class="muted small">Your score shows up after your first catch.</p>${openView()}</div>`
    : html`<div class="glance empty-glance">${topView()}<div class="g-q">Could AI replace you?</div><p class="muted small">Pick which AI tools to count, then find out.</p>${openView("Set up")}</div>`;
}
