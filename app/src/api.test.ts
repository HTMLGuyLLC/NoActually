import { describe, expect, it, vi } from "vitest";
import { api, demo, isLevel } from "./api";
import { meta, progress, stats } from "./test/fixtures";

describe("api outside Tauri", () => {
  it("runs as the demo, answering every command", async () => {
    expect(demo).toBe(true);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ json: async () => ({ meta, stats: stats(), progress: progress() }) })),
    );
    const open = vi.fn();
    vi.stubGlobal("window", { open });
    expect(await api.meta()).toEqual(meta);
    expect((await api.stats({})).overall.tier).toBe("sharp_eyed");
    expect(await api.sources()).not.toHaveLength(0);
    expect(await api.retention()).toEqual({ level: "snippet", full_days: 30, snippet_days: 365 });
    expect(await api.getAutostart()).toBe(false);
    expect((await api.updateStatus()).current).toBe("0.1.0");
    expect((await api.syncStatus()).last_error).toBeNull();
    const writes = [
      api.allow("codex", null, true),
      api.revoke("codex"),
      api.forget("codex"),
      api.pause("codex", true),
      api.importExport("chatgpt-export"),
      api.setRetention({ level: "full", full_days: 1, snippet_days: 1 }),
      api.pin(1, true),
      api.syncNow(),
      api.setAutostart(true),
      api.saveShareCard([1, 2, 3]),
      api.openMain(),
      api.setAutoUpdate(false),
      api.checkForUpdates(),
      api.installUpdate(),
    ];
    for (const w of await Promise.all(writes)) expect(w).toBeNull();
    await api.openShare("https://x.com/intent/post?text=hi");
    expect(open).toHaveBeenCalledWith("https://x.com/intent/post?text=hi", "_blank", "noopener");
    vi.unstubAllGlobals();
  });
});

describe("isLevel", () => {
  it("accepts only the three privacy levels", () => {
    expect(["full", "snippet", "category"].every(isLevel)).toBe(true);
    expect(isLevel("everything")).toBe(false);
    expect(isLevel(null)).toBe(false);
  });
});
