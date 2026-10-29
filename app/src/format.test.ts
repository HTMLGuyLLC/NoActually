import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ago, errorText, filterLabel, fmtDay, isFiltered, last, listNames, localIsoDate, plural, shortSourceName, subjectLabel, subjectText } from "./format";
import { meta } from "./test/fixtures";

describe("ago", () => {
  const now = new Date("2026-10-27T12:00:00Z");
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(now);
  });
  afterEach(() => vi.useRealTimers());
  const secsAgo = (secs: number) => new Date(now.getTime() - secs * 1000).toISOString();

  it("says never without a sync", () => {
    expect(ago(null)).toBe("never");
  });

  it("says just now under 45 seconds", () => {
    expect(ago(secsAgo(44))).toBe("just now");
    expect(ago(secsAgo(0))).toBe("just now");
  });

  it("counts minutes from 45 seconds up to an hour", () => {
    expect(ago(secsAgo(45))).toBe("1 min ago");
    expect(ago(secsAgo(59 * 60))).toBe("59 min ago");
  });

  it("shows the clock time after an hour", () => {
    const iso = secsAgo(3600);
    expect(ago(iso)).toBe(new Date(iso).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" }));
  });
});

describe("filterLabel", () => {
  it("names the narrowest scope picked", () => {
    expect(filterLabel({ provider: "Anthropic", family: "Claude Opus", version: "5.5" }, meta)).toBe("Claude Opus 5.5");
    expect(filterLabel({ provider: "Anthropic", family: "Claude Opus" }, meta)).toBe("Claude Opus");
    expect(filterLabel({ provider: "OpenAI" }, meta)).toBe("OpenAI");
    expect(filterLabel({}, meta)).toBe("AI");
  });

  it("names a source from meta, without the export suffix", () => {
    expect(filterLabel({ source: "claude-code" }, meta)).toBe("Claude Code");
    expect(filterLabel({ source: "chatgpt-export" }, meta)).toBe("ChatGPT");
  });

  it("never returns undefined for a source it doesn't know", () => {
    expect(filterLabel({ source: "brand-new-tool" }, meta)).toBe("brand-new-tool");
  });

  it("copes with a version but no family", () => {
    expect(filterLabel({ version: "5" }, meta)).toBe("5");
  });
});

describe("isFiltered / subjectLabel", () => {
  it("ignores null fields", () => {
    expect(isFiltered({})).toBe(false);
    expect(isFiltered({ source: null })).toBe(false);
    expect(isFiltered({ provider: "OpenAI" })).toBe(true);
  });

  it("reads All AI until a model is picked", () => {
    expect(subjectLabel({}, meta)).toBe("All AI");
    expect(subjectLabel({ provider: "OpenAI" }, meta)).toBe("OpenAI");
  });
});

describe("listNames", () => {
  it("joins names the way a sentence would", () => {
    expect(listNames([])).toBe("");
    expect(listNames(["Claude Code"])).toBe("Claude Code");
    expect(listNames(["Claude Code", "Codex"])).toBe("Claude Code and Codex");
    expect(listNames(["Claude Code", "Codex", "ChatGPT"])).toBe("Claude Code, Codex and ChatGPT");
  });
});

describe("localIsoDate", () => {
  it("uses the local calendar day, zero-padded", () => {
    expect(localIsoDate(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
    expect(localIsoDate(new Date(2026, 11, 31, 0, 0))).toBe("2026-12-31");
  });
});

describe("shortSourceName / errorText", () => {
  it("drops only a trailing export suffix", () => {
    expect(shortSourceName("Claude.ai (export)")).toBe("Claude.ai");
    expect(shortSourceName("Codex")).toBe("Codex");
  });

  it("reads Rust's string errors and JS errors alike", () => {
    expect(errorText("database is locked")).toBe("database is locked");
    expect(errorText(new Error("boom"))).toBe("boom");
  });
});

describe("last", () => {
  it("picks from the end, like at(-n)", () => {
    expect(last([1, 2, 3])).toBe(3);
    expect(last([1, 2, 3], 2)).toBe(2);
    expect(last([1], 2)).toBeUndefined();
    expect(last([])).toBeUndefined();
  });
});

describe("plural", () => {
  it("counts with the right noun", () => {
    expect(plural(1, "time")).toBe("1 time");
    expect(plural(0, "time")).toBe("0 times");
    expect(plural(1234, "task")).toBe(`${(1234).toLocaleString()} tasks`);
    expect(plural(3, "more task")).toBe("3 more tasks");
    expect(plural(2, "trophy", "trophies")).toBe("2 trophies");
  });
});

describe("fmtDay", () => {
  it("writes a day as month and day, or with the options given", () => {
    expect(fmtDay("2026-10-02")).toBe(new Date(2026, 9, 2).toLocaleDateString(undefined, { month: "short", day: "numeric" }));
    expect(fmtDay("2026-10-02", { month: "short", year: "numeric" })).toContain("2026");
  });
});

describe("subjectText", () => {
  it("says AI until a model is picked", () => {
    expect(subjectText({}, meta)).toBe("AI");
    expect(subjectText({ provider: "OpenAI" }, meta)).toBe("OpenAI");
  });
});

describe("browser support", () => {
  // The macOS 12.0 webview is Safari 15.0: no Array.prototype.at (15.4) or roundRect (16).
  it("uses no API newer than Safari 15.0 in the app's source", () => {
    const sources = import.meta.glob<string>(["./**/*.ts", "!./**/*.test.ts"], { query: "?raw", import: "default", eager: true });
    expect(Object.keys(sources).length).toBeGreaterThan(10);
    for (const [file, text] of Object.entries(sources)) {
      expect(text, `${file} uses .at(`).not.toMatch(/\.at\(/);
      expect(text, `${file} uses .roundRect(`).not.toMatch(/\.roundRect\(/);
    }
  });
});
