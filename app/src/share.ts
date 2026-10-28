// Share text and the share card image. Both are made on this machine from numbers only.
import type { Stats } from "./api";

export const SHARE_URL = "https://github.com/HTMLGuyLLC/NoActually";

export function verdictIsNo(s: Pick<Stats, "catches_last_7d">) {
  return s.catches_last_7d >= 3;
}

export function shareText(s: Stats, who: string) {
  const pct = s.pct_real_chats_with_catch != null ? `, and in ${Math.round(s.pct_real_chats_with_catch)}% of my chats` : "";
  const times = s.catches_last_7d === 1 ? "time" : "times";
  return `Could ${who} replace me? ${verdictIsNo(s) ? "No." : "Maybe…"} I caught it ${s.catches_last_7d} ${times} this week${pct}. Me ${s.catches}, ${who} ${s.humbled}. What's your score? ${SHARE_URL}`;
}

export type ShareTarget = {
  id: string;
  label: string;
  /** Where to send the person. Null means the network has no web share link (Instagram). */
  href: string | null;
  /** The network ignores pre-filled text, so the text is copied for pasting. */
  pasteText?: boolean;
};

/**
 * Each network's own "share" link, pre-filled where the network allows it. Only numbers and the
 * project link go into these URLs; chat text never does.
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
    { id: "instagram", label: "Instagram", href: null, pasteText: true },
  ];
}

/** Hosts the app may open. The Rust side checks the same list before opening anything. */
export const SHARE_HOSTS = ["x.com", "www.linkedin.com", "www.facebook.com", "www.threads.net", "bsky.app", "www.reddit.com", "wa.me", "github.com"];

/** Draws the 1200×630 card (the size social sites use for link previews) in the light theme. */
export async function drawShareCard(s: Stats, who: string): Promise<Blob> {
  const W = 1200, H = 630;
  const c = document.createElement("canvas");
  c.width = W * 2;
  c.height = H * 2; // 2× for crisp text on high-density screens
  const g = c.getContext("2d")!;
  g.scale(2, 2);
  await Promise.all(
    ["800 64px 'Bricolage Grotesque Variable'", "400 200px 'Gochi Hand'", "500 24px 'Instrument Sans'", "600 18px 'JetBrains Mono'"].map((f) => document.fonts.load(f)),
  );
  const ink = "#141a2e", muted = "#5d6680", pen = "#e0322b", ai = "#3b5bdb", rule = "#dfe3ee";

  g.fillStyle = "#fbfcfe";
  g.fillRect(0, 0, W, H);
  g.strokeStyle = rule;
  g.lineWidth = 1;
  for (let y = 40; y < H; y += 40) {
    g.beginPath();
    g.moveTo(0, y + 0.5);
    g.lineTo(W, y + 0.5);
    g.stroke();
  }
  g.fillStyle = pen;
  g.fillRect(96, 0, 2, H); // the margin line of a legal pad

  g.textBaseline = "alphabetic";
  g.fillStyle = ink;
  g.font = "800 30px 'Bricolage Grotesque Variable'";
  g.fillText("no,", 140, 78);
  const markGap = g.measureText("no, ").width; // measure in the wordmark's own font
  g.fillStyle = pen;
  g.font = "400 36px 'Gochi Hand'";
  g.fillText("actually.", 140 + markGap, 80);

  g.fillStyle = ink;
  g.font = "800 64px 'Bricolage Grotesque Variable'";
  const q = `Could ${who} replace me?`;
  let size = 64;
  while (g.measureText(q).width > W - 240 && size > 36) g.font = `800 ${(size -= 4)}px 'Bricolage Grotesque Variable'`;
  g.fillText(q, 140, 190);

  g.save();
  g.translate(140, 400);
  g.rotate((-6 * Math.PI) / 180);
  g.fillStyle = verdictIsNo(s) ? pen : muted;
  g.font = "400 200px 'Gochi Hand'";
  g.fillText(verdictIsNo(s) ? "No." : "Hmm.", 0, 0);
  g.restore();

  g.fillStyle = ink;
  g.font = "500 30px 'Instrument Sans'";
  const line1 = `I caught it ${s.catches_last_7d} ${s.catches_last_7d === 1 ? "time" : "times"} this week.`;
  g.fillText(line1, 520, 310);
  if (s.pct_real_chats_with_catch != null) {
    g.fillStyle = muted;
    g.fillText(`It slipped up in ${Math.round(s.pct_real_chats_with_catch)}% of my chats.`, 520, 354);
  }

  // Tier pill
  g.font = "600 18px 'JetBrains Mono'";
  const tier = s.tier.toUpperCase();
  const tw = g.measureText(tier).width + 36;
  g.fillStyle = pen;
  g.beginPath();
  g.roundRect(520, 384, tw, 40, 20);
  g.fill();
  g.fillStyle = "#ffffff";
  g.fillText(tier, 538, 410);

  // Score line
  g.fillStyle = ink;
  g.fillRect(140, 470, W - 280, 3);
  g.font = "600 16px 'JetBrains Mono'";
  g.fillStyle = muted;
  g.fillText("I CAUGHT IT", 140, 510);
  g.textAlign = "right";
  g.fillText("IT CAUGHT ME", W - 140, 510);
  g.font = "800 64px 'Bricolage Grotesque Variable'";
  g.fillStyle = ai;
  g.fillText(String(s.humbled), W - 140, 580);
  g.textAlign = "left";
  g.fillStyle = pen;
  g.fillText(String(s.catches), 140, 580);
  g.textAlign = "center";
  g.fillStyle = muted;
  g.font = "400 34px 'Gochi Hand'";
  g.fillText("vs", W / 2, 566);
  g.font = "500 18px 'Instrument Sans'";
  g.fillText("What's your score?  github.com/HTMLGuyLLC/NoActually", W / 2, 612);

  return new Promise((resolve, reject) => c.toBlob((b) => (b ? resolve(b) : reject(new Error("Couldn't draw the card."))), "image/png"));
}
