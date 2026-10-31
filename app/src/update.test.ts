import { describe, expect, it } from "vitest";
import type { UpdateStatus } from "./api";
import { HOSTILE } from "./test/fixtures";
import { updateBanner, updateLine } from "./update";

const status = (over: Partial<UpdateStatus>): UpdateStatus => ({
  current: "0.1.0",
  auto: true,
  channel: "stable",
  checking: false,
  installing: false,
  available: null,
  last_check: null,
  last_error: null,
  ...over,
});
const banner = (u: UpdateStatus, dismissed: string | null = null) => updateBanner(u, dismissed).value;

describe("updateBanner", () => {
  it("shows nothing until an update is waiting", () => {
    expect(banner(status({}))).toBe("");
  });

  it("offers a restart, never forces one", () => {
    const b = banner(status({ available: "0.2.0" }));
    expect(b).toContain("Version 0.2.0 is available.");
    expect(b).toContain('data-action="install-update"');
    expect(b).toContain('data-action="later-update"');
  });

  it("stays away after Later, until a newer version shows up", () => {
    expect(banner(status({ available: "0.2.0" }), "0.2.0")).toBe("");
    expect(banner(status({ available: "0.3.0" }), "0.2.0")).toContain("0.3.0");
  });

  it("shows progress while installing, with no buttons", () => {
    const b = banner(status({ available: "0.2.0", installing: true }));
    expect(b).toContain("Updating to 0.2.0…");
    expect(b).not.toContain("<button");
  });

  it("escapes the version string, which comes from the network", () => {
    for (const installing of [false, true]) {
      const b = banner(status({ available: HOSTILE, installing }));
      expect(b).not.toContain("<img");
      expect(b).toContain("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;&amp;&#39;&quot;");
    }
  });
});

describe("updateLine", () => {
  it("describes each state", () => {
    expect(updateLine(status({}))).toBe("You have 0.1.0.");
    expect(updateLine(status({ checking: true }))).toBe("Checking for updates…");
    expect(updateLine(status({ installing: true, available: "0.2.0" }))).toBe("Updating to 0.2.0…");
    expect(updateLine(status({ last_check: "2026-10-27T00:00:00Z" }))).toBe("You're up to date (0.1.0).");
    expect(updateLine(status({ available: "0.2.0" }))).toBe("Version 0.2.0 is available. You have 0.1.0.");
    expect(updateLine(status({ last_error: "offline", last_check: "2026-10-27T00:00:00Z" }))).toBe("Couldn't check for updates. You have 0.1.0.");
  });
});
