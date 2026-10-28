import { describe, expect, it } from "vitest";
import type { Stats } from "./api";
import { SHARE_HOSTS, SHARE_URL, shareTargets, shareText } from "./share";

const stats = (over: Partial<Stats>): Stats =>
  ({ catches: 177, humbled: 3, catches_last_7d: 17, pct_real_chats_with_catch: 37.6, tier: "Irreplaceable", ...over }) as Stats;

describe("shareText", () => {
  it("brags with numbers only and links to the project", () => {
    const t = shareText(stats({}), "Claude Opus 5.5");
    expect(t).toBe(`Could Claude Opus 5.5 replace me? No. I caught it 17 times this week, and in 38% of my chats. Me 177, Claude Opus 5.5 3. What's your score? ${SHARE_URL}`);
  });

  it("hedges on a quiet week and handles the singular", () => {
    const t = shareText(stats({ catches_last_7d: 1, pct_real_chats_with_catch: null }), "AI");
    expect(t).toContain("Maybe… I caught it 1 time this week.");
  });
});

describe("shareTargets", () => {
  const text = shareText(stats({}), "AI");
  const targets = shareTargets(text);
  const by = (id: string) => targets.find((t) => t.id === id)!;

  it("pre-fills the post where the network allows it", () => {
    const x = new URL(by("x").href!);
    expect(x.searchParams.get("text")).toBe(text);
    expect(new URL(by("bluesky").href!).searchParams.get("text")).toBe(text);
    expect(new URL(by("linkedin").href!).searchParams.get("text")).toBe(text);
  });

  it("shares the link and copies the text where the network won't take text", () => {
    const fb = by("facebook");
    expect(new URL(fb.href!).searchParams.get("u")).toBe(SHARE_URL);
    expect(fb.pasteText).toBe(true);
    const reddit = new URL(by("reddit").href!);
    expect(reddit.searchParams.get("url")).toBe(SHARE_URL);
    expect(reddit.searchParams.get("title")).not.toContain("https://");
  });

  it("has no web link for Instagram, so it saves the card instead", () => {
    expect(by("instagram").href).toBeNull();
  });

  it("only ever points at allowed hosts", () => {
    for (const t of targets) if (t.href) expect(SHARE_HOSTS).toContain(new URL(t.href).host);
  });
});
