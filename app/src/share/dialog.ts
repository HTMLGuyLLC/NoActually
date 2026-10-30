// The share dialog: what will be posted, one button per network, and image / text fallbacks.
import { api, demo } from "../api";
import { data, must, say } from "../dom";
import { errorText } from "../format";
import { type Html, html } from "../html";
import type { CardShape } from "./cards";
import { shareGuide, shareTargets } from "./share";

export type Post = {
  title: string;
  text: string;
  /** Draws the image to save or post, in the shape the network shows best. */
  image: (shape: CardShape) => Promise<Blob>;
  /** The text quotes a chat, so the person may edit it before it goes anywhere. */
  quotesChat?: boolean;
};

const PASTE_KEY = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent) ? "⌘V" : "Ctrl+V";

const writeClipboard = (text: string) =>
  navigator.clipboard.writeText(text).then(
    () => true,
    () => false,
  );

async function copyText(text: string, out: HTMLElement) {
  say(out, (await writeClipboard(text)) ? "Copied. Paste it anywhere." : text);
}

/** Saves the post's image. `copyImage` also puts it on the clipboard so it pastes into a post. */
async function saveImage(post: Post, out: HTMLElement | null, copyImage = true, shape: CardShape = "wide"): Promise<boolean> {
  say(out, "Drawing your card…");
  try {
    const blob = await post.image(shape);
    let copied = false;
    if (copyImage) {
      try {
        await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
        copied = true;
      } catch {
        /* clipboard images aren't available everywhere */
      }
    }
    if (demo) {
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = "no-actually.png";
      a.click();
      say(out, "Card downloaded.");
      return true;
    }
    const path = await api.saveShareCard(Array.from(new Uint8Array(await blob.arrayBuffer())));
    say(out, path ? `Saved${copied ? " and copied" : ""}.` : copied ? "Copied the image." : "");
    return !!path;
  } catch (e) {
    say(out, `Couldn't make the card: ${errorText(e)}`);
    return false;
  }
}

/** Says what happens next in the share dialog; `paste` makes it the loud, can't-miss version. */
function guide(out: HTMLElement, markup: Html, paste = false) {
  out.innerHTML = markup.value;
  out.className = `share-guide${paste ? " paste" : ""}`;
}

async function share(id: string, post: Post, text: string, out: HTMLElement) {
  const target = shareTargets(text).find((t) => t.id === id);
  if (!target) return;
  const next = shareGuide(target, await writeClipboard(text), PASTE_KEY);
  if (next.saveImage && !(await saveImage(post, null, false, target.card))) return;
  guide(out, next.guide, next.paste);
  if (!next.open || !target.href) return;
  try {
    await api.openShare(target.href);
  } catch (e) {
    guide(out, html`${errorText(e)}`);
  }
}

/**
 * Opens the dialog. On close, focus goes back to `opener` and `onClose` runs (to apply a redraw that
 * waited for the dialog).
 */
export function openShareDialog(post: Post, opener: HTMLElement | null, onClose: () => void): HTMLDialogElement {
  document.querySelector("dialog.share-dialog")?.remove();
  const dlg = document.createElement("dialog");
  dlg.className = "share-dialog";
  dlg.setAttribute("aria-labelledby", "share-title");
  const note = post.quotesChat
    ? "This quotes your chat (already redacted). Edit it above before sharing if you like."
    : "Only these numbers and the project link are shared. Never your chats.";
  dlg.innerHTML = html`
    <div class="sd-head"><h2 id="share-title">${post.title}</h2><button class="sd-close" data-sd="close" aria-label="Close">×</button></div>
    <label class="eyebrow" for="share-text">What you'll post${post.quotesChat ? " · you can edit it" : ""}</label>
    <textarea id="share-text" rows="${post.quotesChat ? 4 : 3}" ${post.quotesChat ? "" : "readonly"}>${post.text}</textarea>
    <div class="share-grid">${shareTargets(post.text).map(
      (t) =>
        html`<button class="share-btn" data-sd="share" data-id="${t.id}"><b>${t.label}</b>${
          t.href
            ? t.pasteText
              ? html`<span class="needs-paste">You'll paste the text</span>`
              : html`<span>Text filled in</span>`
            : html`<span class="needs-paste">Saves image · paste caption</span>`
        }</button>`,
    )}</div>
    <div id="share-guide" class="share-guide" role="status" aria-live="polite">${note}</div>
    <div class="sd-foot"><button class="ghost" data-sd="image">Save image</button><button class="ghost" data-sd="copy">Copy text</button></div>`.value;
  document.body.appendChild(dlg);
  const out = must("#share-guide", dlg);
  const box = must<HTMLTextAreaElement>("#share-text", dlg);
  const close = () => {
    dlg.close();
    dlg.remove();
    if (opener?.isConnected) opener.focus();
    onClose();
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
    const b = e.target instanceof Element ? e.target.closest<HTMLButtonElement>("button[data-sd]") : null;
    if (!b) return;
    const what = b.dataset.sd;
    if (what === "close") close();
    else if (what === "share") void share(data(b, "id"), post, box.value, out);
    else if (what === "copy") void copyText(box.value, out);
    else if (what === "image") void saveImage(post, out);
  });
  dlg.showModal();
  dlg.querySelector<HTMLButtonElement>('[data-sd="share"]')?.focus();
  return dlg;
}
