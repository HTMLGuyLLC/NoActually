import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { dismiss, isDismissed } from "./hints";
import { memoryStorage } from "./test/fixtures";

beforeEach(() => vi.stubGlobal("localStorage", memoryStorage()));
afterEach(() => vi.unstubAllGlobals());

describe("dismissed tips", () => {
  it("shows a tip until it is dismissed, then remembers", () => {
    expect(isDismissed("report-cards")).toBe(false);
    dismiss("report-cards");
    expect(isDismissed("report-cards")).toBe(true);
    expect(localStorage.getItem("noactually.hint.report-cards")).toBe("1");
  });

  it("shows the tip again, rather than failing, when storage is blocked", () => {
    const blocked = () => {
      throw new Error("blocked");
    };
    vi.stubGlobal("localStorage", { getItem: blocked, setItem: blocked });
    expect(isDismissed("report-cards")).toBe(false);
    expect(() => dismiss("report-cards")).not.toThrow();
  });

  it("shows the tip when there is no storage at all", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(isDismissed("report-cards")).toBe(false);
    expect(() => dismiss("report-cards")).not.toThrow();
  });
});
