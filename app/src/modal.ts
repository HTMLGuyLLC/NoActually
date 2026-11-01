// A modal dialog the page builds itself: named for screen readers, closed with Esc or a click outside it,
// and focus goes back to the control that opened it. The share dialog and the report card confirm screen use it.
import type { Html } from "./html";

export type ModalOptions = {
  /** The dialog's class. Opening one removes any other dialog with the same class. */
  className: string;
  /** The id of the heading inside `markup` that names the dialog. */
  labelledBy: string;
  /** The id of the text inside `markup` that says what it's about, so a screen reader reads it on opening. */
  describedBy?: string;
  /** What's inside it. */
  markup: Html;
  /** Focus goes back here on close, if it's still on the page. */
  opener: HTMLElement | null;
  /** Runs after it has closed and focus is back (to apply a redraw that waited for the dialog). */
  onClose?: () => void;
  /** A click on anything inside it (the backdrop and Esc are handled here); `close` closes it. */
  onClick?: (target: Element, close: () => void) => void;
  /** The control to focus first, as a selector inside the dialog. */
  focus?: string;
};

export type Modal = { dlg: HTMLDialogElement; close: () => void };

/** Gives back the page a fallback dialog held still (the dialog that held it is gone or going). */
function release(): void {
  for (const el of document.querySelectorAll("[data-held]")) {
    el.removeAttribute("inert");
    el.removeAttribute("data-held");
  }
}

/** Opens the dialog and puts focus in it. */
export function openModal(o: ModalOptions): Modal {
  document.querySelector(`dialog.${o.className}`)?.remove();
  release();
  const dlg = document.createElement("dialog");
  dlg.className = o.className;
  dlg.setAttribute("aria-labelledby", o.labelledBy);
  if (o.describedBy) dlg.setAttribute("aria-describedby", o.describedBy);
  dlg.innerHTML = o.markup.value;
  document.body.appendChild(dlg);
  // Safari before 15.4 has no showModal (macOS 12.0 to 12.2 may not have the update): the page then draws
  // the dialog over a dimmed page itself, and closes it with Esc itself.
  const native = typeof dlg.showModal === "function";
  const close = () => {
    if (typeof dlg.close === "function") dlg.close();
    else dlg.removeAttribute("open");
    dlg.remove();
    release();
    if (o.opener?.isConnected) o.opener.focus();
    o.onClose?.();
  };
  dlg.addEventListener("cancel", (e) => {
    e.preventDefault(); // Esc: close our way, so focus and redraws are handled
    close();
  });
  // The click is a backdrop click only if the press began there too: dragging a selection out of a box ends outside it.
  const outside = (e: MouseEvent) => {
    // A press on the dialog itself is the backdrop only when it lands outside the box, not on its padding or gaps.
    if (e.target !== dlg) return false;
    const r = dlg.getBoundingClientRect();
    return e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom;
  };
  let pressedOutside = false;
  dlg.addEventListener("mousedown", (e) => {
    pressedOutside = outside(e);
  });
  dlg.addEventListener("click", (e) => {
    const began = pressedOutside;
    pressedOutside = false;
    if (e.target === dlg) {
      if (began && outside(e)) close();
      return;
    }
    if (e.target instanceof Element) o.onClick?.(e.target, close);
  });
  if (native) dlg.showModal();
  else {
    dlg.classList.add("modal-fallback");
    dlg.setAttribute("open", "");
    dlg.setAttribute("aria-modal", "true");
    dlg.setAttribute("role", "dialog");
    for (const el of document.body.children) {
      if (el === dlg || el.hasAttribute("inert")) continue;
      el.setAttribute("inert", "");
      el.setAttribute("data-held", "");
    }
    dlg.addEventListener("keydown", (e) => {
      if (e.key === "Escape") close();
    });
  }
  const first = o.focus ? dlg.querySelector<HTMLElement>(o.focus) : native ? null : dlg.querySelector<HTMLElement>("button, input, select, textarea");
  first?.focus();
  return { dlg, close };
}
