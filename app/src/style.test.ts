import { describe, expect, it } from "vitest";

const css = Object.values(import.meta.glob<string>("./style.css", { query: "?raw", import: "default", eager: true })).join("");
const light = css.slice(0, css.indexOf("@media (prefers-color-scheme: dark)"));

const hex = (name: string) => {
  const h = light.match(new RegExp(`--${name}:\\s*#([0-9a-f]{6})`))?.[1];
  if (!h) throw new Error(`no --${name}`);
  return [0, 2, 4].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
};
const lum = ([r, g, b]: number[]) => {
  const lin = (v = 0) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};
const contrast = (a: number[], b: number[]) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
/** The declarations of the first rule whose selector is exactly `selector`. */
const rule = (selector: string) => css.match(new RegExp(`^${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} \\{([^}]*)\\}`, "m"))?.[1] ?? "";

describe("page spacing", () => {
  it("passes the page's gap through main to every section", () => {
    expect(rule(".wrap")).toContain("gap: 36px");
    expect(rule("main")).toContain("display: grid");
    expect(rule("main")).toContain("gap: inherit");
  });
});

describe("light theme text", () => {
  // Red text sits on these; plain --pen is under 4.5:1 on the pale ones.
  const pairs: [string, string, string][] = [
    [".ladder li .at", "pen-text", "pen-soft"],
    [".hand-note", "pen-text", "paper"],
    [".share-btn .needs-paste", "pen-text", "pen-soft"],
    ['.pick-list li[aria-selected="true"] .pick-name b', "pen-text", "ai-soft"],
  ];
  for (const [selector, fg, bg] of pairs) {
    it(`${selector} uses --${fg} and passes 4.5:1 on --${bg}`, () => {
      expect(rule(selector)).toContain(`color: var(--${fg})`);
      expect(contrast(hex(fg), hex(bg))).toBeGreaterThanOrEqual(4.5);
    });
  }
});

describe("controls", () => {
  it("makes the check-now button at least 24 by 24", () => {
    const r = rule(".sync-now");
    expect(Number(r.match(/width: (\d+)px/)?.[1])).toBeGreaterThanOrEqual(24);
    expect(Number(r.match(/height: (\d+)px/)?.[1])).toBeGreaterThanOrEqual(24);
  });

  it("styles the two quote boxes like the post box", () => {
    expect(css).toContain("#share-text, #share-human, #share-ai {");
  });

  it("draws a dialog itself where showModal is missing", () => {
    expect(css).toContain("dialog.modal-fallback[open]");
  });

  it("falls back to a plain color where color-mix is missing", () => {
    expect(rule(".correction.pinned")).toContain("box-shadow");
    expect(rule(".share-guide.paste")).toContain("background: var(--hl)");
  });
});
