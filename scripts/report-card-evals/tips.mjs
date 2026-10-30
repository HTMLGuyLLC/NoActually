#!/usr/bin/env node
// Checks tips with real agents: real tools, the made-up repo in fixture/, the rules from the real
// plugin's session-start hook, and the made-up chats in tips-cases.json. Each run scores fair tips
// (a fresh model judges each one), follow-ups that got a tip, clean chats that didn't, and tips that
// leaked into files, commits or tool calls. Runs use your Claude plan, so this is manual, never CI.
// Build the helper first: cargo build -p noactually-cli
//
//   node scripts/report-card-evals/tips.mjs [--models sonnet,opus] [--runs 2] [--only id,id] [--jobs 5] [--no-judge]
//   node scripts/report-card-evals/tips.mjs --compare [--models sonnet] [--runs 3]
//
// --compare: the same chats with and without the rules, for what the rules cost in tokens.

import { execFile, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "../..");
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? fallback : args[i + 1];
};
const flag = (name) => args.includes(`--${name}`);
const compare = flag("compare");
const models = opt("models", compare ? "sonnet" : "sonnet,opus").split(",");
const runs = Number(opt("runs", compare ? "3" : "2"));
const jobs = Number(opt("jobs", "5"));
const only = opt("only", "").split(",").filter(Boolean);
const COMPARED = ["clean", "known-pref", "changed-mind", "finish-late"];

const TIP = /^\W*Tip from No, Actually(\.\.\.|…):.*\[(target|evidence|finish|guardrails|purpose|context), (redid (25|50|75|100)%|had to ask)\]\W*$/;
const MARK = /Tip from No, Actually/;
const TOOLS = "Read,Edit,Write,Grep,Glob";

const helper = join(repo, "target/debug/noactually");
if (!existsSync(helper)) throw new Error("Build the helper first: cargo build -p noactually-cli");
const root = mkdtempSync(join(realpathSync(tmpdir()), "tip-evals-"));
const data = join(root, "data");
const plugin = join(root, "plugin");
mkdirSync(data);
writeFileSync(join(data, "report-cards-tips"), "");
cpSync(join(repo, "plugins/grade"), plugin, { recursive: true });
writeFileSync(join(plugin, "bin/na"), `#!/bin/sh\nexec "${helper}" "$@"\n`);
chmodSync(join(plugin, "bin/na"), 0o755);
const env = { ...process.env, NOACTUALLY_DB: join(data, "noactually.db"), CLAUDE_CODE_ENTRYPOINT: "claude-desktop" };

const projectDir = (cwd) => join(homedir(), ".claude/projects", cwd.replace(/[^a-zA-Z0-9]/g, "-"));
const rulesText = readFileSync(join(repo, "crates/engine/src/tip_rules.txt"), "utf8").trim();

/** A saved chat to resume, so the AI's earlier work is fixed: the rules first, as the hook leaves them. */
function savedChat(c, cwd, withRules) {
  const session = randomUUID();
  const lines = [];
  let parent = null;
  let minute = 0;
  const base = { sessionId: session, cwd, version: "2.1.314", gitBranch: "", userType: "external", isSidechain: false };
  const add = (rec) => {
    const uuid = randomUUID();
    const timestamp = new Date(Date.UTC(2026, 9, 1, 9, minute++)).toISOString();
    lines.push(JSON.stringify({ ...base, ...rec, uuid, parentUuid: parent, timestamp }));
    parent = uuid;
  };
  if (withRules) add({ type: "attachment", attachment: { type: "hook_additional_context", content: [rulesText], hookName: "SessionStart", toolUseID: "SessionStart", hookEvent: "SessionStart" } });
  const pairs = [];
  for (let n = 1; n <= (c.filler ?? 0); n++) pairs.push([`Add a constant LIMIT_${n} = ${n * 10} to src/limits.js.`, `Added LIMIT_${n} to src/limits.js.`]);
  for (const [you, ai] of [...pairs, ...(c.history ?? [])]) {
    add({ type: "user", message: { role: "user", content: you } });
    add({ type: "assistant", message: { id: `msg_${randomUUID()}`, type: "message", role: "assistant", model: "claude-sonnet-5-5", content: [{ type: "text", text: ai }], stop_reason: "end_turn", usage: { input_tokens: 10, output_tokens: 10 } } });
  }
  const dir = projectDir(cwd);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${session}.jsonl`), `${lines.join("\n")}\n`);
  return session;
}

const walkFiles = (dir) =>
  readdirSync(dir).flatMap((f) => {
    if (f === ".git") return [];
    const p = join(dir, f);
    return statSync(p).isDirectory() ? walkFiles(p) : [p];
  });

function chatRun(model, c, n, rules) {
  return new Promise((done) => {
    const cwd = join(root, "work", `${model}-${c.id}-${n}-${rules ? "on" : "off"}`);
    mkdirSync(dirname(cwd), { recursive: true });
    cpSync(join(here, "fixture"), cwd, { recursive: true });
    for (const [f, text] of Object.entries(c.files ?? {})) {
      mkdirSync(dirname(join(cwd, f)), { recursive: true });
      writeFileSync(join(cwd, f), text);
    }
    const out = { model, id: c.id, n, expect: c.expect, long: !!c.long, habits: c.habits, lang: c.lang, turns: [], leaks: [] };
    const start = async () => {
      if (c.git) {
        for (const a of [["init", "-q"], ["add", "-A"], ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", "start"]]) await run("git", a, { cwd });
      }
      const pluginArgs = rules ? ["--plugin-dir", plugin] : [];
      const base = ["--model", model, "--setting-sources=", ...pluginArgs];
      const saved = c.history || c.filler ? savedChat(c, cwd, rules) : null;
      if (c.compact) await run("claude", ["-p", ...base, "--resume", saved, "/compact"], { cwd, env, maxBuffer: 1 << 26 });
      const tools = c.git ? ["--tools", `${TOOLS},Bash`, "--allowedTools", "Bash(git:*)"] : ["--tools", TOOLS];
      const p = spawn("claude", ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", ...base, ...tools, "--permission-mode", "acceptEdits", ...(saved ? ["--resume", saved] : ["--no-session-persistence"])], { cwd, env, stdio: ["pipe", "pipe", "ignore"] });
      const send = (t) => p.stdin.write(`${JSON.stringify({ type: "user", message: { role: "user", content: t } })}\n`);
      const timer = setTimeout(() => p.kill(), 420000);
      let buf = "";
      let turn = 0;
      let texts = [];
      let tools_used = 0;
      send(c.turns[0]);
      p.stdout.on("data", (d) => {
        buf += d;
        for (let i = buf.indexOf("\n"); i >= 0; i = buf.indexOf("\n")) {
          const line = buf.slice(0, i);
          buf = buf.slice(i + 1);
          let e;
          try {
            e = JSON.parse(line);
          } catch {
            continue;
          }
          if (e.type === "assistant")
            for (const b of e.message?.content ?? []) {
              if (b.type === "text") texts.push(b.text);
              if (b.type === "tool_use") {
                tools_used++;
                if (MARK.test(JSON.stringify(b.input ?? {}))) out.leaks.push(`tool ${b.name}`);
              }
            }
          if (e.type === "result") {
            const last = (texts.at(-1) ?? "").trim().split("\n").at(-1) ?? "";
            const u = e.usage ?? {};
            out.turns.push({
              you: c.turns[turn],
              ai: texts.join("\n").replace(last, TIP.test(last) ? "" : last).trim().slice(0, 1500),
              tools: tools_used,
              tip: TIP.test(last) ? last.trim() : null,
              stray: texts.some((t) => MARK.test(t)) && !TIP.test(last),
              midTip: texts.slice(0, -1).some((t) => MARK.test(t)),
              tokens: (u.input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.output_tokens ?? 0),
            });
            texts = [];
            tools_used = 0;
            turn++;
            if (turn < c.turns.length) send(c.turns[turn]);
            else p.stdin.end();
          }
        }
      });
      p.on("close", async () => {
        clearTimeout(timer);
        for (const f of walkFiles(cwd)) if (MARK.test(readFileSync(f, "utf8"))) out.leaks.push(`file ${f.slice(cwd.length + 1)}`);
        if (c.git) {
          const log = await run("git", ["log", "--format=%B"], { cwd }).then((r) => r.stdout, () => "");
          out.commits = log.split("\n").filter(Boolean).length;
          if (MARK.test(log)) out.leaks.push("commit message");
        }
        if (rules && !out.turns.length) out.failed = true;
        const project = projectDir(cwd);
        if (project.includes("tip-evals-")) rmSync(project, { recursive: true, force: true });
        rmSync(cwd, { recursive: true, force: true });
        done(out);
      });
    };
    start().catch((e) => done({ ...out, failed: String(e) }));
  });
}

async function pool(items, work) {
  const results = [];
  let next = 0;
  await Promise.all(
    Array.from({ length: jobs }, async () => {
      while (next < items.length) {
        const i = next++;
        results[i] = await work(items[i]);
        process.stderr.write(".");
      }
    }),
  );
  process.stderr.write("\n");
  return results;
}

const JUDGE = `You review one-line coaching tips that an AI coding assistant gave a person. The tip's own line was removed from the chat below, and it's shown separately. A tip is fair only if all of these hold:
1. The person's follow-up told the AI something they already knew when the goal began.
2. The AI could not have found it by looking at the project, and it wasn't the AI's own mistake on a clear request.
3. The bracket tells the truth. "redid N%" means the follow-up made the AI redo real work, not only add to it. "had to ask" means the AI had changed nothing and had to stop and ask for what the person could have said.
4. The tip is kind, specific to this chat, and about the asking, not the person.
Reply with JSON only: {"fair":true or false,"why":"one short sentence"}.`;

async function judge(r) {
  const at = r.turns.findIndex((t) => t.tip);
  const chat = r.turns.slice(0, at + 1).map((t) => `PERSON: ${t.you}\nAI (${t.tools} tool calls): ${t.ai}`).join("\n\n");
  const prompt = `${chat}\n\nTIP GIVEN: ${r.turns[at].tip}`;
  const { stdout } = await run("claude", ["-p", "--model", "opus", "--tools", "", "--setting-sources=", "--no-session-persistence", "--system-prompt", JUDGE, "--output-format", "json", prompt], { maxBuffer: 1 << 24 });
  const text = JSON.parse(stdout).result ?? "";
  try {
    return JSON.parse(text.match(/\{[\s\S]*\}/)[0]);
  } catch {
    return { fair: false, why: `unreadable: ${text.slice(0, 80)}` };
  }
}

const { cases } = JSON.parse(readFileSync(join(here, "tips-cases.json"), "utf8"));
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

async function main() {
  if (compare) {
    const picked = cases.filter((c) => COMPARED.includes(c.id));
    const todo = models.flatMap((m) => picked.flatMap((c) => Array.from({ length: runs }, (_, n) => [true, false].map((on) => [m, c, n + 1, on])).flat()));
    const done = await pool(todo, ([m, c, n, on]) => chatRun(m, c, n, on).then((r) => ({ ...r, on })));
    let total = [0, 0];
    for (const c of picked) {
      const of = (on) => median(done.filter((r) => r.id === c.id && r.on === on).map((r) => r.turns.reduce((s, t) => s + t.tokens, 0)));
      const [a, b] = [of(true), of(false)];
      total = [total[0] + a, total[1] + b];
      console.log(`${c.id}: with ${a}, without ${b} (${(((a - b) / b) * 100).toFixed(1)}%)`);
    }
    console.log(`all: with ${total[0]}, without ${total[1]} (${(((total[0] - total[1]) / total[1]) * 100).toFixed(1)}%)`);
    console.log(`the rules are ${rulesText.split(/\s+/).length} words`);
    return;
  }
  const picked = cases.filter((c) => !only.length || only.includes(c.id));
  const todo = models.flatMap((m) => picked.flatMap((c) => Array.from({ length: runs }, (_, n) => [m, c, n + 1])));
  const results = await pool(todo, ([m, c, n]) => chatRun(m, c, n, true));
  for (const [r, [, c]] of results.map((r, i) => [r, todo[i]])) {
    const want = c.tipTurn ?? 1;
    r.tips = r.turns.filter((t) => t.tip).length;
    r.onTime = !!r.turns[want]?.tip;
    r.early = r.turns.slice(0, c.tipTurn === undefined ? 1 : 0).some((t) => t.tip);
    r.habit = r.turns.find((t) => t.tip)?.tip.match(/\[(\w+),/)?.[1];
  }
  if (!flag("no-judge")) {
    const tipped = results.filter((r) => r.tips);
    const verdicts = await pool(tipped, judge);
    tipped.forEach((r, i) => {
      r.verdict = verdicts[i];
    });
  }
  const file = join(tmpdir(), `tip-evals-${Date.now()}.json`);
  writeFileSync(file, JSON.stringify(results, null, 1));
  rmSync(root, { recursive: true, force: true });

  for (const r of results) {
    const good = r.failed ? false : r.expect === "tip" ? r.onTime && r.tips === 1 : r.tips === 0;
    const notes = [r.failed && "FAILED RUN", r.early && "early", r.turns.some((t) => t.stray) && "stray", r.turns.some((t) => t.midTip) && "mid-reply", r.leaks.length && `LEAK ${r.leaks}`, r.verdict && !r.verdict.fair && `unfair: ${r.verdict.why}`, r.habit && r.habits && !r.habits.includes(r.habit) && `habit ${r.habit}`].filter(Boolean);
    if (!good || notes.length) console.log(`${good ? "ok  " : "MISS"} ${r.model} ${r.id}#${r.n} tips=${r.tips} ${notes.join(" | ")}`);
  }
  const tipped = results.filter((r) => r.verdict);
  const pct = (n, of) => (of ? `${Math.round((n / of) * 100)}% (${n}/${of})` : "n/a");
  const tipWanted = results.filter((r) => r.expect === "tip");
  const clean = results.filter((r) => r.expect === "none");
  const fair = tipped.filter((r) => r.verdict.fair).length;
  const got = (rs) => rs.filter((r) => r.onTime).length;
  console.log(`\nFair tips (90%+):            ${pct(fair, tipped.length)}`);
  console.log(`Follow-ups with a tip (70%+): ${pct(got(tipWanted), tipWanted.length)}`);
  console.log(`  short chats:                ${pct(got(tipWanted.filter((r) => !r.long)), tipWanted.filter((r) => !r.long).length)}`);
  console.log(`  long chats:                 ${pct(got(tipWanted.filter((r) => r.long)), tipWanted.filter((r) => r.long).length)}`);
  console.log(`Clean chats with a tip (<5%): ${pct(clean.filter((r) => r.tips).length, clean.length)}`);
  console.log(`Leaks (zero):                 ${results.reduce((s, r) => s + r.leaks.length, 0)}`);
  console.log(`Tips before a follow-up:      ${results.filter((r) => r.early).length}`);
  console.log(`Runs that failed:             ${results.filter((r) => r.failed).length}`);
  for (const m of models) {
    const of = results.filter((r) => r.model === m);
    console.log(`${m}: follow-ups ${pct(got(of.filter((r) => r.expect === "tip")), of.filter((r) => r.expect === "tip").length)}, clean with a tip ${pct(of.filter((r) => r.expect === "none" && r.tips).length, of.filter((r) => r.expect === "none").length)}`);
  }
  console.log(`\nDetails: ${file}`);
}

main().finally(() => rmSync(root, { recursive: true, force: true }));
