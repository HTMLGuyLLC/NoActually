import "./fonts";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, demo, type Filter, type HallEntry, type Level, type SourceInfo, type Stats, type SyncStatus } from "./api";
import { buildPickItems, matchPickItems, modelName, sameFilter, type PickItem } from "./picker";
import { catchShareText, drawCatchCard, drawShareCard, shareTargets, shareText, verdictIsNo } from "./share";

const TIERS: [string, string][] = [
  ["Irreplaceable", "15+ catches/wk"],
  ["Sharp-eyed", "7–14"],
  ["Coworker", "3–6"],
  ["Rubber stamp", "0–2"],
];
const CATEGORY_TITLES: Record<string, string> = {
  just_wrong: "Flat-out wrong",
  still_broken: "Still broken",
  ignored_instructions: "Ignored instructions",
  missed_something: "Missed something",
  made_it_up: "Made it up",
  misread: "Misread you",
};
const SOURCE_TITLES: Record<string, string> = {
  "claude-code": "Claude Code",
  "claude-cowork": "Claude Cowork",
  codex: "Codex",
  "chatgpt-export": "ChatGPT",
  "claude-ai-export": "Claude.ai",
};
type Tab = "score" | "sources" | "settings";

/** The app icon's check: the logo's only color. */
const MARK = `<svg class="mark" viewBox="0 0 44 44" aria-hidden="true"><path class="mark-check" d="M9 24 L18 33 L36 12"/></svg>`;
const WORDMARK = (cls = "brand") => `<span class="${cls}">${MARK}<span>No, Actually...</span></span>`;

const root = document.getElementById("app")!;
const isGlance = location.hash === "#glance";
let tab: Tab = "score";
let filter: Filter = {};
let allModels: Stats["models"] = [];
let status: SyncStatus = { syncing: false, last_sync: null, last_error: null, files_with_errors: 0 };
let current: Stats | null = null;

/** Every string from chats goes through here before it touches innerHTML. */
const esc = (s: unknown) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const num = (n: number) => n.toLocaleString();
const filterLabel = () =>
  filter.version ? `${filter.family} ${filter.version}` : filter.family ?? filter.provider ?? (filter.source ? SOURCE_TITLES[filter.source] : "AI");
const subjectLabel = () => (Object.keys(filter).length ? filterLabel() : "All AI");

function ago(iso: string | null) {
  if (!iso) return "never";
  const s = Math.round((Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  return new Date(iso).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

function say(el: HTMLElement | null, text: string) {
  if (el) el.textContent = text;
}

async function copyText(text: string, out: HTMLElement | null) {
  try {
    await navigator.clipboard.writeText(text);
    say(out, "Copied. Paste it anywhere.");
  } catch {
    say(out, text);
  }
}

type Post = {
  title: string;
  text: string;
  /** Draws the image to save or post. */
  image: () => Promise<Blob>;
  /** The text quotes a chat, so the person may edit it before it goes anywhere. */
  quotesChat?: boolean;
};

/** Saves the post's image. `copyImage` also puts it on the clipboard so it pastes into a post. */
async function saveImage(post: Post, out: HTMLElement | null, copyImage = true): Promise<boolean> {
  say(out, "Drawing your card…");
  try {
    const blob = await post.image();
    let copied = false;
    if (copyImage) {
      try {
        await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
        copied = true;
      } catch {
        /* clipboard images aren't available everywhere */
      }
    }
    if (demo) {
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = "no-actually.png";
      a.click();
      say(out, "Card downloaded.");
      return true;
    }
    const path = await api.saveShareCard(Array.from(new Uint8Array(await blob.arrayBuffer())));
    say(out, path ? `Saved${copied ? " and copied" : ""}.` : copied ? "Copied the image." : "");
    return !!path;
  } catch (e) {
    say(out, `Couldn't make the card: ${e}`);
    return false;
  }
}

const PASTE_KEY = /Mac|iPhone|iPad/.test(navigator.userAgent) ? "⌘V" : "Ctrl+V";

/** Says what happens next in the share dialog; `paste` makes it the loud, can't-miss version. */
function guide(out: HTMLElement | null, html: string, paste = false) {
  if (!out) return;
  out.innerHTML = html;
  out.className = `share-guide${paste ? " paste" : ""}`;
}

async function share(id: string, post: Post, text: string, out: HTMLElement | null) {
  const target = shareTargets(text).find((t) => t.id === id);
  if (!target) return;
  const copied = await navigator.clipboard.writeText(text).then(
    () => true,
    () => false,
  );
  const pasteNote = copied
    ? `Your text is copied. When ${esc(target.label)} opens, click into the post and press <kbd>${PASTE_KEY}</kbd> to paste it.`
    : `Copy the text above, then paste it into the post.`;
  if (!target.href) {
    // Instagram can't be pre-filled from outside the app: hand over the image and the caption.
    if (await saveImage(post, null, false)) {
      guide(out, `<b>Card saved.</b> Post the image from the Instagram app, then press <kbd>${PASTE_KEY}</kbd> in the caption to paste your text${copied ? "" : " (copy it above first)"}.`, true);
    }
    return;
  }
  guide(out, target.pasteText ? `<b>Paste your text.</b> ${pasteNote}` : `Opening ${esc(target.label)} with your post filled in…`, !!target.pasteText);
  try {
    await api.openShare(target.href);
  } catch (e) {
    guide(out, esc(e));
  }
}

const scorePost = (s: Stats): Post => ({ title: "Share your score", text: shareText(s, filterLabel()), image: () => drawShareCard(s, filterLabel()) });

const catchPost = (e: HallEntry): Post => ({ title: "Share this catch", text: catchShareText(e), image: () => drawCatchCard(e), quotesChat: !!(e.human || e.ai) });

/** The share dialog: what will be posted, one button per network, and image / text fallbacks. */
function openShareDialog(post: Post) {
  document.querySelector("dialog.share-dialog")?.remove();
  const dlg = document.createElement("dialog");
  dlg.className = "share-dialog";
  dlg.setAttribute("aria-labelledby", "share-title");
  const note = post.quotesChat
    ? "This quotes your chat (already redacted). Edit it above before sharing if you like."
    : "Only these numbers and the project link are shared. Never your chats.";
  dlg.innerHTML = `
    <div class="sd-head"><h2 id="share-title">${esc(post.title)}</h2><button class="sd-close" aria-label="Close">×</button></div>
    <label class="eyebrow" for="share-text">What you'll post${post.quotesChat ? " · you can edit it" : ""}</label>
    <textarea id="share-text" rows="${post.quotesChat ? 4 : 3}" ${post.quotesChat ? "" : "readonly"}>${esc(post.text)}</textarea>
    <div class="share-grid">${shareTargets(post.text)
      .map(
        (t) => `<button class="share-btn" data-share="${t.id}"><b>${esc(t.label)}</b>${
          t.href ? (t.pasteText ? `<span class="needs-paste">You'll paste the text</span>` : `<span>Text filled in</span>`) : `<span class="needs-paste">Saves image · paste caption</span>`
        }</button>`,
      )
      .join("")}</div>
    <div id="share-guide" class="share-guide" aria-live="polite">${note}</div>
    <div class="sd-foot"><button class="ghost" id="sd-image">Save image</button><button class="ghost" id="sd-copy">Copy text</button></div>`;
  document.body.appendChild(dlg);
  const out = dlg.querySelector<HTMLElement>("#share-guide");
  const box = dlg.querySelector<HTMLTextAreaElement>("#share-text")!;
  const close = () => {
    dlg.close();
    dlg.remove();
    flushDeferred();
  };
  dlg.querySelector(".sd-close")!.addEventListener("click", close);
  dlg.addEventListener("cancel", (e) => (e.preventDefault(), close())); // Esc
  dlg.addEventListener("click", (e) => e.target === dlg && close()); // backdrop
  dlg.querySelectorAll<HTMLButtonElement>("[data-share]").forEach((b) => b.addEventListener("click", () => share(b.dataset.share!, post, box.value, out)));
  dlg.querySelector("#sd-copy")!.addEventListener("click", () => copyText(box.value, out));
  dlg.querySelector("#sd-image")!.addEventListener("click", () => saveImage(post, out));
  dlg.showModal();
  dlg.querySelector<HTMLButtonElement>("[data-share]")?.focus();
}

// ---- views ---------------------------------------------------------------------------

function verdictBlock(s: Stats) {
  const no = verdictIsNo(s);
  return `
    <div class="eyebrow">This week's verdict</div>
    <h1 class="q">Could <button id="model-trigger" class="subject" aria-haspopup="listbox" aria-expanded="false" title="Pick a model">${esc(
      subjectLabel(),
    )}<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 4.5 6 8l3.5-3.5"/></svg></button> replace you?</h1>
    <div class="verdict">
      <div class="no ${no ? "" : "maybe"}" aria-hidden="true">${no ? "No." : "Hmm."}</div>
      <div class="because">You caught it <b>${num(s.catches_last_7d)} ${s.catches_last_7d === 1 ? "time" : "times"}</b> in the last 7 days.${
        s.pct_real_chats_with_catch != null ? ` It slipped up in ${Math.round(s.pct_real_chats_with_catch)}% of your real chats.` : ""
      }</div>
    </div>
    <div class="score">
      <div class="side you"><span class="eyebrow">You caught it</span><span class="n">${num(s.catches)}</span></div>
      <div class="vs">vs</div>
      <div class="side ai"><span class="eyebrow">It caught you</span><span class="n">${num(s.humbled)}</span></div>
    </div>`;
}

function ladder(s: Stats) {
  return `<ol class="ladder">${TIERS.map(
    ([name, range]) => `<li class="${name === s.tier ? "on" : ""}"><span>${name}</span><span>${range}</span></li>`,
  ).join("")}</ol>`;
}

function chart(s: Stats) {
  const weeks = s.weekly.slice(-20);
  if (!weeks.length) return `<p class="muted">No weeks yet.</p>`;
  const W = 900, H = 240, L = 34, R = 10, T = 26, B = 30;
  const max = Math.max(20, ...weeks.map((w) => w.catches));
  const step = max > 40 ? 20 : max > 20 ? 10 : 5;
  const bw = (W - L - R) / weeks.length;
  const y = (v: number) => T + (H - T - B) * (1 - v / max);
  let svg = `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="Catches per week">`;
  for (let g = 0; g <= max; g += step) {
    svg += `<line x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}" class="grid"/><text x="${L - 8}" y="${y(g) + 4}" text-anchor="end">${g}</text>`;
  }
  svg += `<line x1="${L}" x2="${W - R}" y1="${y(15)}" y2="${y(15)}" class="line15"/><text x="${W - R}" y="${y(15) - 6}" text-anchor="end" class="hand">irreplaceable line</text>`;
  weeks.forEach((w, i) => {
    const x = L + i * bw + bw * 0.18, bwid = bw * 0.64, top = y(w.catches);
    const hot = w.catches >= 15;
    const d = new Date(w.week + "T00:00");
    svg += `<rect x="${x}" y="${top}" width="${bwid}" height="${y(0) - top}" rx="3" class="${hot ? "bar hot" : "bar"}"><title>Week of ${d.toLocaleDateString()}: ${w.catches} catches, ${w.messages} messages</title></rect>`;
    if (hot) svg += `<text x="${x + bwid / 2}" y="${top - 6}" text-anchor="middle" class="hotlabel">${w.catches}</text>`;
    if (i % 2 === 0) svg += `<text x="${x + bwid / 2}" y="${H - 10}" text-anchor="middle">${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}</text>`;
  });
  return svg + "</svg>";
}

function hall(entries: HallEntry[]) {
  if (!entries.length) {
    return `<p class="muted">No saved catches to show. Your privacy setting may be "category only", or the text has aged out. Counts are unaffected.</p>`;
  }
  return `<div class="fame">${entries
    .map(
      (e) => `
      <article class="catch ${e.pinned ? "pinned" : ""}">
        ${e.pinned ? `<span class="pinned-tag">Pinned · kept forever</span>` : ""}
        <div class="meta"><span class="chip">${esc(CATEGORY_TITLES[e.category] ?? e.label)}</span><span class="chip conf">${esc(e.label)} · ${esc(e.day)}</span></div>
        ${e.human ? `<div class="you-said">“${esc(e.human)}”</div>` : ""}
        ${e.ai ? `<div class="ai-said"><b>${esc(e.model || SOURCE_TITLES[e.source])}:</b> ${esc(e.ai)}</div>` : ""}
        <div class="catch-actions">
          <button class="ghost small" data-share-catch="${e.id}">Share</button>
          <button class="ghost small" data-pin="${e.id}" data-pinned="${e.pinned}">${e.pinned ? "Unpin" : "Pin, keep forever"}</button>
        </div>
      </article>`,
    )
    .join("")}</div>`;
}

function shareButtons() {
  return `<div class="share"><button id="open-share">Share your score</button></div>`;
}

function scorecard(s: Stats) {
  const cats = Object.entries(s.categories).sort((a, b) => b[1] - a[1]);
  const topCat = cats[0]?.[1] ?? 1;
  const peak = s.peak_hour == null ? "–" : new Date(2000, 0, 1, s.peak_hour).toLocaleTimeString(undefined, { hour: "numeric" });
  const filtered = Object.keys(filter).length > 0;
  return `
    <section class="toolbar">${
      filtered
        ? `<button class="chip-clear" id="clear-filter">Showing ${esc(filterLabel())} only <span aria-hidden="true">×</span></button>`
        : `<span class="eyebrow">All models · pick one from “All AI” below</span>`
    }<span class="muted small">${esc(s.first_day ?? "")} – ${esc(s.last_day ?? "")} · ${num(s.messages)} messages</span></section>
    <section class="card">
      <div>${verdictBlock(s)}</div>
      <div class="meter">
        ${ladder(s)}
        ${s.next_tier ? `<p class="hand-note">${s.next_tier[1]} more this week for ${esc(s.next_tier[0])}</p>` : ""}
        ${shareButtons()}
      </div>
    </section>
    <section class="stats">
      <div class="stat"><span class="v">${num(s.catches)}</span><span class="l">catches</span></div>
      <div class="stat"><span class="v">${s.pct_real_chats_with_catch != null ? Math.round(s.pct_real_chats_with_catch) + "%" : "–"}</span><span class="l">of real chats had a catch</span></div>
      <div class="stat"><span class="v">${s.best_streak_days} days</span><span class="l">best streak · now ${s.current_streak_days}</span></div>
      <div class="stat"><span class="v">${s.messages_per_catch ? "1 in " + s.messages_per_catch : "–"}</span><span class="l">messages is a catch · peak ${peak}</span></div>
    </section>
    <section><div class="head"><h2>Catches per week</h2></div><div class="chart">${chart(s)}</div></section>
    <section class="split">
      <div>
        <div class="head"><h2>By model</h2></div>
        <div class="models-scroll" tabindex="0" aria-label="Catches by model"><table class="models"><thead><tr><th>Model</th><th>Catches</th><th>Messages</th><th>Rate</th></tr></thead><tbody>
        ${s.models
          .filter((m) => m.messages > 0)
          .map(
            (m) => `<tr><td>${esc(modelName(m))}</td><td class="n">${num(m.catches)}</td><td class="n">${num(m.messages)}</td><td class="n">${((100 * m.catches) / m.messages).toFixed(1)}%</td></tr>`,
          )
          .join("")}
        </tbody></table></div>
      </div>
      <div>
        <div class="head"><h2>What you catch</h2></div>
        <div class="cats">${
          cats.length
            ? cats
                .map(([k, v]) => `<div class="cat"><span>${esc(CATEGORY_TITLES[k] ?? k)}</span><span class="bar-bg"><i style="width:${Math.max(2, (v / topCat) * 100)}%"></i></span><span class="n">${v}</span></div>`)
                .join("")
            : `<p class="muted">No catches yet.</p>`
        }</div>
      </div>
    </section>
    <section><div class="head"><h2>Hall of fame</h2><p class="muted">Snippets are redacted and kept per your privacy setting.</p></div>${hall(s.hall_of_fame)}</section>`;
}

function sourcesView(list: SourceInfo[]) {
  const row = (s: SourceInfo) => {
    const st = s.state;
    const found = s.roots.filter(([, ok]) => ok).map(([p]) => p);
    const where = s.kind === "import" ? (st?.paths.length ? st.paths.join(", ") : "Pick your export file") : found.length ? found.join(", ") : "Not found on this computer";
    const label = st?.allowed ? (st.paused ? "Paused" : "Allowed") : "Not allowed";
    const actions =
      s.kind === "import"
        ? `<button data-import="${s.id}">${st?.paths.length ? "Add another export" : "Import export…"}</button>`
        : st?.allowed
          ? `<button class="ghost" data-pause="${s.id}" data-paused="${st.paused}">${st.paused ? "Resume" : "Pause"}</button><button class="ghost" data-revoke="${s.id}">Stop reading</button>`
          : found.length
            ? `<select data-since="${s.id}" aria-label="How far back to read ${esc(s.name)}"><option value="all">All history</option><option value="30">Last 30 days</option><option value="7">Last 7 days</option><option value="none">From now on</option></select><button data-allow="${s.id}">Allow</button>`
            : "";
    const hint =
      s.kind === "import"
        ? s.id === "chatgpt-export"
          ? "Get it in ChatGPT: Settings → Data controls → Export data."
          : "Get it on claude.ai: Settings → Privacy → Export data."
        : "";
    return `<div class="source ${st?.allowed ? "on" : ""}">
      <div class="src-main"><b>${esc(s.name)}</b><span class="status">${label}</span>
        <p class="muted small mono">${esc(where)}</p>${hint ? `<p class="muted small">${hint}</p>` : ""}</div>
      <div class="src-actions">${actions}${st ? `<button class="danger" data-forget="${s.id}">Forget</button>` : ""}</div>
    </div>`;
  };
  return `
    <section><div class="head"><h2>Sources</h2>
      <p class="muted">Nothing is read until you allow it. No, Actually... reads your messages and the AI's replies on this computer to count catches. Nothing leaves this machine.</p></div>
      <div class="sources">${list.map(row).join("")}</div>
      <p class="muted small">Stop reading keeps your scores. Forget also deletes everything that source produced.</p>
    </section>`;
}

async function settingsView() {
  const [r, autostart] = await Promise.all([api.retention(), api.getAutostart().catch(() => false)]);
  const opt = (level: Level, title: string, body: string) =>
    `<label class="level ${r.level === level ? "on" : ""}"><input type="radio" name="level" value="${level}" ${r.level === level ? "checked" : ""}/><span><b>${title}</b><br/><span class="muted small">${body}</span></span></label>`;
  return `
    <section><div class="head"><h2>General</h2></div>
      <div class="settings-row">
        <label class="toggle"><input id="autostart" type="checkbox" ${autostart ? "checked" : ""} ${demo ? "disabled" : ""}/><span>Launch at login</span></label>
        <span class="muted small">Starts quietly in the menu bar so new catches are counted.</span>
      </div>
      <div class="settings-row">
        <button class="ghost" id="sync-now">Check now</button>
        <span class="muted small" id="sync-line">Last checked ${esc(ago(status.last_sync))}.</span>
      </div>
    </section>
    <section><div class="head"><h2>Privacy</h2>
      <p class="muted">Ordinary messages are never stored, only counted. This controls what's kept for catches you'll see in the Hall of Fame. Text always steps down as it ages, never up.</p></div>
      <div class="levels">
        ${opt("full", "Full message", "Your whole message and the start of the AI's reply. Secrets, emails and links are still redacted.")}
        ${opt("snippet", "Snippet (recommended)", "Only the sentence that matched, up to 140 characters, redacted.")}
        ${opt("category", "Category only", "No text at all. Just “Made it up”, “Still broken”, and so on.")}
      </div>
      <div class="ttl">
        <label>Keep full text for <input id="full-days" type="number" min="1" max="3650" value="${r.full_days}"/> days, then snippet</label>
        <label>Keep snippets for <input id="snippet-days" type="number" min="1" max="3650" value="${r.snippet_days}"/> days, then category only</label>
        <button id="save-privacy">Save</button><span id="privacy-status" class="muted small" aria-live="polite"></span>
      </div>
      <p class="muted small">Pinned catches keep their text until you unpin them.</p>
    </section>`;
}

function glance(s: Stats) {
  const no = verdictIsNo(s);
  return `<div class="glance">
    <div class="g-top">${WORDMARK("brand small")}<span class="tier-label"><span>Tier</span>${esc(s.tier)}</span></div>
    <div class="g-q">Could AI replace you?</div>
    <div class="g-row"><div class="no sm ${no ? "" : "maybe"}">${no ? "No." : "Hmm."}</div>
      <div class="because">You caught it <b>${s.catches_last_7d}×</b> this week.</div></div>
    <div class="g-score"><div><span class="n you">${num(s.catches)}</span><span class="eyebrow">you</span></div><div class="vs">vs</div><div><span class="n ai">${num(s.humbled)}</span><span class="eyebrow">AI</span></div></div>
    <div class="g-meta muted small">${s.current_streak_days ? `${s.current_streak_days}-day streak · ` : ""}${s.next_tier ? `${s.next_tier[1]} more for ${esc(s.next_tier[0])}` : "Top tier"}</div>
    <div class="g-actions"><button id="open-main">Share or see more</button><button class="ghost" id="copy-share" aria-live="polite">Copy text</button></div>
  </div>`;
}

/** "Claude Code", "Claude Code and Codex", "Claude Code, Codex and ChatGPT". */
const listNames = (names: string[]) => (names.length < 2 ? names.join("") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`);

/** Allowed, unpaused sources: what the app is reading right now. */
const watchingNames = (list: SourceInfo[]) => list.filter((x) => x.state?.allowed && !x.state.paused).map((x) => x.name.replace(" (export)", ""));

function onboarding(watching: string[]) {
  if (watching.length) {
    // Allowed, but nothing counted yet (picked "From now on", or the tool has no history yet).
    return `<section class="empty"><span class="eyebrow"><i class="dot live"></i> Watching ${esc(listNames(watching))}</span>
      <h1 class="q">You're all set.</h1>
      <p>Your next chats will be counted as they happen. The first time you catch your AI being wrong, your scorecard shows up here.</p>
      <button class="ghost" id="go-sources">Manage sources</button></section>`;
  }
  return `<section class="empty"><h1 class="q">Let's see if AI could replace you.</h1>
    <p>No, Actually... counts the times you caught your AI being wrong, privately, on this computer. Pick which tools it may read.</p>
    <button id="go-sources">Choose sources</button></section>`;
}

function statusPill() {
  if (status.last_error) return `<button class="sync-pill bad" id="sync-now-top" title="${esc(status.last_error)}">Couldn't check. Retry</button>`;
  if (status.syncing) return `<span class="sync-pill"><i class="dot live"></i>Checking…</span>`;
  return `<span class="sync-pill" title="Checked ${esc(ago(status.last_sync))}"><i class="dot"></i>${demo ? "Demo data" : `Checked ${esc(ago(status.last_sync))}`}</span>`;
}

// ---- picker ----------------------------------------------------------------------------

/** A searchable popover anchored to the headline's model name. Lives on <body> so the card can't clip it. */
let closePicker: ((flush?: boolean) => void) | null = null;

function openPicker(trigger: HTMLElement) {
  closePicker?.(false);
  const items = buildPickItems(allModels);
  const pop = document.createElement("div");
  pop.className = "picker-pop";
  pop.innerHTML = `<input class="pick-search" type="search" placeholder="Search models, e.g. opus 5.5" aria-label="Search models" autocomplete="off" spellcheck="false" />
    <ul class="pick-list" role="listbox" aria-label="Models"></ul>`;
  document.body.appendChild(pop);
  const r = trigger.getBoundingClientRect();
  pop.style.top = `${Math.round(r.bottom + 8)}px`;
  pop.style.left = `${Math.round(Math.max(12, Math.min(r.left, window.innerWidth - pop.offsetWidth - 12)))}px`;
  trigger.setAttribute("aria-expanded", "true");

  const input = pop.querySelector<HTMLInputElement>("input")!;
  const list = pop.querySelector<HTMLUListElement>("ul")!;
  let shown: PickItem[] = items;
  let active = Math.max(0, items.findIndex((i) => sameFilter(i.filter, filter)));

  const draw = () => {
    const searching = input.value.trim().length > 0;
    shown = matchPickItems(items, input.value);
    active = Math.min(active, Math.max(0, shown.length - 1));
    list.innerHTML = shown.length
      ? shown
          .map((i, n) => {
            const rate = i.messages ? ((100 * i.catches) / i.messages).toFixed(1) + "%" : "–";
            return `<li role="option" data-n="${n}" class="d${searching ? 0 : i.depth} ${n === active ? "active" : ""}" aria-selected="${sameFilter(i.filter, filter)}">
              <span class="pick-name"><b>${esc(i.label)}</b>${searching || i.depth === 0 ? `<span class="pick-path">${esc(i.path)}</span>` : ""}</span>
              <span class="pick-count"><b>${num(i.catches)}</b> catches · ${rate}</span></li>`;
          })
          .join("")
      : `<li class="pick-empty">No model matches “${esc(input.value)}”</li>`;
    list.querySelector(".active")?.scrollIntoView({ block: "nearest" });
  };
  /** Hovering only moves the highlight: no rebuild, no scrolling, so the wheel isn't fought. */
  const hover = (n: number) => {
    if (n === active) return;
    list.querySelector(".active")?.classList.remove("active");
    list.querySelector(`[data-n="${n}"]`)?.classList.add("active");
    active = n;
  };
  // Keep the popover under its trigger if the page itself scrolls or resizes.
  const place = () => {
    const r = trigger.getBoundingClientRect();
    pop.style.top = `${Math.round(r.bottom + 8)}px`;
    pop.style.left = `${Math.round(Math.max(12, Math.min(r.left, window.innerWidth - pop.offsetWidth - 12)))}px`;
  };
  const onPageScroll = (e: Event) => {
    if (!pop.contains(e.target as Node)) place();
  };
  const outside = (e: MouseEvent) => {
    if (!pop.contains(e.target as Node) && !trigger.contains(e.target as Node)) close();
  };
  const close = (flush = true) => {
    closePicker = null;
    pop.remove();
    trigger.setAttribute("aria-expanded", "false");
    document.removeEventListener("mousedown", outside, true);
    window.removeEventListener("scroll", onPageScroll, true);
    window.removeEventListener("resize", place);
    if (flush) flushDeferred(); // apply any update that arrived while the picker was open
  };
  closePicker = close;
  const choose = (i: PickItem | undefined) => {
    if (!i) return;
    close(false); // the render below includes any deferred update
    deferred = false;
    filter = i.filter;
    render();
  };
  input.addEventListener("input", () => ((active = 0), draw()));
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") active = Math.min(shown.length - 1, active + 1);
    else if (e.key === "ArrowUp") active = Math.max(0, active - 1);
    else if (e.key === "Enter") return choose(shown[active]);
    else if (e.key === "Escape") return close(), trigger.focus();
    else return;
    e.preventDefault();
    draw();
  });
  list.addEventListener("mousemove", (e) => {
    const n = (e.target as HTMLElement).closest<HTMLElement>("[data-n]")?.dataset.n;
    if (n != null) hover(Number(n));
  });
  window.addEventListener("scroll", onPageScroll, true);
  window.addEventListener("resize", place);
  list.addEventListener("click", (e) => {
    const n = (e.target as HTMLElement).closest<HTMLElement>("[data-n]")?.dataset.n;
    if (n != null) choose(shown[Number(n)]);
  });
  document.addEventListener("mousedown", outside, true);
  draw();
  input.focus();
}

// ---- background updates ----------------------------------------------------------------

let deferred = false;

/** True while the person is mid-action: a popover is open or a field / confirm is in progress. */
function busy() {
  if (document.querySelector(".picker-pop, dialog[open]")) return true;
  if (root.querySelector('[data-confirm="yes"]')) return true;
  const el = document.activeElement;
  return !!el && root.contains(el) && el.matches("input, select, textarea");
}

function updateStatus() {
  const pill = root.querySelector(".sync-pill");
  if (pill) {
    pill.outerHTML = statusPill();
    root.querySelector("#sync-now-top")?.addEventListener("click", () => api.syncNow());
  }
  const line = root.querySelector("#sync-line");
  if (line) line.textContent = `Last checked ${ago(status.last_sync)}.`;
}

/**
 * A sync finished in the background. Only the scorecard shows live numbers, so other tabs just
 * update the status line; and nothing is redrawn under someone who is using a control.
 */
function refresh() {
  updateStatus();
  if (!isGlance && tab !== "score") return;
  if (busy()) {
    deferred = true;
    return;
  }
  render();
}

function flushDeferred() {
  if (!deferred) return;
  deferred = false;
  render();
}
// A field losing focus may be the moment a deferred update can land.
root.addEventListener("focusout", () => setTimeout(() => !busy() && flushDeferred(), 0));

// ---- render ----------------------------------------------------------------------------

let rendering = false;
let again = false;

/** Renders the current view. Overlapping calls (a sync landing mid-render) collapse into one more pass. */
async function render() {
  if (rendering) {
    again = true;
    return;
  }
  rendering = true;
  try {
    closePicker?.(false);
    const s = await api.stats(filter);
    current = s;
    if (!Object.keys(filter).length) allModels = s.models;
    if (isGlance) {
      const watching = s.messages ? [] : watchingNames(await api.sources());
      root.innerHTML = s.messages
        ? glance(s)
        : watching.length
          ? `<div class="glance empty-glance"><span class="eyebrow"><i class="dot live"></i> Watching ${esc(listNames(watching))}</span><div class="g-q">No catches yet.</div><p class="muted small">Your score shows up after your first catch.</p><button class="ghost" id="open-main">Open No, Actually...</button></div>`
          : `<div class="glance empty-glance"><div class="g-q">Could AI replace you?</div><p class="muted small">Pick which AI tools to count, then find out.</p><button id="open-main">Set up</button></div>`;
    } else {
      const titles: Record<Tab, string> = { score: "Scorecard", sources: "Sources", settings: "Settings" };
      const nav = `<header class="top">${WORDMARK()}<div class="top-right">${statusPill()}<nav>${(Object.keys(titles) as Tab[])
        .map((t) => `<button class="tab ${tab === t ? "on" : ""}" data-tab="${t}">${titles[t]}</button>`)
        .join("")}</nav></div></header>`;
      let body = "";
      if (tab === "score") body = s.messages ? scorecard(s) : onboarding(watchingNames(await api.sources()));
      if (tab === "sources") body = sourcesView(await api.sources());
      if (tab === "settings") body = await settingsView();
      root.innerHTML = `<div class="wrap">${nav}${body}</div>`;
    }
    wire(s);
  } catch (e) {
    root.innerHTML = `<div class="wrap"><section class="empty"><h1 class="q">Something went wrong.</h1><p class="muted mono small">${esc(e)}</p><button id="retry">Try again</button></section></div>`;
    root.querySelector("#retry")?.addEventListener("click", () => render());
  } finally {
    rendering = false;
    if (again) {
      again = false;
      render();
    }
  }
}

function wire(s: Stats) {
  const $ = <T extends HTMLElement>(sel: string) => root.querySelector<T>(sel);
  // The button itself confirms the copy, so the quick glance never grows taller.
  $<HTMLButtonElement>("#copy-share")?.addEventListener("click", async (e) => {
    const b = e.currentTarget as HTMLButtonElement;
    const ok = await navigator.clipboard.writeText(shareText(s, filterLabel())).then(() => true, () => false);
    b.textContent = ok ? "Copied ✓" : "Couldn't copy";
    setTimeout(() => (b.textContent = "Copy text"), 1800);
  });
  $("#open-share")?.addEventListener("click", () => openShareDialog(scorePost(s)));
  root.querySelectorAll<HTMLButtonElement>("[data-share-catch]").forEach((b) =>
    b.addEventListener("click", () => {
      const e = s.hall_of_fame.find((h) => h.id === Number(b.dataset.shareCatch));
      if (e) openShareDialog(catchPost(e));
    }),
  );
  $("#open-main")?.addEventListener("click", () => api.openMain());
  $("#go-sources")?.addEventListener("click", () => ((tab = "sources"), render()));
  root.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((b) => b.addEventListener("click", () => ((tab = b.dataset.tab as Tab), render())));
  $("#model-trigger")?.addEventListener("click", (e) => {
    if (closePicker) closePicker();
    else openPicker(e.currentTarget as HTMLElement);
  });
  $("#clear-filter")?.addEventListener("click", () => ((filter = {}), render()));
  root.querySelectorAll<HTMLButtonElement>("[data-pin]").forEach((b) =>
    b.addEventListener("click", async () => {
      await api.pin(Number(b.dataset.pin), b.dataset.pinned !== "true");
      render();
    }),
  );
  root.querySelectorAll<HTMLButtonElement>("[data-allow]").forEach((b) =>
    b.addEventListener("click", async () => {
      const id = b.dataset.allow!;
      const choice = $<HTMLSelectElement>(`[data-since="${id}"]`)?.value ?? "all";
      const fromNow = choice === "none";
      const days = choice === "all" || fromNow ? null : Number(choice);
      // Local calendar date, matching how the store buckets days (toISOString would be UTC).
      const since = days == null ? null : new Date(Date.now() - days * 864e5).toLocaleDateString("en-CA");
      b.disabled = true;
      b.textContent = "Allowing…";
      await api.allow(id, since, fromNow);
      render();
    }),
  );
  const act = (attr: string, fn: (id: string, el: HTMLButtonElement) => Promise<unknown>) =>
    root.querySelectorAll<HTMLButtonElement>(`[data-${attr}]`).forEach((b) => b.addEventListener("click", async () => (await fn(b.dataset[attr]!, b), render())));
  act("revoke", (id) => api.revoke(id));
  act("pause", (id, b) => api.pause(id, b.dataset.paused !== "true"));
  act("import", (id) => api.importExport(id));
  // Forget deletes data, so it takes a second click to confirm.
  root.querySelectorAll<HTMLButtonElement>("[data-forget]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (b.dataset.confirm !== "yes") {
        b.dataset.confirm = "yes";
        b.textContent = "Click again to delete its data";
        return;
      }
      await api.forget(b.dataset.forget!);
      render();
    }),
  );
  root.querySelectorAll<HTMLInputElement>('input[name="level"]').forEach((r) =>
    r.addEventListener("change", () => root.querySelectorAll(".level").forEach((l) => l.classList.toggle("on", l.contains(r)))),
  );
  $("#save-privacy")?.addEventListener("click", async () => {
    const level = root.querySelector<HTMLInputElement>('input[name="level"]:checked')!.value as Level;
    const full_days = Math.max(1, Math.round(Number($<HTMLInputElement>("#full-days")!.value) || 30));
    const snippet_days = Math.max(1, Math.round(Number($<HTMLInputElement>("#snippet-days")!.value) || 365));
    try {
      await api.setRetention({ level, full_days, snippet_days });
      say($("#privacy-status"), "Saved. Older text was trimmed to match.");
    } catch (e) {
      say($("#privacy-status"), String(e));
    }
  });
  $<HTMLInputElement>("#autostart")?.addEventListener("change", async (e) => {
    const box = e.target as HTMLInputElement;
    try {
      await api.setAutostart(box.checked);
    } catch {
      box.checked = !box.checked;
    }
  });
  for (const id of ["#sync-now", "#sync-now-top"]) $(id)?.addEventListener("click", () => api.syncNow());
}

// ---- boot ------------------------------------------------------------------------------

if (isGlance) {
  document.documentElement.classList.add("is-glance");
  if (!demo) window.addEventListener("keydown", (e) => e.key === "Escape" && getCurrentWindow().hide());
}
api.syncStatus().then((s) => ((status = s), render()));
if (!demo) {
  // A check starting changes only the status line; a finished one may change the numbers.
  listen("sync-started", () => ((status = { ...status, syncing: true }), updateStatus()));
  listen("stats-changed", () => api.syncStatus().then((s) => ((status = s), refresh())));
  listen("sync-failed", () => api.syncStatus().then((s) => ((status = s), updateStatus())));
}
// Keep "Checked 3 min ago" honest without re-querying stats.
setInterval(() => current && updateStatus(), 30_000);
window.addEventListener("hashchange", () => location.reload());
// Dev builds only: lets a test simulate a sync finishing in the background.
if (import.meta.env.DEV) Object.assign(window, { __simulateSync: refresh });
render();
