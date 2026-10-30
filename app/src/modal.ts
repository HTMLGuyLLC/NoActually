// A modal dialog the page builds itself: named for screen readers, closed with Esc or a click outside,
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

/** Opens the dialog and puts focus in it. */
export function openModal(o: ModalOptions): Modal {
  document.querySelector(`dialog.${o.className}`)?.remove();
  const dlg = document.createElement("dialog");
  dlg.className = o.className;
  dlg.setAttribute("aria-labelledby", o.labelledBy);
  if (o.describedBy) dlg.setAttribute("aria-describedby", o.describedBy);
  dlg.innerHTML = o.markup.value;
  document.body.appendChild(dlg);
  const close = () => {
    dlg.close();
    dlg.remove();
    if (o.opener?.isConnected) o.opener.focus();
    o.onClose?.();
  };
  dlg.addEventListener("cancel", (e) => {
    e.preventDefault(); // Esc: close our way, so focus and redraws are handled
    close();
  });
  dlg.addEventListener("click", (e) => {
    if (e.target === dlg) {
      // A click on the dialog itself is the backdrop only when it lands outside the box, not on its padding or gaps.
      const r = dlg.getBoundingClientRect();
      if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) close();
      return;
    }
    if (e.target instanceof Element) o.onClick?.(e.target, close);
  });
  dlg.showModal();
  if (o.focus) dlg.querySelector<HTMLElement>(o.focus)?.focus();
  return { dlg, close };
}
