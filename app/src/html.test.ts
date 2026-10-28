import { describe, expect, it } from "vitest";
import { EMPTY, esc, Html, html, num, raw } from "./html";
import { HOSTILE } from "./test/fixtures";

describe("esc", () => {
  it("escapes every character that could open a tag or close an attribute", () => {
    expect(esc(HOSTILE)).toBe("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;&amp;&#39;&quot;");
  });

  it("prints nothing for null and undefined, and strings anything else", () => {
    expect(esc(null)).toBe("");
    expect(esc(undefined)).toBe("");
    expect(esc(0)).toBe("0");
    expect(esc(new Error("<b>"))).toBe("Error: &lt;b&gt;");
  });

  it("leaves ordinary text alone", () => {
    expect(esc("Claude Opus 5.5 · “quoted” — ok")).toBe("Claude Opus 5.5 · “quoted” — ok");
  });
});

describe("html", () => {
  it("escapes interpolations in content and attributes", () => {
    const h = html`<p title="${HOSTILE}">${HOSTILE}</p>`;
    expect(h).toBeInstanceOf(Html);
    expect(h.value).not.toContain("<img");
    expect(h.value).not.toContain('"alert');
    expect(h.value.startsWith('<p title="&lt;img')).toBe(true);
  });

  it("nests templates and lists without double escaping", () => {
    const items = ["<a>", "b&c"].map((x) => html`<li>${x}</li>`);
    expect(html`<ul>${items}</ul>`.value).toBe("<ul><li>&lt;a&gt;</li><li>b&amp;c</li></ul>");
  });

  it("inserts raw() fragments as is, and nothing for null/undefined", () => {
    expect(html`${raw("<svg/>")}${null}${undefined}${3}`.value).toBe("<svg/>3");
    expect(String(html`a${EMPTY}b`)).toBe("ab");
  });
});

describe("num", () => {
  it("groups thousands", () => {
    expect(num(1234567)).toBe((1234567).toLocaleString());
  });
});
