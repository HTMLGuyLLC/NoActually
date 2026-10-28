// The share card images, drawn on a canvas on this machine. Always the light theme.
import type { Stats } from "../api";
import { clip, SHARE_URL_SHORT } from "./share";

/** The light theme's colors (style.css :root), which the cards always use whatever the app's theme. */
export const CARD_THEME = {
  paper: "#fbfcfe",
  ink: "#141a2e",
  muted: "#5d6680",
  pen: "#e0322b",
  onPen: "#ffffff",
  ai: "#3b5bdb",
  aiSoft: "#e7ecfd",
} as const;

const W = 1200;
const H = 630; // the size social sites use for link previews

/** A 2× canvas (crisp on high-density screens) filled with paper, once the fonts it uses are loaded. */
async function canvas(fonts: string[]): Promise<{ c: HTMLCanvasElement; g: CanvasRenderingContext2D }> {
  const c = document.createElement("canvas");
  c.width = W * 2;
  c.height = H * 2;
  const g = c.getContext("2d");
  if (!g) throw new Error("Couldn't draw the card.");
  g.scale(2, 2);
  await Promise.all(fonts.map((f) => document.fonts.load(f)));
  g.fillStyle = CARD_THEME.paper;
  g.fillRect(0, 0, W, H);
  return { c, g };
}

const png = (c: HTMLCanvasElement) =>
  new Promise<Blob>((resolve, reject) => c.toBlob((b) => (b ? resolve(b) : reject(new Error("Couldn't draw the card."))), "image/png"));

/** The logo: the icon's mark, then "No, Actually..." in ink, with the text baseline at `y`. */
function drawWordmark(g: CanvasRenderingContext2D, x: number, y: number) {
  const size = 32;
  const k = size / 27; // the mark's own box inside the icon's 44-unit grid (x 9–36, y 10.5–37.5)
  g.save();
  g.translate(x, y - size + 6);
  g.scale(k, k);
  g.translate(-9, -10.5);
  g.lineCap = "round";
  g.lineJoin = "round";
  // Same mark as the app icon: check in ink, underline in red.
  g.strokeStyle = CARD_THEME.pen;
  g.lineWidth = 2.6;
  g.beginPath();
  g.moveTo(10.7, 35.2);
  g.quadraticCurveTo(22, 32.7, 34, 35);
  g.stroke();
  g.translate(22, 22);
  g.rotate((-4 * Math.PI) / 180);
  g.translate(-22, -22);
  g.strokeStyle = CARD_THEME.ink;
  g.lineWidth = 4.4;
  g.beginPath();
  g.moveTo(12.9, 24.1);
  g.lineTo(18.9, 30.1);
  g.lineTo(31.8, 13.8);
  g.stroke();
  g.restore();
  g.fillStyle = CARD_THEME.ink;
  g.font = "800 28px 'Bricolage Grotesque Variable'";
  g.fillText("No, Actually...", x + size + 8, y);
}

type ScoreCard = Pick<Stats, "verdict" | "catches_last_7d" | "pct_real_chats_with_catch" | "catches" | "humbled">;

/** The score card. `tier` is the tier's display name. */
export async function drawShareCard(s: ScoreCard, who: string, tier: string): Promise<Blob> {
  const { c, g } = await canvas([
    "800 64px 'Bricolage Grotesque Variable'",
    "400 200px 'Gochi Hand'",
    "500 24px 'Instrument Sans'",
    "600 18px 'JetBrains Mono'",
  ]);
  const { ink, muted, pen, ai } = CARD_THEME;
  const no = s.verdict === "no";

  g.textBaseline = "alphabetic";
  drawWordmark(g, 140, 78);

  g.fillStyle = ink;
  g.font = "800 64px 'Bricolage Grotesque Variable'";
  const q = `Could ${who} replace me?`;
  let size = 64;
  while (g.measureText(q).width > W - 240 && size > 36) {
    size -= 4;
    g.font = `800 ${size}px 'Bricolage Grotesque Variable'`;
  }
  g.fillText(q, 140, 190);

  g.save();
  g.translate(140, 400);
  g.rotate((-6 * Math.PI) / 180);
  g.fillStyle = no ? pen : muted;
  g.font = "400 200px 'Gochi Hand'";
  g.fillText(no ? "No." : "Hmm.", 0, 0);
  g.restore();

  g.fillStyle = ink;
  g.font = "500 30px 'Instrument Sans'";
  g.fillText(`I caught it ${s.catches_last_7d} ${s.catches_last_7d === 1 ? "time" : "times"} this week.`, 520, 310);
  if (s.pct_real_chats_with_catch != null) {
    g.fillStyle = muted;
    g.fillText(`It slipped up in ${Math.round(s.pct_real_chats_with_catch)}% of my chats.`, 520, 354);
  }

  // Tier pill
  g.font = "600 18px 'JetBrains Mono'";
  const label = tier.toUpperCase();
  const tw = g.measureText(label).width + 36;
  g.fillStyle = pen;
  g.beginPath();
  g.roundRect(520, 384, tw, 40, 20);
  g.fill();
  g.fillStyle = CARD_THEME.onPen;
  g.fillText(label, 538, 410);

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
  g.fillText(`What's your score?  ${SHARE_URL_SHORT}`, W / 2, 612);

  return png(c);
}

type CatchLike = { model: string; human: string | null; ai: string | null };

/** Wraps text to lines no wider than `max` px in the current font, ending in "…" if it was cut. */
export function wrap(g: Pick<CanvasRenderingContext2D, "measureText">, text: string, max: number, maxLines: number): string[] {
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
  const last = lines[maxLines - 1];
  if (last !== undefined && words.join(" ") !== lines.join(" ")) lines[maxLines - 1] = last.replace(/\s*\S*$/, "…");
  return lines;
}

/** A card for one catch: your words in red pen, the AI's admission underneath. */
export async function drawCatchCard(e: CatchLike): Promise<Blob> {
  const { c, g } = await canvas([
    "800 54px 'Bricolage Grotesque Variable'",
    "400 52px 'Gochi Hand'",
    "500 26px 'Instrument Sans'",
    "600 16px 'JetBrains Mono'",
  ]);
  const { ink, muted, pen, ai, aiSoft } = CARD_THEME;

  drawWordmark(g, 140, 78);

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
    lines.forEach((l, i) => {
      g.fillText(l, 166, y + 66 + i * 36);
    });
  }
  g.textAlign = "center";
  g.fillStyle = muted;
  g.font = "500 18px 'Instrument Sans'";
  g.fillText(`Could AI replace you?  ${SHARE_URL_SHORT}`, W / 2, 606);
  return png(c);
}
