import { describe, expect, it } from "vitest";
import { HOSTILE, meta, overall, prompts, stats } from "../test/fixtures";
import { gradeKey, promptLines } from "./cards";
import {
  clip,
  correctionShareText,
  promptShareText,
  QUOTE_CLIP,
  quoteClip,
  SHARE_HOSTS,
  SHARE_URL,
  SHARE_URL_SHORT,
  type ShareTarget,
  shareGuide,
  shareTargets,
  shareText,
} from "./share";

describe("shareText", () => {
  it("brags with the score and its tier, numbers only, and links to the project", () => {
    const t = shareText(stats(), "Claude Opus 5.5", "Sharp-eyed");
    expect(t).toBe(
      `Could Claude Opus 5.5 replace me? No. My score is 86 (Sharp-eyed): 83% of my prompts were clear the first time. I corrected it 41 times in the last 28 days, and it corrected me 2 times. What's yours? ${SHARE_URL}`,
    );
  });

  it("handles the singular", () => {
    const s = stats({ overall: overall({ corrections: 1, humbled: 1 }) });
    expect(shareText(s, "AI", "Sharp-eyed")).toContain("I corrected it 1 time in the last 28 days, and it corrected me 1 time.");
  });

  it("follows the verdict, never re-deriving it from the score", () => {
    expect(shareText(stats({ overall: overall({ verdict: "hmm" }) }), "AI", "Sharp-eyed")).toContain("Hmm.");
    expect(shareText(stats({ overall: overall({ verdict: "no", score: 10 }) }), "AI", "Rubber stamp")).toContain("No.");
  });
});

describe("promptShareText", () => {
  it("posts the tier's answer and the landed share, numbers only", () => {
    const tier = meta.prompt_tiers[2];
    if (!tier) throw new Error("fixture");
    expect(promptShareText(prompts(), tier)).toBe(
      `Do I make my AI guess? Sometimes. 95% of my prompts in the last 28 days were clear the first time. Prompt tier: Tech lead. Do you make yours guess? ${SHARE_URL}`,
    );
  });

  it("matches the card's lines", () => {
    expect(promptLines(prompts())).toEqual(["95% of my prompts in the last 28 days were clear the first time.", "20% of all my prompts were specific."]);
    expect(promptLines(prompts(), "across all time")[0]).toBe("95% of my prompts across all time were clear the first time.");
    expect(gradeKey(prompts())).toBe("SPECIFIC 20%  ·  DECENT 30%  ·  VAGUE 50%");
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

  it("clips both quotes at QUOTE_CLIP, the same length the card image uses", () => {
    const t = correctionShareText({ model: "GPT", human: "h".repeat(300), ai: "a".repeat(300) });
    expect(t).toContain(`“${"h".repeat(QUOTE_CLIP - 1)}…”`);
    expect(t).toContain(`“${"a".repeat(QUOTE_CLIP - 1)}…”`);
  });

  it("cuts both quotes shorter when two long ones and a long model name would be over X and Bluesky", () => {
    const e = { model: "Claude Opus 5.5 (extended thinking)", human: "h".repeat(300), ai: "a".repeat(300) };
    const t = correctionShareText(e);
    expect(quoteClip(e)).toBeLessThan(QUOTE_CLIP);
    expect(t.length).toBeLessThanOrEqual(300);
    // X counts a link as 23 characters.
    expect(t.length - SHARE_URL.length + 23).toBeLessThanOrEqual(280);
    // Both shrink together, so neither side is favoured.
    const n = quoteClip(e);
    expect(t).toContain(`“${"h".repeat(n - 1)}…”`);
    expect(t).toContain(`“${"a".repeat(n - 1)}…”`);
  });

  it("keeps the full clip when the post already fits, and never goes below a floor", () => {
    expect(quoteClip({ model: "GPT", human: "h".repeat(300), ai: "a".repeat(300) })).toBe(QUOTE_CLIP);
    expect(quoteClip({ model: "m".repeat(400), human: "h".repeat(300), ai: "a".repeat(300) })).toBe(20);
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
    const tier = { id: "hawk_eyed", name: "Hawk-eyed", floor: 90, verdict: "Religiously.", blurb: "You catch AI slipping nearly every time." };
    expect(correctionsShareText({ corrections: 37, humbled: 1 }, tier)).toBe(
      "Do I correct my AI? Religiously. I corrected it 37 times in the last 28 days, and it corrected me 1 time. Correction tier: Hawk-eyed. Do you correct yours? https://github.com/HTMLGuyLLC/NoActually",
    );
    expect(correctionLines({ corrections: 1, humbled: 0, judgment: 96 }, tier, "across all time")).toEqual([
      "I corrected it 1 time across all time. It corrected me 0.",
      "Judgment 96 of 100. You catch AI slipping nearly every time.",
    ]);
    const low = { ...tier, blurb: "Most of its slips get past you." };
    expect(correctionLines({ corrections: 1, humbled: 4, judgment: 20 }, low)[1]).toBe("Judgment 20 of 100. Most of its slips get past you.");
  });
});

describe("quotes with emoji", () => {
  it("never cuts one in half, so the links can always be built", () => {
    const text = `${"a".repeat(88)}😀 and more`;
    const cut = clip(text, 90);
    expect(cut).toContain("😀");
    expect(() => shareTargets(cut)).not.toThrow();
  });

  it("drops half an emoji left by an edit", () => {
    expect(shareTargets("hi \ud83d there")[0]?.href).toContain("hi%20%20there");
  });
});
