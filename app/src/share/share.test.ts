import { describe, expect, it } from "vitest";
import { HOSTILE, meta, overall, prompts, stats } from "../test/fixtures";
import { gradeKey, promptLines } from "./cards";
import {
  clip,
  correctionShareText,
  promptShareText,
  SHARE_HOSTS,
  SHARE_URL,
  SHARE_URL_SHORT,
  type ShareTarget,
  shareGuide,
  shareTargets,
  shareText,
} from "./share";

describe("shareText", () => {
  const unranked = { score: null, direction: null, judgment: null, tier: null, next_tier: null, verdict: "hmm" as const };

  it("brags with the score and its tier, numbers only, and links to the project", () => {
    const t = shareText(stats(), "Claude Opus 5.5", "Sharp-eyed");
    expect(t).toBe(
      `Could Claude Opus 5.5 replace me? No. My score is 86 (Sharp-eyed): 83% of my tasks landed without steering, and I corrected it 41 times to its 2 in the last 28 days. What's yours? ${SHARE_URL}`,
    );
  });

  it("falls back to this week's corrections before there is a score, and handles the singular", () => {
    const s = stats({ overall: overall(unranked), corrections_last_7d: 1 });
    expect(shareText(s, "AI", "Not scored yet")).toBe(
      `Could AI replace me? Maybe… I corrected it 1 time this week. Me 177, AI 3. What's your score? ${SHARE_URL}`,
    );
    expect(shareText(stats({ overall: overall(unranked), corrections_last_7d: 17 }), "AI", "")).toContain("I corrected it 17 times this week.");
  });

  it("follows the verdict, never re-deriving it from the score", () => {
    expect(shareText(stats({ overall: overall({ verdict: "hmm" }) }), "AI", "Sharp-eyed")).toContain("Maybe…");
    expect(shareText(stats({ overall: overall({ verdict: "no", score: 10 }) }), "AI", "Rubber stamp")).toContain("No.");
  });
});

describe("promptShareText", () => {
  it("posts the tier's answer and the landed share, numbers only", () => {
    const tier = meta.prompt_tiers[2];
    if (!tier) throw new Error("fixture");
    expect(promptShareText(prompts(), tier)).toBe(
      `Do I make my AI guess? Sometimes. 95% of my tasks in the last 28 days landed without me adding, clarifying or starting over. Prompt tier: Tech lead. Do you make yours guess? ${SHARE_URL}`,
    );
  });

  it("matches the card's lines", () => {
    expect(promptLines(prompts())).toEqual(["95% of my tasks in the last 28 days landed without steering.", "20% of all my prompts were sharp."]);
    expect(promptLines(prompts(), "ever")[0]).toBe("95% of my tasks ever landed without steering.");
    expect(gradeKey(prompts())).toBe("SHARP 20%  ·  SOLID 30%  ·  THIN 50%");
  });
});

describe("shareTargets", () => {
  const text = shareText(stats(), "AI", "Sharp-eyed");
  const targets = shareTargets(text);
  const by = (id: string): ShareTarget => {
    const t = targets.find((x) => x.id === id);
    if (!t) throw new Error(`no target ${id}`);
    return t;
  };
  const href = (id: string) => new URL(by(id).href ?? "about:blank");

  it("pre-fills the post where the network allows it", () => {
    expect(href("x").searchParams.get("text")).toBe(text);
    expect(href("bluesky").searchParams.get("text")).toBe(text);
    expect(href("linkedin").searchParams.get("text")).toBe(text);
  });

  it("shares the link and copies the text where the network won't take text", () => {
    expect(href("facebook").searchParams.get("u")).toBe(SHARE_URL);
    expect(by("facebook").pasteText).toBe(true);
    expect(href("reddit").searchParams.get("url")).toBe(SHARE_URL);
    expect(href("reddit").searchParams.get("title")).not.toContain("https://");
  });

  it("has no web link for Instagram, so it saves the card instead", () => {
    expect(by("instagram").href).toBeNull();
  });

  it("asks for the tall card only where the network shows it best (Instagram's 4:5 feed)", () => {
    expect(by("instagram").card).toBe("portrait");
    for (const t of targets) if (t.id !== "instagram") expect(t.card).toBeUndefined();
  });

  it("only ever points at allowed hosts", () => {
    for (const t of targets) if (t.href) expect(SHARE_HOSTS).toContain(new URL(t.href).host);
  });

  it("prints the project link without the scheme on cards", () => {
    expect(SHARE_URL_SHORT).toBe("github.com/HTMLGuyLLC/NoActually");
  });
});

describe("shareGuide", () => {
  const target = (over: Partial<ShareTarget>): ShareTarget => ({ id: "x", label: "X", href: "https://x.com/", ...over });

  it("just opens a network that takes the text", () => {
    const g = shareGuide(target({}), true, "⌘V");
    expect(g).toMatchObject({ saveImage: false, open: true, paste: false });
    expect(g.guide.value).toBe("Opening X with your post filled in…");
  });

  it("tells the person to paste where text can't be pre-filled", () => {
    const g = shareGuide(target({ label: "Facebook", pasteText: true }), true, "⌘V");
    expect(g).toMatchObject({ saveImage: false, open: true, paste: true });
    expect(g.guide.value).toContain("Your text is copied. When Facebook opens");
    expect(g.guide.value).toContain("<kbd>⌘V</kbd>");
  });

  it("asks for a manual copy when the clipboard refused", () => {
    const g = shareGuide(target({ pasteText: true }), false, "Ctrl+V");
    expect(g.guide.value).toContain("Copy the text above, then paste it into the post.");
  });

  it("saves the card first for Instagram, mentioning a manual copy only when needed", () => {
    const copied = shareGuide(target({ label: "Instagram", href: null, pasteText: true }), true, "⌘V");
    expect(copied).toMatchObject({ saveImage: true, open: false, paste: true });
    expect(copied.guide.value).not.toContain("copy it above first");
    expect(shareGuide(target({ href: null }), false, "⌘V").guide.value).toContain("(copy it above first)");
  });

  it("escapes the network label", () => {
    expect(shareGuide(target({ label: HOSTILE }), true, "⌘V").guide.value).not.toContain("<img");
  });
});

describe("correctionShareText", () => {
  it("quotes both sides, clipped, and links the project", () => {
    const t = correctionShareText({ model: "Claude Opus 5", human: "that's not a real page", ai: "You're right, that route doesn't exist." });
    expect(t).toBe(
      `I corrected Claude Opus 5. Me: “that's not a real page” Claude Opus 5: “You're right, that route doesn't exist.” Could AI replace me? No, actually. ${SHARE_URL}`,
    );
    const long = correctionShareText({ model: "GPT", human: "x".repeat(300), ai: null });
    expect(long).toContain("…”");
    expect(long.length).toBeLessThan(220);
  });

  it("works when only the category was kept", () => {
    expect(correctionShareText({ model: "", human: null, ai: null })).toBe(`I corrected my AI. Could AI replace me? No, actually. ${SHARE_URL}`);
  });
});

describe("clip", () => {
  it("collapses whitespace and cuts with an ellipsis", () => {
    expect(clip("  a \n b  ", 10)).toBe("a b");
    expect(clip("abcdefghij", 5)).toBe("abcd…");
  });
});

describe("corrections share", () => {
  it("posts the correction tier's word and who corrected whom, numbers only", async () => {
    const { correctionsShareText } = await import("./share");
    const { correctionLines } = await import("./cards");
    const tier = { id: "hawk_eyed", name: "Hawk-eyed", floor: 90, verdict: "Usually." };
    expect(correctionsShareText({ corrections: 37, humbled: 1 }, tier)).toBe(
      "Do I correct my AI? Usually. I corrected it 37 times in the last 28 days, and it corrected me 1 time. Correction tier: Hawk-eyed. Do you correct yours? https://github.com/HTMLGuyLLC/NoActually",
    );
    expect(correctionLines({ corrections: 1, humbled: 0, judgment: 96 }, "ever")).toEqual([
      "I corrected it 1 time ever. It corrected me 0.",
      "Judgment 96 of 100: I check its work, and it rarely has to correct me.",
    ]);
  });
});
