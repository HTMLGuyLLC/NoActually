// Share text, network links and the dialog's guidance. All made on this machine from numbers only
// (or, for a single catch, the stored and already redacted snippet the person can edit first).
import type { Briefs, BriefTier, Stats } from "../api";
import { type Html, html } from "../html";
import { landedPct } from "../meta";

export const SHARE_URL = "https://github.com/HTMLGuyLLC/NoActually";
/** The link as printed on the cards: "github.com/HTMLGuyLLC/NoActually". */
export const SHARE_URL_SHORT = SHARE_URL.replace(/^https:\/\//, "");

type ScoreLike = Pick<Stats, "verdict" | "catches_last_7d" | "pct_real_chats_with_catch" | "catches" | "humbled">;

export function shareText(s: ScoreLike, who: string) {
  const pct = s.pct_real_chats_with_catch != null ? `, and in ${Math.round(s.pct_real_chats_with_catch)}% of my chats` : "";
  const times = s.catches_last_7d === 1 ? "time" : "times";
  return `Could ${who} replace me? ${s.verdict === "no" ? "No." : "Maybe…"} I caught it ${s.catches_last_7d} ${times} this week${pct}. Me ${s.catches}, ${who} ${s.humbled}. What's your score? ${SHARE_URL}`;
}

/** The briefs score, ready to post: the tier's answer and how many tasks landed. Numbers only. */
export function briefShareText(b: Pick<Briefs, "landed_last_28d" | "tasks_last_28d">, tier: BriefTier) {
  return `Do I make my AI guess? ${tier.verdict} ${landedPct(b)}% of my tasks in the last 28 days landed without me adding, clarifying or starting over. Brief tier: ${tier.name}. Do you make yours guess? ${SHARE_URL}`;
}

import type { CardShape } from "./cards";

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
 * score that's numbers and the project link; for a catch it's the redacted, clipped snippet,
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

type CatchLike = { model: string; human: string | null; ai: string | null };

export function clip(s: string, n: number) {
  const t = s.replace(/\s+/g, " ").trim();
  return t.length > n ? `${t.slice(0, n - 1).trimEnd()}…` : t;
}

/** A single catch, ready to post. Quotes are the stored (already redacted) snippets, clipped short. */
export function catchShareText(e: CatchLike, url = SHARE_URL) {
  const who = e.model || "my AI";
  const you = e.human ? ` Me: “${clip(e.human, 90)}”` : "";
  const ai = e.ai ? ` ${who}: “${clip(e.ai, 90)}”` : "";
  return `I caught ${who} being wrong.${you}${ai} Could AI replace me? No, actually. ${url}`;
}
