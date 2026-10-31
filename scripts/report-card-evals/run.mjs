#!/usr/bin/env node
// Checks the report card skill against made-up chats with known answers (cases.json).
//
// Each run sends one chat, the helper's own digest of it (`noactually digest --transcript`) and the skill text to `claude -p`
// (no tools, no saved session, your settings and plugins skipped), then checks the card:
// one record block, the right seal and ids, the expected task count and grades, and a visible
// grade that matches the block. Runs use your Claude plan, so this is manual, never CI.
//
//   node scripts/report-card-evals/run.mjs [--model sonnet] [--runs 1] [--only id,id] [--no-digest]
//
// NOACTUALLY_BIN=path/to/noactually skips the cargo build.
// --no-digest: the app isn't installed; the card must have no record block and no dollar figures.

import { execFileSync } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? fallback : args[i + 1];
};
const flag = (name) => args.includes(`--${name}`);
const model = opt("model", "sonnet");
const runs = Number(opt("runs", "1"));
const only = opt("only", "")?.split(",").filter(Boolean) ?? [];
const noDigest = flag("no-digest");
const outDir = opt("out", join(tmpdir(), `report-card-evals-${Date.now()}`));
mkdirSync(outDir, { recursive: true });

const { cases } = JSON.parse(readFileSync(join(here, "cases.json"), "utf8"));
const skill = readFileSync(join(here, "../../plugins/grade/skills/me/SKILL.md"), "utf8").replace(/^---[\s\S]*?---\n/, "");

const MODEL = "claude-opus-5-5";
const at = (minute) => new Date(Date.UTC(2026, 9, 1, 10, minute)).toISOString();
const pad = (n) => String(n).padStart(8, "0");

/** The chat as a Claude Code transcript: each ask answered two minutes later, then a minute to the next. */
function transcript(c) {
  const lines = [];
  let minute = 0;
  let last = null;
  c.chat.forEach(([who, text], i) => {
    if (who === "you") {
      // A sign-off lands as the reply arrives, so it costs no time.
      minute += last === "ai" ? 1 : 0;
      lines.push({ type: "user", uuid: `u-${i}`, promptId: `p-${i}`, timestamp: at(minute), message: { content: text } });
    } else {
      minute += 2;
      lines.push({
        type: "assistant",
        uuid: `a-${i}`,
        timestamp: at(minute),
        message: {
          id: `m-${i}`,
          model: MODEL,
          content: [{ type: "text", text }],
          usage: { input_tokens: 10_000, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: 5_000 },
        },
      });
    }
    last = who;
  });
  return lines.map((l) => JSON.stringify(l)).join("\n") + "\n";
}

/**
 * The digest the real helper (`noactually digest --transcript`) makes for the chat, run against a
 * throwaway home so no real data is read and no seal key is left behind. `bin` is a prebuilt
 * `noactually` (NOACTUALLY_BIN); without it cargo builds and runs it.
 */
function realDigest(c) {
  const home = mkdtempSync(join(tmpdir(), "report-card-evals-home-"));
  const file = join(home, "projects/proj", `${randomUUID()}.jsonl`);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, transcript(c));
  const bin = process.env.NOACTUALLY_BIN;
  const [cmd, pre] = bin ? [bin, []] : ["cargo", ["run", "-q", "-p", "noactually-cli", "--"]];
  const env = { ...process.env, NOACTUALLY_DB: join(home, "noactually.db"), NOACTUALLY_CLAUDE_PROJECTS: join(home, "projects") };
  try {
    return execFileSync(cmd, [...pre, "digest", "--transcript", file, "--source", "claude-code"], {
      env,
      encoding: "utf8",
      cwd: join(here, "../.."),
    }).trimEnd();
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
}

/** The chat as the AI saw it. */
const chatText = (c) =>
  c.chat.map(([who, text]) => `${who === "you" ? "Person" : "You (the AI)"}: ${text}`).join("\n\n");

/** The helper's digest for the chat, with the seal and prompt ids it holds. */
function render(c) {
  // A case can hand-write its digest (`digest`, with {seal}) for a summary the helper would never make.
  const digest = c.digest ? c.digest.replace("{seal}", randomBytes(4).toString("hex")) : realDigest(c);
  const seal = digest.match(/^seal (\S+)/m)?.[1];
  if (!seal) throw new Error(`no digest for ${c.id}`);
  const ids = [...digest.matchAll(/^ {2}(\S+) "/gm)].map((m) => ({ id: m[1] }));
  return { seal, prompts: ids, digest };
}

function prompt(c) {
  // With --no-digest the app isn't installed, so nothing is built or run.
  const { seal, prompts, digest } = noDigest ? { seal: "", prompts: [], digest: "" } : render(c);
  const chat = chatText(c);
  let body = skill.replace(/^!`.*`$/m, digest);
  body = body.replace("$ARGUMENTS", "");
  return {
    seal,
    prompts,
    text: `Here is this session so far.\n\n${chat}\n\n---\n\nThe person typed: /grade:me\n\nThe grade:me skill loaded:\n\n${body}`,
  };
}

const SYSTEM =
  "You are Claude Code, an AI coding assistant. You've been working with this person in this session; the conversation so far is shown in the message. Follow the skill the person invoked.";

function run(c) {
  const { seal, prompts, text } = prompt(c);
  const env = { HOME: process.env.HOME, PATH: process.env.PATH, USER: process.env.USER, TERM: "dumb" };
  const raw = execFileSync(
    "claude",
    ["-p", text, "--model", model, "--tools", "", "--system-prompt", SYSTEM, "--no-session-persistence", "--setting-sources", "", "--output-format", "json"],
    { env, encoding: "utf8", maxBuffer: 16 << 20, timeout: 300_000 },
  );
  const out = JSON.parse(raw);
  return { seal, prompts, out, card: out.result ?? "" };
}

/** Problems with one card, or [] when it's right. */
function check(c, { seal, prompts, card }) {
  const bad = [];
  const blocks = [...card.matchAll(/^\s*```noactually\s*\n([\s\S]*?)```/gm)].map((m) => m[1].trim());
  if (noDigest) {
    if (blocks.length) bad.push("record block without a summary");
    if (/\$\d/.test(card)) bad.push("dollar figures without a summary");
    return { bad, grades: [] };
  }
  if (c.expect.tasks === 0) {
    if (blocks.length) bad.push("record block for a chat with no tasks");
    if (/Grade:\s*\**[A-F]\b/.test(card)) bad.push("a grade for a chat with no tasks");
    return { bad, grades: [] };
  }
  if (blocks.length !== 1) return { bad: [`${blocks.length} record blocks`], grades: [] };
  let rec;
  try {
    rec = JSON.parse(blocks[0]);
  } catch {
    return { bad: ["record block isn't JSON"], grades: [] };
  }
  if (rec.v !== 1) bad.push(`v=${rec.v}`);
  if (rec.k !== seal) bad.push(`seal ${rec.k} != ${seal}`);
  const ids = new Set(prompts.map((p) => p.id));
  const tasks = rec.tasks ?? [];
  const wantTasks = c.expect.tasks;
  if (tasks.length !== wantTasks) bad.push(`${tasks.length} tasks, want ${wantTasks}`);
  const grades = tasks.map((t) => {
    for (const x of [t.at, t.to, ...(t.waste ?? []).map((w) => w[0])]) if (!ids.has(x)) bad.push(`unknown id ${x}`);
    for (const w of t.waste ?? []) if (![0, 25, 50, 75, 100].includes(w[1])) bad.push(`share ${w[1]}`);
    const n = (t.waste ?? []).filter((w) => w[1] > 0).length;
    const byCount = ["A", "B", "C", "D"][n] ?? "F";
    // A goal that was never said is a D at best (engine::LetterGrade::of).
    return t.wrong && byCount < "D" ? "D" : byCount;
  });
  const want = c.expect.grades;
  grades.forEach((g, i) => {
    if (want[i] && !want[i].includes(g)) bad.push(`task ${i + 1} graded ${g}, want ${want[i].join("/")}`);
  });
  const shown = [...card.matchAll(/Grade:\s*\**([A-F])\b/g)].map((m) => m[1]);
  if (shown.length && shown.join() !== grades.join()) bad.push(`visible grade ${shown.join()} != record ${grades.join()}`);
  if (c.expect.miss_any && !tasks.some((t) => (t.miss ?? []).some((m) => c.expect.miss_any.includes(m)))) {
    bad.push(`missing habits ${JSON.stringify(tasks.map((t) => t.miss))}, want one of ${c.expect.miss_any.join("/")}`);
  }
  const LABEL = "For No, Actually... (you can ignore this):";
  if (!card.includes(LABEL)) bad.push("no label line before the record");
  const prose = card.replace(/```[\s\S]*?```/g, "").replace(LABEL, "");
  for (const line of prose.split("\n")) if (/^\s*For\b/.test(line)) bad.push(`stray line: ${line.trim().slice(0, 40)}`);
  const leak = prose.match(/\b(record|summary|seal|digest)\b/i);
  if (leak) bad.push(`internal word "${leak[1]}" shown`);
  const limit = 300 * wantTasks;
  const count = prose.split(/\s+/).filter(Boolean).length;
  if (count > limit) bad.push(`${count} words, limit ${limit}`);
  return { bad, grades };
}

const picked = cases.filter((c) => !only.length || only.includes(c.id));
let failed = 0;
const rows = [];
for (const c of picked) {
  for (let r = 1; r <= runs; r++) {
    let result;
    try {
      result = run(c);
    } catch (e) {
      failed++;
      rows.push([c.id, r, "ERROR", String(e.message).split("\n")[0]]);
      continue;
    }
    const { bad, grades } = check(c, result);
    const u = result.out.usage ?? {};
    writeFileSync(join(outDir, `${c.id}-${r}.md`), `${result.card}\n\n<!-- usage ${JSON.stringify(u)} -->\n`);
    if (bad.length) failed++;
    rows.push([c.id, r, bad.length ? "FAIL" : "ok", `${grades.join(",")} out=${u.output_tokens ?? "?"} ${bad.join("; ")}`]);
    console.log(rows.at(-1).join("  "));
  }
}
console.log(`\n${picked.length * runs - failed}/${picked.length * runs} passed (${model}${noDigest ? ", no summary" : ""}). Cards: ${outDir}`);
process.exitCode = failed ? 1 : 0;
