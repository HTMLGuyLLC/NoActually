// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { must } from "../dom";
import { HOSTILE } from "../test/fixtures";
import { openShareDialog, type Post } from "./dialog";

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("share dialog", () => {
  const writeText = vi.fn(async (_: string) => {});
  const onClose = vi.fn();
  let opener: HTMLButtonElement;
  const post = (over: Partial<Post> = {}): Post => ({ title: "Share your score", text: "Could AI replace me? No.", image: async () => new Blob(), ...over });
  const dialog = () => must<HTMLDialogElement>("dialog.share-dialog");
  const click = (sel: string) => must<HTMLButtonElement>(sel, dialog()).click();

  beforeEach(() => {
    document.body.innerHTML = `<button id="opener">Share</button>`;
    opener = must<HTMLButtonElement>("#opener");
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    vi.spyOn(window, "open").mockImplementation(() => null);
  });
  afterEach(() => {
    writeText.mockReset();
    onClose.mockReset();
  });

  it("shows the text read-only, escaped, with one button per network", () => {
    openShareDialog(post({ title: HOSTILE, text: HOSTILE }), opener, onClose);
    expect(dialog().querySelector("img")).toBeNull();
    expect(must<HTMLTextAreaElement>("#share-text").value).toBe(HOSTILE);
    expect(must<HTMLTextAreaElement>("#share-text").readOnly).toBe(true);
    expect(dialog().querySelectorAll('[data-sd="share"]')).toHaveLength(8);
    expect(document.activeElement?.getAttribute("data-id")).toBe("x");
  });

  it("lets the person edit text that quotes a chat", () => {
    openShareDialog(post({ quotesChat: true }), opener, onClose);
    expect(must<HTMLTextAreaElement>("#share-text").readOnly).toBe(false);
    expect(must("#share-guide").textContent).toContain("This quotes your chat");
  });

  describe("a correction's two quotes", () => {
    const quotes = { model: "Claude", human: "my client Acme owes me", ai: "You're right about Acme." };
    const edit = (sel: string, value: string) => {
      const el = must<HTMLTextAreaElement>(sel, dialog());
      el.value = value;
      el.dispatchEvent(new Event("input", { bubbles: true }));
    };

    it("edits each quote in its own box and warns that redaction can miss things", () => {
      openShareDialog(post({ quotes }), opener, onClose);
      expect(must<HTMLTextAreaElement>("#share-human").value).toBe(quotes.human);
      expect(must<HTMLTextAreaElement>("#share-ai").value).toBe(quotes.ai);
      expect(must<HTMLTextAreaElement>("#share-text").readOnly).toBe(true);
      expect(must("#share-guide").textContent).toContain("Redaction can miss things");
      expect(must("#share-guide").textContent).not.toContain("already redacted");
    });

    it("draws Save image and Instagram from the edited quotes", async () => {
      const image = vi.fn(async () => new Blob());
      URL.createObjectURL = vi.fn(() => "blob:card");
      openShareDialog(post({ quotes, image }), opener, onClose);
      edit("#share-human", "my client owes me");
      edit("#share-ai", "");
      click('[data-sd="image"]');
      await settle();
      expect(image).toHaveBeenLastCalledWith("wide", { model: "Claude", human: "my client owes me", ai: null });
      click('[data-sd="share"][data-id="instagram"]');
      await settle();
      expect(image).toHaveBeenLastCalledWith("portrait", { model: "Claude", human: "my client owes me", ai: null });
    });

    it("sends the edited quotes to the network and the clipboard", async () => {
      openShareDialog(post({ quotes }), opener, onClose);
      edit("#share-human", "my client owes me");
      expect(must<HTMLTextAreaElement>("#share-text").value).not.toContain("Acme owes");
      click('[data-sd="share"][data-id="x"]');
      await settle();
      const sent = String(vi.mocked(window.open).mock.calls.at(-1)?.[0]);
      expect(decodeURIComponent(sent)).toContain("Me: “my client owes me”");
      expect(sent).not.toContain("owes%20me%20Acme");
      expect(writeText).toHaveBeenLastCalledWith(expect.stringContaining("my client owes me"));
      expect(writeText).toHaveBeenLastCalledWith(expect.not.stringContaining("Acme”"));
    });
  });

  it("copies the text and opens a network that takes it", async () => {
    openShareDialog(post(), opener, onClose);
    click('[data-sd="share"][data-id="x"]');
    await settle();
    expect(writeText).toHaveBeenCalledWith("Could AI replace me? No.");
    expect(must("#share-guide").textContent).toBe("Opening X with your post filled in…");
    expect(window.open).toHaveBeenCalledWith(expect.stringContaining("https://x.com/intent/post?text="), "_blank", "noopener");
  });

  it("makes the paste step loud where the network ignores the text", async () => {
    openShareDialog(post(), opener, onClose);
    click('[data-sd="share"][data-id="facebook"]');
    await settle();
    expect(must("#share-guide").className).toBe("share-guide paste");
  });

  it("copies the text on Copy text, or shows it when the clipboard refuses", async () => {
    openShareDialog(post(), opener, onClose);
    click('[data-sd="copy"]');
    await settle();
    expect(must("#share-guide").textContent).toBe("Copied. Paste it anywhere.");
    writeText.mockRejectedValueOnce(new Error("denied"));
    click('[data-sd="copy"]');
    await settle();
    expect(must("#share-guide").textContent).toBe("Could AI replace me? No.");
  });

  it("reports a card that couldn't be drawn", async () => {
    openShareDialog(
      post({
        image: async () => {
          throw new Error("no canvas");
        },
      }),
      opener,
      onClose,
    );
    click('[data-sd="image"]');
    await settle();
    expect(must("#share-guide").textContent).toBe("Couldn't make the card: no canvas");
  });

  it("saves the tall card for Instagram and the wide one from Save image", async () => {
    const image = vi.fn(async () => new Blob());
    URL.createObjectURL = vi.fn(() => "blob:card");
    openShareDialog(post({ image }), opener, onClose);
    click('[data-sd="share"][data-id="instagram"]');
    await settle();
    expect(image).toHaveBeenLastCalledWith("portrait", undefined);
    expect(must("#share-guide").textContent).toContain("Post the image from the Instagram app");
    click('[data-sd="image"]');
    await settle();
    expect(image).toHaveBeenLastCalledWith("wide", undefined);
  });

  it("closes from the × button, focusing the opener and running onClose", () => {
    openShareDialog(post(), opener, onClose);
    click('[data-sd="close"]');
    expect(document.querySelector("dialog.share-dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on a click outside the box, not on its padding", () => {
    openShareDialog(post(), opener, onClose);
    const box = { left: 100, right: 500, top: 50, bottom: 400 } as DOMRect;
    vi.spyOn(dialog(), "getBoundingClientRect").mockReturnValue(box);
    const clickAt = (clientX: number, clientY: number) => dialog().dispatchEvent(new MouseEvent("click", { bubbles: true, clientX, clientY }));
    clickAt(300, 200); // padding inside the box
    expect(document.querySelector("dialog.share-dialog")).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    clickAt(50, 200); // backdrop
    expect(document.querySelector("dialog.share-dialog")).toBeNull();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("replaces an open dialog rather than stacking", () => {
    openShareDialog(post(), opener, onClose);
    openShareDialog(post(), opener, onClose);
    expect(document.querySelectorAll("dialog.share-dialog")).toHaveLength(1);
  });
});
