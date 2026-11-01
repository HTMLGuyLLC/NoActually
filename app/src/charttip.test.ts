// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bindChartTips } from "./charttip";

const tip = () => document.querySelector<HTMLElement>(".chart-tip");

describe("bindChartTips", () => {
  let root: HTMLElement;
  beforeEach(() => {
    document.body.innerHTML = `<div id="root"><svg><circle id="a" tabindex="0" data-tip="Oct 5: 50"></circle><circle id="b" tabindex="0" data-tip="Oct 12: 20"></circle></svg><button id="out">o</button></div>`;
    document.documentElement.className = "";
    root = document.getElementById("root") as HTMLElement;
    bindChartTips(root);
  });
  const focus = (id: string) => document.getElementById(id)?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));

  it("shows the card when a mark gets keyboard focus, and hides it when focus leaves", () => {
    focus("a");
    expect(tip()?.hidden).toBe(false);
    expect(tip()?.textContent).toBe("Oct 5: 50");
    document.getElementById("a")?.dispatchEvent(new FocusEvent("focusout", { bubbles: true, relatedTarget: document.getElementById("out") }));
    expect(tip()?.hidden).toBe(true);
  });

  it("hides the card on Escape, and quiets the CSS tooltips until the pointer moves", () => {
    focus("a");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(tip()?.hidden).toBe(true);
    expect(document.documentElement.classList.contains("tips-off")).toBe(true);
    document.dispatchEvent(new Event("pointermove"));
    expect(document.documentElement.classList.contains("tips-off")).toBe(false);
  });

  describe("keyboard", () => {
    const el = (id: string) => document.getElementById(id) as unknown as SVGElement;
    const press = (id: string, key: string) => {
      const e = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      el(id).dispatchEvent(e);
      return e;
    };

    beforeEach(() => {
      document.getElementById("b")?.setAttribute("tabindex", "-1");
      document.body.querySelector("svg")?.insertAdjacentHTML("beforeend", '<circle id="c" tabindex="-1" data-tip="Oct 19: 80"></circle>');
    });

    it("moves along the marks with the arrow keys, Home and End, stopping at the ends", () => {
      const spies = { a: vi.spyOn(el("a"), "focus"), b: vi.spyOn(el("b"), "focus"), c: vi.spyOn(el("c"), "focus") };
      expect(press("a", "ArrowRight").defaultPrevented).toBe(true);
      expect(spies.b).toHaveBeenCalledTimes(1);
      press("b", "ArrowRight");
      expect(spies.c).toHaveBeenCalledTimes(1);
      press("c", "ArrowRight"); // the last one stays put
      expect(spies.c).toHaveBeenCalledTimes(2);
      press("c", "Home");
      expect(spies.a).toHaveBeenCalledTimes(1);
      press("a", "End");
      expect(spies.c).toHaveBeenCalledTimes(3);
      press("c", "ArrowLeft");
      expect(spies.b).toHaveBeenCalledTimes(2);
    });

    it("leaves other keys alone", () => {
      expect(press("a", "Tab").defaultPrevented).toBe(false);
      expect(press("a", "Enter").defaultPrevented).toBe(false);
    });

    it("makes the mark you land on the chart's one tab stop", () => {
      focus("c");
      expect(el("c").getAttribute("tabindex")).toBe("0");
      expect(el("a").getAttribute("tabindex")).toBe("-1");
      expect(el("b").getAttribute("tabindex")).toBe("-1");
    });
  });
});
