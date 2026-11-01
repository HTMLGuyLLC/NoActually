// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { must } from "./dom";
import { html } from "./html";
import { type ModalOptions, openModal } from "./modal";

describe("openModal", () => {
  let opener: HTMLButtonElement;
  const onClose = vi.fn();
  const onClick = vi.fn();
  const open = (over: Partial<ModalOptions> = {}) =>
    openModal({
      className: "demo-dialog",
      labelledBy: "demo-title",
      markup: html`<h2 id="demo-title">Hello</h2><p id="demo-text">What this is.</p><button id="first">One</button><button id="second">Two</button>`,
      opener,
      onClose,
      onClick,
      ...over,
    });

  beforeEach(() => {
    document.body.innerHTML = `<button id="opener">Open</button>`;
    opener = must<HTMLButtonElement>("#opener");
    onClose.mockReset();
    onClick.mockReset();
  });

  it("opens a modal dialog with its markup, named by its heading", () => {
    const { dlg } = open();
    expect(dlg.open).toBe(true);
    expect(dlg.className).toBe("demo-dialog");
    expect(dlg.getAttribute("aria-labelledby")).toBe("demo-title");
    expect(dlg.getAttribute("aria-describedby")).toBeNull();
    expect(must("h2", dlg).textContent).toBe("Hello");
    expect(document.body.contains(dlg)).toBe(true);
  });

  it("can be described by a part of itself, so a screen reader reads it on opening", () => {
    expect(open({ describedBy: "demo-text" }).dlg.getAttribute("aria-describedby")).toBe("demo-text");
  });

  it("puts focus on the control asked for", () => {
    open({ focus: "#second" });
    expect(document.activeElement?.id).toBe("second");
  });

  it("replaces another dialog of the same class, and leaves other classes alone", () => {
    const other = document.createElement("dialog");
    other.className = "other-dialog";
    document.body.appendChild(other);
    const first = open().dlg;
    const second = open().dlg;
    expect(document.body.contains(first)).toBe(false);
    expect(document.body.contains(second)).toBe(true);
    expect(document.body.contains(other)).toBe(true);
  });

  it("closes on Esc, with focus back on the opener and onClose after it", () => {
    const { dlg } = open({ focus: "#first" });
    const event = new Event("cancel", { cancelable: true });
    dlg.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true); // the page closes it, not the browser
    expect(document.body.contains(dlg)).toBe(false);
    expect(document.activeElement).toBe(opener);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  const box = (dlg: HTMLDialogElement) => {
    dlg.getBoundingClientRect = () => ({ left: 100, right: 300, top: 100, bottom: 300, width: 200, height: 200, x: 100, y: 100, toJSON: () => ({}) });
  };
  const at = (type: string, x: number, y: number, target: EventTarget) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX: x, clientY: y }));

  it("closes on a click outside its box, but not on its own padding", () => {
    const { dlg } = open();
    box(dlg);
    at("mousedown", 150, 150, dlg);
    at("click", 150, 150, dlg);
    expect(onClose).not.toHaveBeenCalled();
    expect(document.body.contains(dlg)).toBe(true);
    at("mousedown", 10, 10, dlg);
    at("click", 10, 10, dlg);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.body.contains(dlg)).toBe(false);
  });

  it("stays open when a selection is dragged out of a box and let go on the backdrop", () => {
    const { dlg } = open();
    box(dlg);
    at("mousedown", 150, 150, must("#first", dlg)); // the press began inside
    at("click", 10, 10, dlg); // the click lands on the common ancestor, outside the box
    expect(onClose).not.toHaveBeenCalled();
    expect(document.body.contains(dlg)).toBe(true);
  });

  it("stays open when a press began on the backdrop and ended inside", () => {
    const { dlg } = open();
    box(dlg);
    at("mousedown", 10, 10, dlg);
    at("click", 150, 150, dlg);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("forgets an earlier backdrop press once the click has come", () => {
    const { dlg } = open();
    box(dlg);
    at("mousedown", 10, 10, dlg);
    at("click", 150, 150, dlg); // ended inside: no close, and the press is spent
    at("click", 10, 10, dlg); // a click with no press of its own
    expect(onClose).not.toHaveBeenCalled();
  });

  it("hands a click inside to the page's handler, with a way to close", () => {
    const { dlg } = open({
      onClick: (target, close) => {
        onClick(target.id);
        if (target.id === "second") close();
      },
    });
    must<HTMLButtonElement>("#first", dlg).click();
    expect(onClick).toHaveBeenLastCalledWith("first");
    expect(document.body.contains(dlg)).toBe(true);
    must<HTMLButtonElement>("#second", dlg).click();
    expect(onClick).toHaveBeenLastCalledWith("second");
    expect(document.body.contains(dlg)).toBe(false);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(opener);
  });

  it("can be closed from outside, and doesn't fail when the opener has gone", () => {
    const { close, dlg } = open();
    opener.remove();
    close();
    expect(document.body.contains(dlg)).toBe(false);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("works without an opener or an onClose", () => {
    const { close, dlg } = openModal({ className: "demo-dialog", labelledBy: "demo-title", markup: html`<h2 id="demo-title">Hello</h2>`, opener: null });
    close();
    expect(document.body.contains(dlg)).toBe(false);
  });

  describe("without showModal (Safari before 15.4)", () => {
    const proto = HTMLDialogElement.prototype;
    const real = { showModal: proto.showModal, close: proto.close };
    beforeEach(() => {
      Object.defineProperty(proto, "showModal", { value: undefined, configurable: true, writable: true });
      Object.defineProperty(proto, "close", { value: undefined, configurable: true, writable: true });
    });
    afterEach(() => {
      Object.defineProperty(proto, "showModal", { value: real.showModal, configurable: true, writable: true });
      Object.defineProperty(proto, "close", { value: real.close, configurable: true, writable: true });
    });

    it("still opens the dialog, shows it, and focuses the control asked for", () => {
      const { dlg } = open({ focus: "#second" });
      expect(dlg.hasAttribute("open")).toBe(true);
      expect(dlg.classList.contains("modal-fallback")).toBe(true);
      expect(dlg.getAttribute("aria-modal")).toBe("true");
      expect(document.activeElement?.id).toBe("second");
    });

    it("focuses the first control when none is asked for", () => {
      open();
      expect(document.activeElement?.id).toBe("first");
    });

    it("holds the rest of the page still while open, and gives it back on close", () => {
      const { close } = open();
      expect(opener.hasAttribute("inert")).toBe(true);
      close();
      expect(opener.hasAttribute("inert")).toBe(false);
      expect(document.activeElement).toBe(opener);
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it("closes on Esc, which the browser no longer reports", () => {
      const { dlg } = open();
      dlg.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      expect(document.body.contains(dlg)).toBe(false);
      expect(onClose).toHaveBeenCalledTimes(1);
    });
  });
});
