import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { dismiss, dismissRescored, isDismissed, rescoredUnseen } from "./hints";
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

describe("the rescored line", () => {
  const first = "2026-11-01T10:00:00Z";
  it("shows nothing without a notice, and a notice until it is dismissed", () => {
    expect(rescoredUnseen(null)).toBe(false);
    expect(rescoredUnseen(first)).toBe(true);
    dismissRescored(first);
    expect(rescoredUnseen(first)).toBe(false);
  });

  it("shows again for a later rebuild, not an earlier one", () => {
    dismissRescored(first);
    expect(rescoredUnseen("2026-11-02T10:00:00Z")).toBe(true);
    expect(rescoredUnseen("2026-10-30T10:00:00Z")).toBe(false);
  });

  it("shows the line, rather than failing, when storage is blocked", () => {
    const blocked = () => {
      throw new Error("blocked");
    };
    vi.stubGlobal("localStorage", { getItem: blocked, setItem: blocked });
    expect(rescoredUnseen(first)).toBe(true);
    expect(() => dismissRescored(first)).not.toThrow();
  });
});
