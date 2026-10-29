// Share text, network links and the dialog's guidance. All made on this machine from numbers only
// (or, for a single correction, the stored and already redacted snippet the person can edit first).
import type { Overall, Prompts, PromptTier, Stats } from "../api";
import { plural } from "../format";
import { type Html, html } from "../html";
import { landedPct } from "../meta";
import { PERIOD_WHEN } from "../state";
import type { CardShape } from "./cards";

export const SHARE_URL = "https://github.com/HTMLGuyLLC/NoActually";
/** The link as printed on the cards: "github.com/HTMLGuyLLC/NoActually". */
export const SHARE_URL_SHORT = SHARE_URL.replace(/^https:\/\//, "");

type ScoreLike = Pick<Stats, "overall" | "corrections_last_7d" | "corrections" | "humbled">;

/** The score, ready to post: the answer, the score and its two halves. Numbers only. */
export function shareText(s: ScoreLike, who: string, tier: string, when = PERIOD_WHEN["28d"]) {
  const o = s.overall;
  const answer = o.verdict === "no" ? "No." : "Maybe…";
  if (o.score == null) {
    return `Could ${who} replace me? ${answer} I corrected it ${plural(s.corrections_last_7d, "time")} this week. Me ${s.corrections}, ${who} ${s.humbled}. What's your score? ${SHARE_URL}`;
  }
  return `Could ${who} replace me? ${answer} My score is ${o.score} (${tier}): ${o.direction}% of my tasks landed without steering, and I corrected it ${plural(o.corrections, "time")} to its ${o.humbled} ${when}. What's yours? ${SHARE_URL}`;
}

/** The prompts score, ready to post: the tier's answer and how many tasks landed. Numbers only. */
export function promptShareText(b: Pick<Prompts, "window_landed" | "window_tasks">, tier: PromptTier, when = PERIOD_WHEN["28d"]) {
  return `Do I make my AI guess? ${tier.verdict} ${landedPct(b)}% of my tasks ${when} landed without me adding, clarifying or starting over. Prompt tier: ${tier.name}. Do you make yours guess? ${SHARE_URL}`;
}

/** The corrections answer, ready to post: the tier's word and who corrected whom. Numbers only. */
export function correctionsShareText(o: Pick<Overall, "corrections" | "humbled">, tier: PromptTier, when = PERIOD_WHEN["28d"]) {
  return `Do I correct my AI's mistakes? ${tier.verdict} I corrected it ${plural(o.corrections, "time")} ${when}, and it corrected me ${plural(o.humbled, "time")}. Correction tier: ${tier.name}. Do you correct yours? ${SHARE_URL}`;
}

export type ShareTarget = {
  id: string;
  label: string;
  /** Where to send the person. Null means the network has no web share link (Instagram). */
  href: string | null;
  /** The network ignores pre-filled text, so the text is copied for pasting. */
  pasteText?: boolean;
  /** The image shape the network shows best, when it posts the saved card. */
  card?: CardShape;
};

/**
 * Each network's own "share" link, pre-filled with `text` where the network allows it. For a
 * score that's numbers and the project link; for a correction it's the redacted, clipped snippet,
 * which the share dialog shows (and lets the person edit) before anything opens.
 */
export function shareTargets(text: string, url = SHARE_URL): ShareTarget[] {
  const t = encodeURIComponent(text);
  const u = encodeURIComponent(url);
  // Networks that take one text field get the link inside the text (it already ends with it).
  return [
    { id: "x", label: "X", href: `https://x.com/intent/post?text=${t}` },
    { id: "linkedin", label: "LinkedIn", href: `https://www.linkedin.com/feed/?shareActive=true&text=${t}` },
    { id: "facebook", label: "Facebook", href: `https://www.facebook.com/sharer/sharer.php?u=${u}`, pasteText: true },
    { id: "threads", label: "Threads", href: `https://www.threads.net/intent/post?text=${t}` },
    { id: "bluesky", label: "Bluesky", href: `https://bsky.app/intent/compose?text=${t}` },
    { id: "reddit", label: "Reddit", href: `https://www.reddit.com/submit?url=${u}&title=${encodeURIComponent(text.replace(` ${url}`, ""))}` },
    { id: "whatsapp", label: "WhatsApp", href: `https://wa.me/?text=${t}` },
    { id: "instagram", label: "Instagram", href: null, pasteText: true, card: "portrait" },
  ];
}

/** Hosts the app may open. The Rust side checks the same list before opening anything. */
export const SHARE_HOSTS = ["x.com", "www.linkedin.com", "www.facebook.com", "www.threads.net", "bsky.app", "www.reddit.com", "wa.me", "github.com"];

/** What the share dialog does for a network, and what it tells the person. */
export type ShareGuide = {
  /** Save the card image first (Instagram); the guide is shown only once it's saved. */
  saveImage: boolean;
  /** Open the network's link. */
  open: boolean;
  guide: Html;
  /** The loud, can't-miss version: the person has to paste. */
  paste: boolean;
};

/** Decides the share dialog's next step for `target`, given whether the text reached the clipboard. */
export function shareGuide(target: ShareTarget, copied: boolean, pasteKey: string): ShareGuide {
  const kbd = html`<kbd>${pasteKey}</kbd>`;
  if (!target.href) {
    // Instagram can't be pre-filled from outside the app: hand over the image and the caption.
    return {
      saveImage: true,
      open: false,
      paste: true,
      guide: html`<b>Card saved.</b> Post the image from the Instagram app, then press ${kbd} in the caption to paste your text${copied ? "" : " (copy it above first)"}.`,
    };
  }
  if (target.pasteText) {
    const note = copied
      ? html`Your text is copied. When ${target.label} opens, click into the post and press ${kbd} to paste it.`
      : html`Copy the text above, then paste it into the post.`;
    return { saveImage: false, open: true, paste: true, guide: html`<b>Paste your text.</b> ${note}` };
  }
  return { saveImage: false, open: true, paste: false, guide: html`Opening ${target.label} with your post filled in…` };
}

export type CorrectionLike = { model: string; human: string | null; ai: string | null };

export function clip(s: string, n: number) {
  const t = s.replace(/\s+/g, " ").trim();
  return t.length > n ? `${t.slice(0, n - 1).trimEnd()}…` : t;
}

/** A single correction, ready to post. Quotes are the stored (already redacted) snippets, clipped short. */
export function correctionShareText(e: CorrectionLike, url = SHARE_URL) {
  const who = e.model || "my AI";
  const you = e.human ? ` Me: “${clip(e.human, 90)}”` : "";
  const ai = e.ai ? ` ${who}: “${clip(e.ai, 90)}”` : "";
  return `I corrected ${who}.${you}${ai} Could AI replace me? No, actually. ${url}`;
}
