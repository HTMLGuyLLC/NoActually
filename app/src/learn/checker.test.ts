// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Check } from "../api";
import { meta } from "../test/fixtures";

const checkBrief = vi.fn<(text: string) => Promise<Check>>();
vi.mock("../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../api")>()), api: { checkBrief }, demo: false }));

const { refreshCheck } = await import("./checker");
const state = await import("../state");

const check = (signals: number): Check => ({ signals, grade: signals === 7 ? "sharp" : "thin", rules: [], not_a_brief: false });

describe("refreshCheck", () => {
  let root: HTMLElement;
  beforeEach(() => {
    state.resetState();
    state.setMeta(meta);
    root = document.createElement("div");
    root.innerHTML = `<div id="check-result"></div>`;
    checkBrief.mockReset();
  });

  it("never lets a slow answer for an older draft overwrite a newer one", async () => {
    let finishOld: (c: Check) => void = () => {};
    checkBrief.mockImplementationOnce(() => new Promise((r) => (finishOld = r))).mockResolvedValueOnce(check(7));
    state.setDraft("old draft");
    const first = refreshCheck(root);
    state.setDraft("new draft");
    await refreshCheck(root);
    expect(root.textContent).toContain("Sharp");
    finishOld(check(0));
    await first;
    expect(root.textContent).toContain("Sharp");
    expect(checkBrief.mock.calls.map((c) => c[0])).toEqual(["old draft", "new draft"]);
  });

  it("shows the prompt for an empty draft without asking Rust", async () => {
    state.setDraft("   ");
    await refreshCheck(root);
    expect(checkBrief).not.toHaveBeenCalled();
    expect(root.textContent).toContain("Type or paste a prompt");
  });

  it("says so when a check fails", async () => {
    checkBrief.mockRejectedValueOnce(new Error("boom"));
    state.setDraft("Fix it");
    await refreshCheck(root);
    expect(root.textContent).toBe("Couldn't check that draft.");
  });
});
