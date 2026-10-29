// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { must } from "../dom";
import { HOSTILE, row } from "../test/fixtures";
import { buildPickItems, type PickItem } from "./picker";
import { closePicker, openPicker, pickerOpen, pickStatView } from "./popover";

const items = buildPickItems([row("Anthropic", "Claude Opus", "5", 60), row("OpenAI", "GPT Astra", "6", 34)]);

describe("pickStatView", () => {
  const item: PickItem = { filter: {}, label: "x", path: "", depth: 0, corrections: 60, messages: 100, tasks: 1200, continued: 10, landed: 8 };
  const none: PickItem = { ...item, corrections: 0, messages: 0, tasks: 0, continued: 0, landed: 0 };

  it("shows corrections for the corrections pages", () => {
    expect(pickStatView(item, "corrections").value).toBe("<b>60</b> corrections · 60.0%");
    expect(pickStatView(none, "corrections").value).toBe("<b>0</b> corrections · –");
  });

  it("shows landed and tasks for the prompts page", () => {
    expect(pickStatView(item, "prompts").value).toBe(`<b>80%</b> landed · ${(1200).toLocaleString()} tasks`);
    expect(pickStatView(none, "prompts").value).toBe("<b>–</b> landed · 0 tasks");
  });

  it("shows both rates for the overview", () => {
    expect(pickStatView(item, "both").value).toBe("<b>60.0%</b> corrected · <b>80%</b> landed");
    expect(pickStatView(none, "both").value).toBe("<b>–</b> corrected · <b>–</b> landed");
  });
});

describe("model picker popover", () => {
  let trigger: HTMLButtonElement;
  const onChoose = vi.fn<(i: PickItem) => void>();
  const onClose = vi.fn<(flush: boolean) => void>();
  const open = (list = items) => openPicker(trigger, { items: list, stat: "corrections", isSelected: (i) => i.label === "All AI", onChoose, onClose });
  const input = () => must<HTMLInputElement>(".pick-search");
  const key = (k: string) => input().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));

  beforeEach(() => {
    closePicker(false);
    onChoose.mockReset();
    onClose.mockReset();
    document.body.innerHTML = `<div id="app"><button id="model-trigger" aria-expanded="false">All AI</button><button id="other">x</button></div>`;
    trigger = must<HTMLButtonElement>("#model-trigger");
  });

  it("is a combobox: focus in the field, the highlight announced through aria-activedescendant", () => {
    open();
    expect(pickerOpen()).toBe(true);
    expect(document.activeElement).toBe(input());
    expect(input().getAttribute("role")).toBe("combobox");
    expect(input().getAttribute("aria-controls")).toBe("pick-list");
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(input().getAttribute("aria-activedescendant")).toBe("pick-opt-0");
    expect(must("#pick-opt-0").getAttribute("aria-selected")).toBe("true");
  });

  it("moves the highlight with the arrow keys and chooses with Enter", () => {
    open();
    key("ArrowDown");
    key("ArrowDown");
    expect(input().getAttribute("aria-activedescendant")).toBe("pick-opt-2");
    key("ArrowUp");
    expect(must(".pick-list .active").id).toBe("pick-opt-1");
    key("Enter");
    expect(onChoose).toHaveBeenCalledWith(items[1]);
    expect(onClose).toHaveBeenCalledWith(false); // the choice redraws anyway
    expect(document.activeElement).toBe(trigger);
    expect(pickerOpen()).toBe(false);
    expect(document.querySelector(".picker-pop")).toBeNull();
  });

  it("closes on Escape, returning focus to the trigger and flushing deferred redraws", () => {
    open();
    key("Escape");
    expect(document.activeElement).toBe(trigger);
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(onClose).toHaveBeenCalledWith(true);
    expect(onChoose).not.toHaveBeenCalled();
  });

  it("filters as you type and escapes what was typed", () => {
    open();
    input().value = "gpt";
    input().dispatchEvent(new Event("input"));
    expect(document.querySelectorAll(".pick-list [role=option]")).toHaveLength(1);
    input().value = HOSTILE;
    input().dispatchEvent(new Event("input"));
    expect(must(".pick-empty").textContent).toContain(HOSTILE);
    expect(document.querySelector(".pick-list img")).toBeNull();
    expect(input().hasAttribute("aria-activedescendant")).toBe(false);
  });

  it("chooses on click", () => {
    open();
    must("#pick-opt-1").dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onChoose).toHaveBeenCalledWith(items[1]);
  });

  it("closes when focus moves elsewhere, so a deferred redraw can land", () => {
    open();
    must<HTMLButtonElement>("#other").focus();
    expect(pickerOpen()).toBe(false);
    expect(onClose).toHaveBeenCalledWith(true);
  });

  it("closes on a click outside, but not on the trigger itself", () => {
    open();
    trigger.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(pickerOpen()).toBe(true);
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(pickerOpen()).toBe(false);
  });

  it("shows the chosen stat beside each model", () => {
    openPicker(trigger, { items, stat: "prompts", isSelected: () => false, onChoose, onClose });
    expect(must("#pick-opt-0").textContent).toContain("landed");
    expect(must("#pick-opt-0").textContent).not.toContain("corrections");
    closePicker(false);
    open();
    expect(must("#pick-opt-0").textContent).toContain("corrections");
  });

  it("replaces an open picker instead of stacking a second", () => {
    open();
    open();
    expect(document.querySelectorAll(".picker-pop")).toHaveLength(1);
  });
});
