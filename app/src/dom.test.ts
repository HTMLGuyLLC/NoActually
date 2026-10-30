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
