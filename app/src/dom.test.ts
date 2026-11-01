// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import { captureFocus, data, focusKey, must, restoreFocus, say } from "./dom";

const page = (body: string) => `<h1>Title</h1>${body}`;

describe("must / data / say", () => {
  beforeEach(() => {
    document.body.innerHTML = `<div id="app"><button data-id="7">x</button></div>`;
  });

  it("returns the element, or throws a clear error", () => {
    expect(must("#app").id).toBe("app");
    expect(() => must("#nope")).toThrow("Missing element: #nope");
  });

  it("reads a required data attribute", () => {
    const b = must("button");
    expect(data(b, "id")).toBe("7");
    expect(() => data(b, "pinned")).toThrow("Missing data-pinned on <button>");
  });

  it("sets text, ignoring a missing element", () => {
    say(must("button"), "<b>hi</b>");
    expect(must("button").innerHTML).toBe("&lt;b&gt;hi&lt;/b&gt;");
    say(null, "nothing happens");
  });
});

describe("focus across a redraw", () => {
  let root: HTMLElement;
  beforeEach(() => {
    document.body.innerHTML = `<div id="app"></div><button id="outside">o</button>`;
    root = must("#app");
  });
  const redraw = (markup: string) => {
    const memo = captureFocus(root);
    root.innerHTML = markup;
    restoreFocus(root, memo);
  };

  it("keys a control by id, or by action and id", () => {
    root.innerHTML = `<button id="a">a</button><button data-action="pin" data-id="3">p</button><button data-action="retry">r</button><span>s</span>`;
    const [a, pin, retry] = root.querySelectorAll("button");
    expect(focusKey(a ?? null)).toBe("#a");
    expect(focusKey(pin ?? null)).toBe('[data-action="pin"][data-id="3"]');
    expect(focusKey(retry ?? null)).toBe('[data-action="retry"]');
    expect(focusKey(root.querySelector("span"))).toBeNull();
  });

  it("keys the rest by field name, chart tip or label, and escapes what it quotes", () => {
    root.innerHTML = `<input name="level" type="radio" value="full"><input name="full-days"><button aria-label="Close &quot;x&quot;">x</button><svg><circle data-tip="Week of Oct 5&#10;3 corrections" tabindex="0"></circle></svg>`;
    const [radio, days, close] = root.querySelectorAll("input, button");
    expect(focusKey(radio ?? null)).toBe('input[name="level"][value="full"]');
    expect(focusKey(days ?? null)).toBe('input[name="full-days"]');
    expect(focusKey(close ?? null)).toBe('button[aria-label="Close \\"x\\""]');
    const mark = root.querySelector("circle");
    expect(focusKey(mark)).toBe('[data-tip="Week of Oct 5\\a 3 corrections"]');
    expect(root.querySelector(focusKey(mark) ?? "")).toBe(mark);
  });

  it("keeps focus on a mark with a tip and a form field through a background redraw", () => {
    const markup = `<i data-tip="Oct 5: 3" tabindex="0"></i><i data-tip="Oct 12: 4" tabindex="-1"></i><input name="full-days">`;
    root.innerHTML = markup;
    root.querySelectorAll("i")[1]?.focus();
    redraw(markup);
    expect(document.activeElement?.getAttribute("data-tip")).toBe("Oct 12: 4");
    must<HTMLInputElement>("input").focus();
    redraw(markup);
    expect((document.activeElement as HTMLInputElement).name).toBe("full-days");
  });

  it("puts focus back on the same control after the markup is replaced", () => {
    root.innerHTML = page(`<button data-action="tab" data-id="sources">Sources</button>`);
    must<HTMLButtonElement>('[data-id="sources"]').focus();
    redraw(page(`<button data-action="tab" data-id="sources">Sources</button>`));
    expect(document.activeElement?.getAttribute("data-id")).toBe("sources");
  });

  it("falls back to the page heading when the control is gone", () => {
    root.innerHTML = page(`<button data-action="allow" data-id="codex">Allow</button>`);
    must<HTMLButtonElement>("button").focus();
    redraw(page(`<p>Allowed</p>`));
    expect(document.activeElement?.tagName).toBe("H1");
    expect(must("h1").tabIndex).toBe(-1);
  });

  it("falls back to the current sub-tab when there's no heading", () => {
    root.innerHTML = `<button class="tab on" data-action="settings-pane" data-id="sources">Sources</button><button data-action="allow" data-id="codex">Allow</button>`;
    must<HTMLButtonElement>('[data-action="allow"]').focus();
    redraw(`<button class="tab on" data-action="settings-pane" data-id="sources">Sources</button><p>Allowed</p>`);
    expect(document.activeElement?.getAttribute("data-action")).toBe("settings-pane");
  });

  it("falls back to the first control when there's neither a tab nor a heading", () => {
    root.innerHTML = `<button data-action="allow" data-id="codex">Allow</button>`;
    must<HTMLButtonElement>("button").focus();
    redraw(`<p>Allowed</p><button data-action="other">Other</button>`);
    expect(document.activeElement?.getAttribute("data-action")).toBe("other");
  });

  it("never steals focus that was outside the app", () => {
    root.innerHTML = page("");
    must<HTMLButtonElement>("#outside").focus();
    redraw(page(""));
    expect(document.activeElement?.id).toBe("outside");
  });
});
