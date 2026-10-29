import { describe, expect, it } from "vitest";
import { HOSTILE } from "../test/fixtures";
import { lineChart, type Point, sparkline, stackChart } from "./charts";

const safe = (markup: string) => {
  expect(markup).not.toContain("<img");
  expect(markup).not.toContain('onerror="');
};
const count = (m: string, needle: string) => m.split(needle).length - 1;

const line = (points: Point[], over: Partial<Parameters<typeof lineChart>[0]> = {}) =>
  lineChart({ id: "landed", label: "Percent landed", summary: "Went up.", points, ...over }).value;

describe("lineChart", () => {
  const points: Point[] = [
    { label: "Oct 5", value: 50 },
    { label: "Oct 12", value: 20, weak: true },
    { label: "Oct 19", value: 80 },
    { label: "Oct 26", value: null },
  ];

  it("leaves weak points off the line but still draws them as hollow dots", () => {
    const m = line(points);
    // Two strong points: one move, one line-to. The weak 20 and the empty week aren't on the path.
    const d = m.match(/<path d="([^"]+)" class="trend"/)?.[1] ?? "";
    expect(d.match(/M/g)).toHaveLength(1);
    expect(d.match(/L/g)).toHaveLength(1);
    expect(count(m, 'class="dot-strong"')).toBe(2);
    expect(count(m, 'class="dot-weak"')).toBe(1);
    expect(m).toContain("Oct 12: 20 (too few tasks to count)");
  });

  it("draws no line, but still the hollow dots, when every point is weak", () => {
    const m = line([
      { label: "a", value: 10, weak: true },
      { label: "b", value: 30, weak: true },
    ]);
    expect(m).not.toContain('class="trend"');
    expect(count(m, 'class="dot-weak"')).toBe(2);
  });

  it("draws a single strong point without a line segment", () => {
    const m = line([{ label: "a", value: 40 }]);
    expect(m).toContain('<path d="M');
    expect(m).not.toMatch(/<path d="[^"]*L/);
    expect(count(m, "<circle")).toBe(1);
  });

  it("says there are no weeks when there are no points, and draws nothing", () => {
    const m = line([]);
    expect(m).toBe('<p class="muted">No weeks yet.</p>');
  });

  it("labels the picture and links its summary for screen readers", () => {
    const m = line(points);
    expect(m).toContain('role="img" aria-label="Percent landed" aria-describedby="landed-summary"');
    expect(m).toContain('<p class="sr-only" id="landed-summary">Went up.</p>');
  });

  it("draws a tier band with its name and floor", () => {
    const m = line(points, { bands: [{ from: 80, name: "Tech lead" }], unit: "%" });
    expect(m).toContain('class="band-line"');
    expect(m).toContain("Tech lead 80%");
    expect(m).toContain(">100%</text>");
  });

  it("keeps a value past the axis on the chart", () => {
    const m = line([{ label: "a", value: 250 }]);
    const cy = Number(m.match(/cy="([\d.]+)"/)?.[1]);
    expect(cy).toBe(18); // the top of the plot area, not above it
    const low = Number(line([{ label: "a", value: -5 }]).match(/cy="([\d.]+)"/)?.[1]);
    expect(low).toBe(190); // the baseline
  });

  it("thins the x labels so a long series stays readable", () => {
    const many = Array.from({ length: 40 }, (_, i) => ({ label: `w${i}`, value: 10 }));
    const m = line(many);
    expect(count(m, "<text") - 3 /* y grid */).toBeLessThan(15);
    expect(m).toContain(">w39<"); // the last one is always shown
  });

  it("escapes labels, bands, the summary and the unit", () => {
    safe(line([{ label: HOSTILE, value: 1 }], { bands: [{ from: 50, name: HOSTILE }], label: HOSTILE, summary: HOSTILE, id: "x", unit: "%" }));
  });
});

describe("stackChart", () => {
  const chart = (over: Partial<Parameters<typeof stackChart>[0]> = {}) =>
    stackChart({
      id: "steer",
      label: "Steering by cause",
      summary: "Mostly added.",
      labels: ["Oct", "Nov"],
      series: [
        { title: "Added", values: [3, 0] },
        { title: "Redo", values: [1, 2] },
      ],
      ...over,
    }).value;

  it("draws one segment per non-zero value, with a numbered class per series", () => {
    const m = chart();
    expect(count(m, "<rect")).toBe(3); // the zero is skipped
    expect(m).toContain('class="seg s0"');
    expect(m).toContain('class="seg s1"');
    expect(m).toContain("Oct: Added 3");
    expect(m).not.toContain("Nov: Added");
  });

  it("keys the series by name, and labels the picture with a summary id", () => {
    const m = chart();
    expect(m).toContain('<li><i class="seg s0" aria-hidden="true"></i>Added</li>');
    expect(m).toContain('role="img" aria-label="Steering by cause" aria-describedby="steer-summary"');
    expect(m).toContain('<p class="sr-only" id="steer-summary">Mostly added.</p>');
  });

  it("says there are no weeks when there is nothing to stack", () => {
    expect(chart({ labels: [], series: [] })).toBe('<p class="muted">No weeks yet.</p>');
  });

  it("draws an all-zero chart without dividing by zero", () => {
    const m = chart({ series: [{ title: "Added", values: [0, 0] }] });
    expect(m).not.toContain("<rect");
    expect(m).not.toContain("NaN");
  });

  it("escapes series titles and labels", () => {
    safe(chart({ labels: [HOSTILE, "b"], series: [{ title: HOSTILE, values: [1, 1] }], label: HOSTILE, summary: HOSTILE }));
  });
});

describe("sparkline", () => {
  it("draws a line through the strong points", () => {
    const m = sparkline(
      [
        { label: "a", value: 20 },
        { label: "b", value: 90, weak: true },
        { label: "c", value: 60 },
      ],
      "Purpose per week",
    ).value;
    expect(m).toContain('role="img" aria-label="Purpose per week"');
    expect(count(m, "L")).toBe(1); // the weak middle point is skipped
  });

  it("shows a dash when fewer than two points can be trusted", () => {
    expect(sparkline([{ label: "a", value: 20 }], "x").value).toContain("–");
    expect(
      sparkline(
        [
          { label: "a", value: 20, weak: true },
          { label: "b", value: null },
        ],
        "x",
      ).value,
    ).not.toContain("<svg");
    expect(sparkline([], "x").value).not.toContain("<svg");
  });

  it("escapes its label", () => {
    safe(
      sparkline(
        [
          { label: "a", value: 1 },
          { label: "b", value: 2 },
        ],
        HOSTILE,
      ).value,
    );
  });
});
