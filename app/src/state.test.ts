import { describe, expect, it } from "vitest";
import { checkingLeft, hasOverlayTitleBar, SETTINGS_PANES, setSyncing, state } from "./state";

const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15";
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

describe("Settings", () => {
  it("lists General first and opens on it", () => {
    expect(SETTINGS_PANES[0]).toBe("general");
    expect(state.settingsPane).toBe("general");
  });
});

describe("checkingLeft", () => {
  it("counts down from the start of a check, and stops at zero", () => {
    setSyncing(10_000);
    expect(checkingLeft(10_000)).toBe(2000);
    expect(checkingLeft(11_500)).toBe(500);
    expect(checkingLeft(12_000)).toBe(0);
    expect(checkingLeft(60_000)).toBe(0);
  });
});

describe("hasOverlayTitleBar", () => {
  it("is only the main window of the app on a Mac", () => {
    expect(hasOverlayTitleBar(true, false, MAC)).toBe(true);
    expect(hasOverlayTitleBar(true, true, MAC)).toBe(false);
    expect(hasOverlayTitleBar(false, false, MAC)).toBe(false);
    expect(hasOverlayTitleBar(true, false, WINDOWS)).toBe(false);
  });
});
