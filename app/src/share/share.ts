// Share text, network links and the dialog's guidance. All made on this machine from numbers only
// (or, for a single correction, the redacted snippet the person can edit first).
import type { Overall, Prompts, PromptTier, Stats } from "../api";
import { plural } from "../format";
import { type Html, html } from "../html";
import { landedPct } from "../meta";
import { PERIOD_WHEN } from "../state";
import type { CardShape } from "./cards";

export const SHARE_URL = "https://github.com/HTMLGuyLLC/NoActually";
/** The link as printed on the cards: "github.com/HTMLGuyLLC/NoActually". */
export const SHARE_URL_SHORT = SHARE_URL.replace(/^https:\/\//, "");

type ScoreLike = Pick<Stats, "overall">;

/** The score, ready to post: the answer, the score and its two halves. Numbers only. Only offered once there is a tier. */
export function shareText(s: ScoreLike, who: string, tier: string, when = PERIOD_WHEN["28d"]) {
  const o = s.overall;
  const answer = o.verdict === "no" ? "No." : "Hmm.";
  return `Could ${who} replace me? ${answer} My score is ${o.score} (${tier}): ${o.direction}% of my prompts were clear the first time. I corrected it ${plural(o.corrections, "time")} ${when}, and it corrected me ${plural(o.humbled, "time")}. What's yours? ${SHARE_URL}`;
}

/** The prompts score, ready to post: the tier's answer and how many prompts were clear the first time. Numbers only. */
export function promptShareText(b: Pick<Prompts, "window_landed" | "window_tasks">, tier: PromptTier, when = PERIOD_WHEN["28d"]) {
  return `Do I make my AI guess? ${tier.verdict} ${landedPct(b)}% of my prompts ${when} were clear the first time. Prompt tier: ${tier.name}. Do you make yours guess? ${SHARE_URL}`;
}

/** The corrections answer, ready to post: the tier's word and who corrected whom. Numbers only. */
export function correctionsShareText(o: Pick<Overall, "corrections" | "humbled">, tier: PromptTier, when = PERIOD_WHEN["28d"]) {
  return `Do I correct my AI? ${tier.verdict} I corrected it ${plural(o.corrections, "time")} ${when}, and it corrected me ${plural(o.humbled, "time")}. Correction tier: ${tier.name}. Do you correct yours? ${SHARE_URL}`;
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
export function shareTargets(rawText: string, url = SHARE_URL): ShareTarget[] {
  // A half of an emoji, from an edit or an older cut, would make encoding throw.
  const text = rawText.replace(/\p{Cs}/gu, "");
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
  const chars = [...t]; // whole characters, so an emoji is never cut in half
  return chars.length > n
    ? `${chars
        .slice(0, n - 1)
        .join("")
        .trimEnd()}…`
    : t;
}

/** The longest quote, in chars, in the post text and on the card image alike, so the image never shows more than the box. */
export const QUOTE_CLIP = 90;

/** The fewest characters a quote is cut to, however long the rest of the post is. */
const QUOTE_FLOOR = 20;
/** The longest post X takes (a link counts as `X_LINK`), and Bluesky (a link counts in full). */
const X_LIMIT = 280;
const X_LINK = 23;
const BLUESKY_LIMIT = 300;

const quoted = (e: CorrectionLike, n: number, url: string) => {
  const who = e.model || "my AI";
  const you = e.human ? ` Me: “${clip(e.human, n)}”` : "";
  const ai = e.ai ? ` ${who}: “${clip(e.ai, n)}”` : "";
  return `I corrected ${who}.${you}${ai} Could AI replace me? No, actually. ${url}`;
};

/**
 * How many characters each quote keeps: `QUOTE_CLIP`, or fewer when two long quotes and a long model name
 * would push the post over what X and Bluesky take. Both quotes shrink together. The card image uses it too.
 */
export function quoteClip(e: CorrectionLike, url = SHARE_URL): number {
  const fits = (t: string) => t.length <= BLUESKY_LIMIT && t.length - url.length + X_LINK <= X_LIMIT;
  let n = QUOTE_CLIP;
  while (n > QUOTE_FLOOR && !fits(quoted(e, n, url))) n--;
  return n;
}

/** A single correction, ready to post. Quotes are the (redacted, possibly edited) snippets, clipped to `quoteClip`. */
export function correctionShareText(e: CorrectionLike, url = SHARE_URL) {
  return quoted(e, quoteClip(e, url), url);
}
