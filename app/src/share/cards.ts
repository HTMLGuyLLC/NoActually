// The share card images, drawn on a canvas on this machine. Always the light theme.
import type { Overall, Prompts, PromptTier, Stats } from "../api";
import { plural } from "../format";
import { landedPct, percent } from "../meta";
import { PERIOD_WHEN } from "../state";
import { type CorrectionLike, clip, SHARE_URL_SHORT } from "./share";

/** The light theme's colors (style.css :root), which the cards always use whatever the app's theme. */
export const CARD_THEME = {
  paper: "#fbfcfe",
  ink: "#141a2e",
  muted: "#5d6680",
  pen: "#e0322b",
  onPen: "#ffffff",
  ai: "#3b5bdb",
  aiSoft: "#e7ecfd",
  /** The empty part of a bar. */
  track: "#dfe3ee",
} as const;

/**
 * `wide` is the link-preview size most networks show (1.91:1). `portrait` is Instagram's tallest
 * feed post (4:5), which a wide card would leave as a thin strip.
 */
export type CardShape = "wide" | "portrait";
export const CARD_SIZES: Record<CardShape, { w: number; h: number }> = {
  wide: { w: 1200, h: 630 },
  portrait: { w: 1080, h: 1350 },
};

/** A 2× canvas (crisp on high-density screens) filled with paper, once the fonts it uses are loaded. */
async function canvas(shape: CardShape, fonts: string[]): Promise<{ c: HTMLCanvasElement; g: CanvasRenderingContext2D }> {
  const { w, h } = CARD_SIZES[shape];
  const c = document.createElement("canvas");
  c.width = w * 2;
  c.height = h * 2;
  const g = c.getContext("2d");
  if (!g) throw new Error("Couldn't draw the card.");
  g.scale(2, 2);
  await Promise.all(fonts.map((f) => document.fonts.load(f)));
  g.fillStyle = CARD_THEME.paper;
  g.fillRect(0, 0, w, h);
  return { c, g };
}

const png = (c: HTMLCanvasElement) =>
  new Promise<Blob>((resolve, reject) => c.toBlob((b) => (b ? resolve(b) : reject(new Error("Couldn't draw the card."))), "image/png"));

/** A rounded-rectangle path (CanvasRenderingContext2D.roundRect needs Safari 16). */
function roundedRect(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const k = Math.min(r, w / 2, h / 2);
  g.moveTo(x + k, y);
  g.arcTo(x + w, y, x + w, y + h, k);
  g.arcTo(x + w, y + h, x, y + h, k);
  g.arcTo(x, y + h, x, y, k);
  g.arcTo(x, y, x + w, y, k);
  g.closePath();
}

/** The logo: the icon's mark, then "No, Actually..." in ink, with the text baseline at `y`. */
function drawWordmark(g: CanvasRenderingContext2D, x: number, y: number, scale = 1) {
  const size = 32 * scale;
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
  g.font = `800 ${28 * scale}px 'Bricolage Grotesque Variable'`;
  g.fillText("No, Actually...", x + size + 8 * scale, y);
}

/** The largest size (down to `min`) at which `text` fits in `max` px, with `font(size)` applied. */
function fitFont(g: CanvasRenderingContext2D, text: string, font: (size: number) => string, size: number, max: number, min: number) {
  g.font = font(size);
  while (g.measureText(text).width > max && size > min) {
    size -= 4;
    g.font = font(size);
  }
  return size;
}

/** The tier label in a red pill, its top-left corner at (x, y). */
function drawPill(g: CanvasRenderingContext2D, label: string, x: number, y: number, scale = 1) {
  g.font = `600 ${18 * scale}px 'JetBrains Mono'`;
  const text = label.toUpperCase();
  const h = 40 * scale;
  g.fillStyle = CARD_THEME.pen;
  g.beginPath();
  roundedRect(g, x, y, g.measureText(text).width + 36 * scale, h, h / 2);
  g.fill();
  g.fillStyle = CARD_THEME.onPen;
  g.fillText(text, x + 18 * scale, y + 26 * scale);
}

/** "I CORRECTED AI 177  vs  3 AI CORRECTED ME" between x and x + w, with the rule on top at `y`. */
function drawScoreLine(g: CanvasRenderingContext2D, s: ScoreCard, who: string, x: number, y: number, w: number, scale = 1) {
  const { ink, muted, pen, ai } = CARD_THEME;
  g.fillStyle = ink;
  g.fillRect(x, y, w, 3 * scale);
  g.font = `600 ${16 * scale}px 'JetBrains Mono'`;
  g.fillStyle = muted;
  g.fillText(`I CORRECTED ${who}`.toUpperCase(), x, y + 40 * scale, w / 2 - 8 * scale);
  g.textAlign = "right";
  g.fillText(`${who} CORRECTED ME`.toUpperCase(), x + w, y + 40 * scale, w / 2 - 8 * scale);
  g.font = `800 ${64 * scale}px 'Bricolage Grotesque Variable'`;
  g.fillStyle = ai;
  g.fillText(String(s.humbled), x + w, y + 110 * scale);
  g.textAlign = "left";
  g.fillStyle = pen;
  g.fillText(String(s.corrections), x, y + 110 * scale);
  g.textAlign = "center";
  g.fillStyle = muted;
  g.font = `400 ${34 * scale}px 'Gochi Hand'`;
  g.fillText("vs", x + w / 2, y + 96 * scale);
  g.textAlign = "left";
}

/** The footer line, centered at `y`. */
function drawFooter(g: CanvasRenderingContext2D, text: string, w: number, y: number, scale = 1) {
  g.textAlign = "center";
  g.fillStyle = CARD_THEME.muted;
  g.font = `500 ${18 * scale}px 'Instrument Sans'`;
  g.fillText(text, w / 2, y);
  g.textAlign = "left";
}

type ScoreCard = Pick<Stats, "overall" | "corrections_last_7d" | "corrections" | "humbled">;

const SCORE_FONTS = ["800 64px 'Bricolage Grotesque Variable'", "400 200px 'Gochi Hand'", "500 24px 'Instrument Sans'", "600 18px 'JetBrains Mono'"];
const heading = (size: number) => `800 ${size}px 'Bricolage Grotesque Variable'`;
const hand = (size: number) => `400 ${size}px 'Gochi Hand'`;
/** "Score 86. 83% of my prompts were clear the first time." or, before a score, this week's corrections. */
const timesLine = (s: ScoreCard) =>
  s.overall.score == null
    ? `I corrected it ${plural(s.corrections_last_7d, "time")} this week.`
    : `Score ${s.overall.score}. ${s.overall.direction}% of my prompts were clear the first time.`;
const pctLine = (s: ScoreCard, when: string) =>
  s.overall.score == null ? null : `I corrected it ${plural(s.overall.corrections, "time")} to its ${s.overall.humbled} ${when}.`;

/** Hand-written `text`, rotated like a red-pen mark, baseline at (x, y). */
function drawMark(g: CanvasRenderingContext2D, text: string, color: string, x: number, y: number, size: number) {
  g.save();
  g.translate(x, y);
  g.rotate((-6 * Math.PI) / 180);
  g.fillStyle = color;
  g.font = hand(size);
  g.fillText(text, 0, 0);
  g.restore();
}

/** The big hand-written verdict. */
function drawVerdict(g: CanvasRenderingContext2D, s: ScoreCard, x: number, y: number, size: number) {
  const no = s.overall.verdict === "no";
  drawMark(g, no ? "No." : "Hmm.", no ? CARD_THEME.pen : CARD_THEME.muted, x, y, size);
}

/** The score card. `tier` is the tier's display name. */
export async function drawShareCard(s: ScoreCard, who: string, tier: string, shape: CardShape = "wide", when = PERIOD_WHEN["28d"]): Promise<Blob> {
  const { c, g } = await canvas(shape, SCORE_FONTS);
  const { w } = CARD_SIZES[shape];
  const { ink, muted } = CARD_THEME;
  const q = `Could ${who} replace me?`;
  const pct = pctLine(s, when);
  g.textBaseline = "alphabetic";

  if (shape === "portrait") {
    const x = 96;
    const inner = w - 2 * x;
    drawWordmark(g, x, 150, 1.4);
    g.fillStyle = ink;
    // Up to two lines; a long model name shrinks the type until the whole question fits.
    let size = 84;
    let lines: string[];
    for (;;) {
      g.font = heading(size);
      lines = wrap(g, q, inner, 2);
      if (lines.join(" ") === q || size <= 48) break;
      size -= 4;
    }
    const top = lines.length > 1 ? 260 : 300;
    for (const [i, l] of lines.entries()) g.fillText(l, x, top + i * size * 1.12);
    drawVerdict(g, s, x - 8, 720, 360);
    g.fillStyle = ink;
    g.font = "500 42px 'Instrument Sans'";
    g.fillText(timesLine(s), x, 840);
    if (pct) {
      g.fillStyle = muted;
      g.fillText(pct, x, 900);
    }
    drawPill(g, tier, x, 950, 1.5);
    drawScoreLine(g, s, who, x, 1080, inner, 1.5);
    drawFooter(g, `What's your score?  ${SHARE_URL_SHORT}`, w, 1290, 1.4);
    return png(c);
  }

  drawWordmark(g, 140, 78);
  g.fillStyle = ink;
  fitFont(g, q, heading, 64, w - 240, 36);
  g.fillText(q, 140, 190);
  drawVerdict(g, s, 140, 400, 200);
  g.fillStyle = ink;
  g.font = "500 30px 'Instrument Sans'";
  g.fillText(timesLine(s), 520, 310);
  if (pct) {
    g.fillStyle = muted;
    g.fillText(pct, 520, 354);
  }
  drawPill(g, tier, 520, 384);
  drawScoreLine(g, s, who, 140, 470, w - 280);
  drawFooter(g, `What's your score?  ${SHARE_URL_SHORT}`, w, 612);
  return png(c);
}

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

const CORRECTION_FONTS = ["800 54px 'Bricolage Grotesque Variable'", "400 52px 'Gochi Hand'", "500 26px 'Instrument Sans'", "600 16px 'JetBrains Mono'"];

/**
 * Where the correction card's parts go. `k` scales the type; the portrait card fits more lines and
 * centers the quote in the space under the title (`center`) instead of starting at `quoteY`.
 */
const CORRECTION_LAYOUT = {
  wide: { x: 140, k: 1, markY: 78, titleY: 170, titleLines: 1, quoteY: 250, center: false, quoteLines: 3, aiLines: 2, footerY: 606 },
  portrait: { x: 96, k: 1.4, markY: 150, titleY: 300, titleLines: 2, quoteY: 0, center: true, quoteLines: 5, aiLines: 5, footerY: 1290 },
} as const;

/** A card for one correction: your words in red pen, the AI's admission underneath. */
export async function drawCorrectionCard(e: CorrectionLike, shape: CardShape = "wide"): Promise<Blob> {
  const { c, g } = await canvas(shape, CORRECTION_FONTS);
  const { w } = CARD_SIZES[shape];
  const { x, k, ...L } = CORRECTION_LAYOUT[shape];
  const { ink, pen, ai, aiSoft } = CARD_THEME;
  const inner = w - 2 * x;

  drawWordmark(g, x, L.markY, k);

  g.fillStyle = ink;
  g.font = heading(54 * k);
  const title = `I corrected ${e.model || "my AI"}.`;
  // One line squeezes a long model name to fit (fillText's max width); more lines wrap it.
  const titleLines = L.titleLines > 1 ? wrap(g, title, inner, L.titleLines) : [title];
  let y: number = L.titleY;
  for (const line of titleLines) {
    g.fillText(line, x, y, inner);
    y += 60 * k;
  }

  const quoteFont = `400 ${52 * k}px 'Gochi Hand'`;
  const aiFont = `500 ${26 * k}px 'Instrument Sans'`;
  const pad = 26 * k;
  const lead = 36 * k;
  g.font = quoteFont;
  const quote = e.human ? wrap(g, `“${clip(e.human, 160)}”`, inner, L.quoteLines) : [];
  g.font = aiFont;
  const reply = e.ai ? wrap(g, clip(e.ai, 200), inner - 2 * pad - 8 * k, L.aiLines) : [];
  const boxH = 34 * k + reply.length * lead;

  y = Math.max(y + 20 * k, L.quoteY);
  if (L.center) {
    // Glyphs rise ~42k above the first baseline; center from there to the box's bottom.
    const blockH = 42 * k + quote.length * 58 * k + (reply.length ? boxH : 0);
    const room = L.footerY - 60 * k - (y - 42 * k);
    y += Math.max(0, (room - blockH) / 2);
  }
  g.fillStyle = pen;
  g.font = quoteFont;
  for (const line of quote) {
    g.fillText(line, x, y);
    y += 58 * k;
  }
  if (reply.length) {
    g.fillStyle = aiSoft;
    g.beginPath();
    roundedRect(g, x, y, inner, boxH, 14 * k);
    g.fill();
    g.fillStyle = ai;
    g.font = `600 ${16 * k}px 'JetBrains Mono'`;
    g.fillText((e.model || "AI").toUpperCase(), x + pad, y + 30 * k);
    g.fillStyle = ink;
    g.font = aiFont;
    reply.forEach((l, i) => {
      g.fillText(l, x + pad, y + 66 * k + i * lead);
    });
  }
  drawFooter(g, `Could AI replace you?  ${SHARE_URL_SHORT}`, w, L.footerY, k);
  return png(c);
}

type PromptsCard = Pick<Prompts, "window_landed" | "window_tasks" | "grades" | "tasks">;

const PROMPT_QUESTION = "Do I make my AI guess?";
export const promptLines = (b: PromptsCard, when = PERIOD_WHEN["28d"]): [string, string] => [
  `${landedPct(b)}% of my prompts ${when} were clear the first time.`,
  `${percent(b.grades.sharp ?? 0, b.tasks)}% of all my prompts were sharp.`,
];

/** "SHARP 20% · SOLID 24% · THIN 56%": the bar's key. */
export const gradeKey = (b: PromptsCard) => ["sharp", "solid", "thin"].map((id) => `${id.toUpperCase()} ${percent(b.grades[id] ?? 0, b.tasks)}%`).join("  ·  ");

/** Sharp / solid / thin as one bar, sharp first, `w` wide with its top-left at (x, y). */
function drawGradeBar(g: CanvasRenderingContext2D, b: PromptsCard, x: number, y: number, w: number, h: number) {
  const parts: [string, string][] = [
    ["sharp", CARD_THEME.pen],
    ["solid", CARD_THEME.ink],
    ["thin", CARD_THEME.track],
  ];
  g.save();
  g.beginPath();
  roundedRect(g, x, y, w, h, h / 3);
  g.clip();
  g.fillStyle = CARD_THEME.track;
  g.fillRect(x, y, w, h);
  let at = x;
  for (const [id, color] of parts) {
    const part = (w * (b.grades[id] ?? 0)) / Math.max(1, b.tasks);
    g.fillStyle = color;
    g.fillRect(at, y, part, h);
    at += part;
  }
  g.restore();
}

type CorrectionsCard = Pick<Overall, "corrections" | "humbled" | "judgment">;

const CORRECTION_QUESTION = "Do I correct my AI?";
export const correctionLines = (o: CorrectionsCard, when = PERIOD_WHEN["28d"]): [string, string] => [
  `I corrected it ${plural(o.corrections, "time")} ${when}. It corrected me ${o.humbled}.`,
  `Judgment ${o.judgment ?? 0} of 100: I check its work, and it rarely has to correct me.`,
];

/** The corrections card: "Do I correct my AI?", the tier's answer, and who corrected whom. */
export async function drawCorrectionsCard(o: CorrectionsCard, tier: PromptTier, shape: CardShape = "wide", when = PERIOD_WHEN["28d"]): Promise<Blob> {
  const { c, g } = await canvas(shape, SCORE_FONTS);
  const { w } = CARD_SIZES[shape];
  const { ink, muted, pen } = CARD_THEME;
  const [corrected, judged] = correctionLines(o, when);
  g.textBaseline = "alphabetic";

  if (shape === "portrait") {
    const x = 96;
    const inner = w - 2 * x;
    drawWordmark(g, x, 150, 1.4);
    g.fillStyle = ink;
    fitFont(g, CORRECTION_QUESTION, heading, 84, inner, 48);
    g.fillText(CORRECTION_QUESTION, x, 300);
    drawMark(g, tier.verdict, pen, x - 8, 620, fitFont(g, tier.verdict, hand, 240, inner - 40, 120));
    g.fillStyle = ink;
    g.font = "500 40px 'Instrument Sans'";
    for (const [i, l] of wrap(g, corrected, inner, 2).entries()) g.fillText(l, x, 760 + i * 52);
    g.fillStyle = muted;
    for (const [i, l] of wrap(g, judged, inner, 2).entries()) g.fillText(l, x, 890 + i * 52);
    drawPill(g, tier.name, x, 1010, 1.5);
    drawFooter(g, `Do you correct yours?  ${SHARE_URL_SHORT}`, w, 1290, 1.4);
    return png(c);
  }

  drawWordmark(g, 140, 78);
  g.fillStyle = ink;
  fitFont(g, CORRECTION_QUESTION, heading, 64, w - 280, 36);
  g.fillText(CORRECTION_QUESTION, 140, 190);
  drawMark(g, tier.verdict, pen, 140, 350, fitFont(g, tier.verdict, hand, 170, w - 320, 90));
  g.fillStyle = ink;
  g.font = "500 30px 'Instrument Sans'";
  g.fillText(corrected, 140, 440, w - 280);
  g.fillStyle = muted;
  g.fillText(judged, 140, 484, w - 280);
  drawPill(g, tier.name, 140, 512);
  drawFooter(g, `Do you correct yours?  ${SHARE_URL_SHORT}`, w, 612);
  return png(c);
}

/** The prompts card: "Do I make my AI guess?", the tier's answer, and how many were clear the first time. */
export async function drawPromptCard(b: PromptsCard, tier: PromptTier, shape: CardShape = "wide", when = PERIOD_WHEN["28d"]): Promise<Blob> {
  const { c, g } = await canvas(shape, SCORE_FONTS);
  const { w } = CARD_SIZES[shape];
  const { ink, muted, pen } = CARD_THEME;
  const [landed, sharp] = promptLines(b, when);
  g.textBaseline = "alphabetic";

  if (shape === "portrait") {
    const x = 96;
    const inner = w - 2 * x;
    drawWordmark(g, x, 150, 1.4);
    g.fillStyle = ink;
    fitFont(g, PROMPT_QUESTION, heading, 84, inner, 48);
    g.fillText(PROMPT_QUESTION, x, 300);
    drawMark(g, tier.verdict, pen, x - 8, 620, fitFont(g, tier.verdict, hand, 240, inner - 40, 120));
    g.fillStyle = ink;
    g.font = "500 40px 'Instrument Sans'";
    for (const [i, l] of wrap(g, landed, inner, 2).entries()) g.fillText(l, x, 760 + i * 52);
    g.fillStyle = muted;
    g.fillText(sharp, x, 890);
    drawPill(g, tier.name, x, 940, 1.5);
    drawGradeBar(g, b, x, 1060, inner, 26);
    g.font = "600 24px 'JetBrains Mono'";
    g.fillStyle = muted;
    g.fillText(gradeKey(b), x, 1136);
    drawFooter(g, `Do you make yours guess?  ${SHARE_URL_SHORT}`, w, 1290, 1.4);
    return png(c);
  }

  drawWordmark(g, 140, 78);
  g.fillStyle = ink;
  fitFont(g, PROMPT_QUESTION, heading, 64, w - 280, 36);
  g.fillText(PROMPT_QUESTION, 140, 190);
  drawMark(g, tier.verdict, pen, 140, 350, fitFont(g, tier.verdict, hand, 170, w - 320, 90));
  g.fillStyle = ink;
  g.font = "500 30px 'Instrument Sans'";
  g.fillText(landed, 140, 440, w - 280);
  g.fillStyle = muted;
  g.fillText(sharp, 140, 484, w - 280);
  drawPill(g, tier.name, 140, 512);
  drawFooter(g, `Do you make yours guess?  ${SHARE_URL_SHORT}`, w, 612);
  return png(c);
}
