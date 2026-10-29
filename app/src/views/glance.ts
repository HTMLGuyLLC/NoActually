// Quick glance: the small window under the menu bar icon.
import type { Meta, Stats } from "../api";
import { type Html, html, num } from "../html";
import { briefTier, landedPct, tierName, tierOf } from "../meta";
import { wordmarkView } from "./chrome";

export function glanceView(s: Stats, meta: Meta): Html {
  const o = s.overall;
  const no = o.verdict === "no";
  const next = tierOf(meta, o.next_tier);
  const progress =
    o.score == null ? `${o.to_rank} more ${o.to_rank === 1 ? "task" : "tasks"} to get a score` : next ? `${next.name} at ${next.floor}` : "Top tier";
  return html`<div class="glance">
    <div class="g-top">${wordmarkView("brand small")}${o.tier ? html`<span class="tier-label"><span>Score ${o.score}</span>${tierName(meta, o.tier)}</span>` : ""}</div>
    <div class="g-q">Could AI replace you?</div>
    <div class="g-row"><div class="no sm${no ? "" : " maybe"}">${no ? "No." : "Hmm."}</div>
      <div class="because">You caught it <b>${s.catches_last_7d}×</b> this week.</div></div>
    <div class="g-score"><div><span class="n you">${num(s.catches)}</span><span class="eyebrow">you</span></div><div class="vs">vs</div><div><span class="n ai">${num(s.humbled)}</span><span class="eyebrow">AI</span></div></div>
    <div class="g-meta muted small">${s.current_streak_days ? `${s.current_streak_days}-day streak · ` : ""}${progress}</div>
    ${briefsLine(s, meta)}
    <div class="g-actions"><button data-action="open-main">Share or see more</button><button class="ghost" data-action="copy-share">Copy text</button><span class="sr-only" id="copy-status" role="status" aria-live="polite"></span></div>
  </div>`;
}

/** "Briefs: Tech lead · 86% landed", once there's a brief tier. */
function briefsLine(s: Stats, meta: Meta): Html | "" {
  const tier = briefTier(meta, s.briefs.tier);
  return tier ? html`<div class="g-meta muted small">Briefs: ${tier.name} · ${landedPct(s.briefs)}% landed</div>` : "";
}

/** Before the first catch. `watching` names the sources being read right now (empty: none allowed yet). */
export function emptyGlanceView(watching: string): Html {
  return watching
    ? html`<div class="glance empty-glance"><span class="eyebrow"><i class="dot live"></i> Watching ${watching}</span><div class="g-q">No catches yet.</div><p class="muted small">Your score shows up after your first catch.</p><button class="ghost" data-action="open-main">Open No, Actually...</button></div>`
    : html`<div class="glance empty-glance"><div class="g-q">Could AI replace you?</div><p class="muted small">Pick which AI tools to count, then find out.</p><button data-action="open-main">Set up</button></div>`;
}
