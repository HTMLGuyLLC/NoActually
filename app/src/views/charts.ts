// Small SVG charts for the Improve tab. Every chart gets a text summary for screen readers, and
// series differ by lightness and a gap between segments as well as color. Each bar and dot is
// keyboard-focusable and carries its tooltip text as its label.
import { type Html, html } from "../html";

export type Point = {
  label: string;
  value: number | null;
  /** Too few tasks to trust: drawn hollow, left off the line. */
  weak?: boolean;
  /** What the dot's tooltip says, in place of the label and rounded value. */
  tip?: string;
};
export type Band = { from: number; name: string };

/** Full-width charts; half-width ones pass a smaller `width` so their text stays readable. */
const FULL = 900;
const L = 40;
const R = 12;
const T = 18;
const B = 30;

/** Every `n`th label, so the x axis never crowds. */
const every = (count: number, width: number) => Math.max(1, Math.ceil(count / (width >= FULL ? 10 : 6)));

/** A gridline label: whole numbers as they are, others with one decimal (2.5, not 3). */
const gridNum = (g: number) => String(Math.round(g * 10) / 10);

/** Wraps a chart so the box that scrolls sideways can be reached and scrolled with the keyboard. */
export const chartBox = (label: string, body: Html): Html => html`<div class="chart" tabindex="0" role="region" aria-label="${label}">${body}</div>`;

/** An x-axis label; the last one hangs left so it isn't cut off at the edge. */
const xLabel = (x: number, y: number, text: string, last: boolean) => html`<text x="${x}" y="${y}" text-anchor="${last ? "end" : "middle"}">${text}</text>`;

/** A line over percent-like values (`min` to `max`), with optional tier bands behind it. */
export function lineChart(o: {
  id: string;
  label: string;
  summary: string;
  points: Point[];
  min?: number;
  max?: number;
  width?: number;
  bands?: Band[];
  unit?: string;
  height?: number;
  /** Why a dot is hollow, in its tooltip. */
  weakNote?: string;
}): Html {
  const H = o.height ?? 220;
  const W = o.width ?? FULL;
  const max = o.max ?? 100;
  const min = o.min ?? 0;
  const n = o.points.length;
  if (!n) return html`<p class="muted">No weeks yet.</p>`;
  const x = (i: number) => (n === 1 ? (L + W - R) / 2 : L + ((W - L - R) * i) / (n - 1));
  // Values below `min` sit on the floor rather than off the chart.
  const y = (v: number) => T + (H - T - B) * (1 - (Math.min(max, Math.max(min, v)) - min) / (max - min));
  const unit = o.unit ?? "";
  const bands = (o.bands ?? []).map(
    (b) =>
      html`<line x1="${L}" x2="${W - R}" y1="${y(b.from)}" y2="${y(b.from)}" class="band-line"/><text x="${L + 6}" y="${y(b.from) - 5}" class="band-name">${b.name} ${b.from}${unit}</text>`,
  );
  const grid = [min, (min + max) / 2, max].map(
    (g) =>
      html`<line x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}" class="grid"/><text x="${L - 8}" y="${y(g) + 4}" text-anchor="end">${gridNum(g)}${unit}</text>`,
  );
  // The line skips weak and empty points, so a quiet week never drags it to zero.
  const strong = o.points.map((p, i) => ({ p, i })).filter(({ p }) => p.value != null && !p.weak);
  const path = strong.map(({ p, i }, k) => `${k ? "L" : "M"}${x(i).toFixed(1)},${y(p.value ?? 0).toFixed(1)}`).join(" ");
  const step = every(n, W);
  const weakNote = o.weakNote ?? "too few tasks to count";
  const dots = o.points.map((p, i) => {
    const label = i % step === 0 || i === n - 1 ? xLabel(x(i), H - 10, p.label, i === n - 1 && n > 1) : "";
    if (p.value == null) return label;
    const tip = `${p.tip ?? `${p.label}: ${Math.round(p.value)}${unit}`}${p.weak ? ` (${weakNote})` : ""}`;
    return html`<circle cx="${x(i)}" cy="${y(p.value)}" r="${p.weak ? 4 : 4.5}" class="${p.weak ? "dot-weak" : "dot-strong"}" data-tip="${tip}" tabindex="0" role="img" aria-label="${tip}"></circle>${label}`;
  });
  return html`<svg viewBox="0 0 ${W} ${H}" role="group" aria-label="${o.label}" aria-describedby="${o.id}-summary">${grid}${bands}${path ? html`<path d="${path}" class="trend"/>` : ""}${dots}</svg>
    <p class="sr-only" id="${o.id}-summary">${o.summary}</p>`;
}

/** Stacked bars: one bar per bucket, one segment per series (numbered classes s0, s1… for color and pattern). */
export function stackChart(o: {
  id: string;
  label: string;
  summary: string;
  labels: string[];
  /** `seg` picks a series' color class (s0 red, s1 ink, s2 blue, s3 muted, or ga to gf for report card letter grades); by default, its position. */
  series: { title: string; values: number[]; seg?: string | undefined }[];
  /** One tip per bucket for the whole bar (its breakdown); without, each segment names itself. */
  tips?: string[];
  width?: number;
  height?: number;
}): Html {
  const H = o.height ?? 200;
  const W = o.width ?? FULL;
  const n = o.labels.length;
  if (!n) return html`<p class="muted">No weeks yet.</p>`;
  const totals = o.labels.map((_, i) => o.series.reduce((t, s) => t + (s.values[i] ?? 0), 0));
  const max = Math.max(1, ...totals);
  const bw = (W - L - R) / n;
  const y = (v: number) => T + (H - T - B) * (1 - v / max);
  const step = every(n, W);
  const bars = o.labels.map((label, i) => {
    let acc = 0;
    const segs = o.series.map((s, si) => {
      const v = s.values[i] ?? 0;
      if (!v) return "";
      const top = y(acc + v);
      const tip = `${label}: ${s.title} ${v}`;
      const own = o.tips ? "" : html` data-tip="${tip}" tabindex="0" role="img" aria-label="${tip}"`;
      const seg = html`<rect x="${L + i * bw + bw * 0.18}" y="${top}" width="${bw * 0.64}" height="${y(acc) - top}" class="seg ${s.seg ?? `s${si}`}"${own}></rect>`;
      acc += v;
      return seg;
    });
    const tip = o.tips?.[i];
    const bar = tip ? html`<g class="bar-group" data-tip="${tip}" tabindex="0" role="img" aria-label="${tip}">${segs}</g>` : segs;
    return html`${bar}${i % step === 0 || i === n - 1 ? xLabel(L + i * bw + bw / 2, H - 10, label, false) : ""}`;
  });
  const grid = [0, max].map(
    (g) => html`<line x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}" class="grid"/><text x="${L - 8}" y="${y(g) + 4}" text-anchor="end">${g}</text>`,
  );
  const key = html`<ul class="chart-key">${o.series.map((s, si) => html`<li><i class="seg ${s.seg ?? `s${si}`}" aria-hidden="true"></i>${s.title}</li>`)}</ul>`;
  return html`<svg viewBox="0 0 ${W} ${H}" role="group" aria-label="${o.label}" aria-describedby="${o.id}-summary">${grid}${bars}</svg>${key}
    <p class="sr-only" id="${o.id}-summary">${o.summary}</p>`;
}

/** A tiny trend line for one value, 0 to 100. Weak points are skipped. */
export function sparkline(points: Point[], label: string): Html {
  const w = 120;
  const h = 32;
  const strong = points.map((p, i) => ({ p, i })).filter(({ p }) => p.value != null && !p.weak);
  if (strong.length < 2) return html`<span class="muted small">–</span>`;
  const x = (i: number) => (points.length === 1 ? w / 2 : (w * i) / (points.length - 1));
  const y = (v: number) => 3 + (h - 6) * (1 - v / 100);
  const d = strong.map(({ p, i }, k) => `${k ? "L" : "M"}${x(i).toFixed(1)},${y(p.value ?? 0).toFixed(1)}`).join(" ");
  return html`<svg class="spark" viewBox="0 0 ${w} ${h}" role="img" aria-label="${label}"><path d="${d}"/></svg>`;
}
