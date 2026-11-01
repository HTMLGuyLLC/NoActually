// The share dialog: what will be posted, one button per network, and image / text fallbacks.
import { api, demo } from "../api";
import { data, must, say } from "../dom";
import { errorText } from "../format";
import { EMPTY, type Html, html } from "../html";
import { openModal } from "../modal";
import type { CardShape } from "./cards";
import { type CorrectionLike, correctionShareText, shareGuide, shareTargets } from "./share";

export type Post = {
  title: string;
  text: string;
  /** Draws the image to save or post, in the shape the network shows best. `quotes` are the edited quotes, when the post has them. */
  image: (shape: CardShape, quotes?: CorrectionLike) => Promise<Blob>;
  /** The text quotes a chat as one box, so the person may edit it before it goes anywhere. The image can't follow it; use `quotes` for that. */
  quotesChat?: boolean;
  /**
   * A correction's two quotes. The person edits each in its own box, and the post text, the image
   * and every network draw from the edited words, never from the stored entry.
   */
  quotes?: CorrectionLike;
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
async function saveImage(post: Post, out: HTMLElement | null, quotes?: CorrectionLike, copyImage = true, shape: CardShape = "wide"): Promise<boolean> {
  say(out, "Drawing your card…");
  try {
    const blob = await post.image(shape, quotes);
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

async function share(id: string, post: Post, text: string, quotes: CorrectionLike | undefined, out: HTMLElement) {
  const target = shareTargets(text).find((t) => t.id === id);
  if (!target) return;
  const next = shareGuide(target, await writeClipboard(text), PASTE_KEY);
  if (next.saveImage && !(await saveImage(post, null, quotes, false, target.card))) return;
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
  const note =
    post.quotes || post.quotesChat
      ? "This quotes your chat. Redaction can miss things. Check it before you share."
      : "Only these numbers and the project link are shared. Never your chats.";
  const { dlg } = openModal({
    className: "share-dialog",
    labelledBy: "share-title",
    opener,
    onClose,
    focus: '[data-sd="share"]',
    markup: html`
    <div class="sd-head"><h2 id="share-title">${post.title}</h2><button class="sd-close" data-sd="close" aria-label="Close">×</button></div>
    ${
      post.quotes
        ? html`<label class="eyebrow" for="share-human">Your message · you can edit it</label>
    <textarea id="share-human" rows="2">${post.quotes.human ?? ""}</textarea>
    <label class="eyebrow" for="share-ai">${post.quotes.model || "The AI"}'s reply · you can edit it</label>
    <textarea id="share-ai" rows="2">${post.quotes.ai ?? ""}</textarea>`
        : EMPTY
    }
    <label class="eyebrow" for="share-text">What you'll post${post.quotesChat ? " · you can edit it" : ""}</label>
    <textarea id="share-text" rows="${post.quotesChat || post.quotes ? 4 : 3}" ${post.quotesChat && !post.quotes ? "" : "readonly"}>${post.quotes ? correctionShareText(post.quotes) : post.text}</textarea>
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
    <div class="sd-foot"><button class="ghost" data-sd="image">Save image</button><button class="ghost" data-sd="copy">Copy text</button></div>`,
    onClick: (target, close) => {
      const b = target.closest<HTMLButtonElement>("button[data-sd]");
      if (!b) return;
      const what = b.dataset.sd;
      if (what === "close") close();
      else if (what === "share") void share(data(b, "id"), post, box.value, edited(), out);
      else if (what === "copy") void copyText(box.value, out);
      else if (what === "image") void saveImage(post, out, edited());
    },
  });
  const out = must("#share-guide", dlg);
  const box = must<HTMLTextAreaElement>("#share-text", dlg);
  const { quotes } = post;
  const human = quotes ? must<HTMLTextAreaElement>("#share-human", dlg) : null;
  const reply = quotes ? must<HTMLTextAreaElement>("#share-ai", dlg) : null;
  /** The quotes as edited. Everything that leaves the dialog draws from these. */
  function edited(): CorrectionLike | undefined {
    return quotes && human && reply ? { model: quotes.model, human: human.value.trim() || null, ai: reply.value.trim() || null } : undefined;
  }
  const refresh = () => {
    const q = edited();
    if (q) box.value = correctionShareText(q);
  };
  human?.addEventListener("input", refresh);
  reply?.addEventListener("input", refresh);
  return dlg;
}
