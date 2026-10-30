// Turning report cards on asks first: one screen says what will be added and where it comes from.
import type { ReportCardTool } from "./api";
import { html } from "./html";
import { openModal } from "./modal";

/** What the report cards section calls each tool, the command that asks for one in it, and the source the app has to read its chats from. */
export const TOOL_WORDS: Record<ReportCardTool, { name: string; command: string; source: string }> = {
  claude_code: { name: "Claude Code", command: "/grade:me", source: "claude-code" },
  codex: { name: "Codex", command: "$grade-me", source: "codex" },
};

/** The one thing Codex asks of the person, which the confirm screen says before it happens. */
export const CODEX_TRUST = "Codex will ask you to trust this now, and again after app updates. Say yes.";

/** What the screen lists. Codex downloads nothing (its files are written here), and has no tips yet. */
export function confirmLines(tool: ReportCardTool): string[] {
  const claude = tool === "claude_code";
  return [
    claude
      ? "Adds a small No, Actually... plugin to Claude Code, downloaded from GitHub. It updates when the app does."
      : "Adds two small files Codex uses to run report cards. Your Codex settings are backed up first.",
    "A small check runs on this computer after each reply. It uses no AI and sends nothing.",
    claude
      ? "Report cards are written by your AI, so they use your tokens: only when you ask, plus a little in each chat if you turn on tips."
      : "Report cards are written by your AI, so they use your tokens: only when you ask. Tips are Claude Code only for now.",
    "Shows a one-line reminder when a task ends (no tokens). You can turn that off.",
    "When you ask for a report card, your AI reads a short summary of the chat that No, Actually... makes on your computer. No, Actually... itself sends nothing anywhere.",
    ...(claude ? [] : [CODEX_TRUST]),
  ];
}

/** What to say once a tool is on: where to go next. */
export const doneLine = (tool: ReportCardTool): string =>
  `Done. Start a new ${TOOL_WORDS[tool].name} chat, finish a task, then type ${TOOL_WORDS[tool].command}.`;

/** What to say once a tool is off. Chats already open keep their tips, so say how to stop those if tips were on. */
export const offLine = (tool: ReportCardTool, tipsWereOn = false): string =>
  `Report cards are off in ${TOOL_WORDS[tool].name}.${tool === "claude_code" && tipsWereOn ? " Chats already open keep their tips. Tell your AI “no more tips” in those." : ""}`;

/**
 * Opens the screen. Resolves true on Turn on, false on Cancel, Esc or a click outside it. Focus goes back to
 * `opener` first, so the redraw that follows keeps it on the same button; `onClose` then runs (to apply a redraw
 * that waited for the dialog).
 */
export function confirmTurnOn(tool: ReportCardTool, opener: HTMLElement | null, onClose?: () => void): Promise<boolean> {
  return new Promise((resolve) => {
    let answer = false;
    openModal({
      className: "confirm-dialog",
      labelledBy: "confirm-title",
      describedBy: "confirm-list",
      markup: html`
      <h2 id="confirm-title">Turn on report cards in ${TOOL_WORDS[tool].name}?</h2>
      <ul id="confirm-list" class="confirm-list">${confirmLines(tool).map((line) => html`<li>${line}</li>`)}</ul>
      <div class="sd-foot"><button data-confirm-dialog="yes">Turn on</button><button class="ghost" data-confirm-dialog="no">Cancel</button></div>`,
      opener,
      focus: '[data-confirm-dialog="yes"]',
      onClick: (target, close) => {
        const b = target.closest<HTMLButtonElement>("button[data-confirm-dialog]");
        if (!b) return;
        answer = b.dataset.confirmDialog === "yes";
        close();
      },
      onClose: () => {
        resolve(answer);
        onClose?.();
      },
    });
  });
}
