// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
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
});
