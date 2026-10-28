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
    { id: "instagram", label: "Instagram", href: null, pasteText: true },
  ];
}

/** Hosts the app may open. The Rust side checks the same list before opening anything. */
export const SHARE_HOSTS = ["x.com", "www.linkedin.com", "www.facebook.com", "www.threads.net", "bsky.app", "www.reddit.com", "wa.me", "github.com"];

/** The app icon's underlined check, then "No, Actually...", with the text baseline at `y`. */
function drawWordmark(g: CanvasRenderingContext2D, x: number, y: number, pen: string, ink: string) {
  const size = 36;
  const k = size / 44;
  g.save();
  g.translate(x, y - size + 6);
  g.scale(k, k);
  g.lineCap = "round";
  g.lineJoin = "round";
  g.strokeStyle = pen;
  g.lineWidth = 5.5;
  g.beginPath();
  g.moveTo(9, 23);
  g.lineTo(17, 31);
  g.lineTo(35, 11);
  g.stroke();
  g.strokeStyle = "#e2b600";
  g.lineWidth = 3.2;
  g.beginPath();
  g.moveTo(8, 38);
  g.quadraticCurveTo(22, 35, 36, 38);
  g.stroke();
  g.restore();
  const tx = x + size + 8;
  g.fillStyle = ink;
  g.font = "800 30px 'Bricolage Grotesque Variable'";
  g.fillText("No,", tx, y);
  const gap = g.measureText("No, ").width; // measured in the wordmark's own font
  g.fillStyle = pen;
  g.font = "400 36px 'Gochi Hand'";
  g.fillText("Actually...", tx + gap, y + 2);
}

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
  drawWordmark(g, 140, 78, pen, ink);

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

type CatchLike = { model: string; human: string | null; ai: string | null };

function clip(s: string, n: number) {
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

/** Wraps text to lines no wider than `max` px in the current font. */
function wrap(g: CanvasRenderingContext2D, text: string, max: number, maxLines: number) {
  const words = text.split(" ");
  const lines: string[] = [];
  let line = "";
  for (const w of words) {
    const next = line ? `${line} ${w}` : w;
    if (g.measureText(next).width > max && line) {
      lines.push(line);
      line = w;
      if (lines.length === maxLines) break;
    } else line = next;
  }
  if (lines.length < maxLines && line) lines.push(line);
  if (lines.length === maxLines && words.join(" ") !== lines.join(" ")) lines[maxLines - 1] = lines[maxLines - 1].replace(/\s*\S*$/, "…");
  return lines;
}

/** A 1200×630 card for one catch: your words in red pen, the AI's admission underneath. */
export async function drawCatchCard(e: CatchLike): Promise<Blob> {
  const W = 1200, H = 630;
  const c = document.createElement("canvas");
  c.width = W * 2;
  c.height = H * 2;
  const g = c.getContext("2d")!;
  g.scale(2, 2);
  await Promise.all(
    ["800 54px 'Bricolage Grotesque Variable'", "400 52px 'Gochi Hand'", "500 26px 'Instrument Sans'", "600 16px 'JetBrains Mono'"].map((f) => document.fonts.load(f)),
  );
  const ink = "#141a2e", muted = "#5d6680", pen = "#e0322b", ai = "#3b5bdb", aiSoft = "#e7ecfd", rule = "#dfe3ee";
  g.fillStyle = "#fbfcfe";
  g.fillRect(0, 0, W, H);
  g.strokeStyle = rule;
  for (let y = 40; y < H; y += 40) {
    g.beginPath();
    g.moveTo(0, y + 0.5);
    g.lineTo(W, y + 0.5);
    g.stroke();
  }
  g.fillStyle = pen;
  g.fillRect(96, 0, 2, H);

  drawWordmark(g, 140, 78, pen, ink);

  g.fillStyle = ink;
  g.font = "800 54px 'Bricolage Grotesque Variable'";
  g.fillText(`I caught ${e.model || "my AI"} being wrong.`, 140, 170, W - 280);

  let y = 250;
  if (e.human) {
    g.fillStyle = pen;
    g.font = "400 52px 'Gochi Hand'";
    for (const line of wrap(g, `“${clip(e.human, 160)}”`, W - 280, 3)) {
      g.fillText(line, 140, y);
      y += 58;
    }
  }
  if (e.ai) {
    g.font = "500 26px 'Instrument Sans'";
    const lines = wrap(g, clip(e.ai, 200), W - 340, 2);
    const boxH = 34 + lines.length * 36;
    g.fillStyle = aiSoft;
    g.beginPath();
    g.roundRect(140, y, W - 280, boxH, 14);
    g.fill();
    g.fillStyle = ai;
    g.font = "600 16px 'JetBrains Mono'";
    g.fillText((e.model || "AI").toUpperCase(), 166, y + 30);
    g.fillStyle = ink;
    g.font = "500 26px 'Instrument Sans'";
    lines.forEach((l, i) => g.fillText(l, 166, y + 66 + i * 36));
  }
  g.textAlign = "center";
  g.fillStyle = muted;
  g.font = "500 18px 'Instrument Sans'";
  g.fillText("Could AI replace you?  github.com/HTMLGuyLLC/NoActually", W / 2, 606);
  return new Promise((resolve, reject) => c.toBlob((b) => (b ? resolve(b) : reject(new Error("Couldn't draw the card."))), "image/png"));
}
