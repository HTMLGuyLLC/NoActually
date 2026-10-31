#!/usr/bin/env node
// Release file names and the updater's latest.json, for scripts/finish-release.sh.
//
//   node scripts/latest-json.mjs plain-name <file> <version>
//     Prints the plain name a release file gets ("No-Actually-0.2.0-Windows-Setup-x64.exe").
//
//   node scripts/latest-json.mjs version <latest.json>
//     Prints the version a latest.json offers, for scripts/publish-release.sh.
//
//   node scripts/latest-json.mjs finish --version 0.2.0 --tag v0.2.0 --repo owner/name \
//     --assets assets.txt (one "<id>\t<name>" or "<name>" per line) [--in latest.json] [--mac-file NAME --mac-sig FILE.sig]
//     Prints latest.json with every URL pointing at the renamed files, plus the Mac entries.
import { readFileSync } from "node:fs";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

/** Most specific first. Plain names map to themselves, so renaming twice is harmless. */
const PLAIN = [
  ["x64-setup.exe", "Windows-Setup-x64.exe"],
  ["x64_en-US.msi", "Windows-IT-Installer-x64.msi"],
  ["amd64.AppImage", "x86_64.AppImage"],
  ["amd64.deb", "Linux-Ubuntu-Debian-amd64.deb"],
  ["x86_64.rpm", "Linux-Fedora-x86_64.rpm"],
  ["universal.dmg", "Mac-universal.dmg"],
  [".app.tar.gz", "Mac-Update-universal.app.tar.gz"],
];

export function plainName(name, version) {
  const p = `No-Actually-${version}-`;
  if (name.startsWith(p)) return name;
  const hit = PLAIN.find(([suffix]) => name.endsWith(suffix));
  return hit ? p + hit[1] : name;
}

/** Updater targets a universal Mac build serves. The plugin looks up `<os>-<arch>-<installer>`, then `<os>-<arch>`. */
const MAC_TARGETS = ["darwin-universal", "darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"];

/**
 * latest.json for a release: tauri-action's entries (Windows, Linux, maybe macOS) re-pointed at
 * the renamed files, the locally built Mac update added, and every file checked to be on the release.
 * On a draft release tauri-action v1 links files by asset id (".../download/v0.2.0/683868082"), so
 * `assetIds` maps those ids to the files' current names.
 */
export function finishLatest({ latest, version, tag, repo, assets, assetIds = {}, mac, now = new Date() }) {
  const out = {
    version,
    notes: latest?.notes ?? "",
    pub_date: latest?.pub_date ?? now.toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: {},
  };
  const url = (file) => `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(file)}`;
  for (const [target, p] of Object.entries(latest?.platforms ?? {})) {
    const last = decodeURIComponent(p.url.split("/").pop());
    const file = plainName(assetIds[last] ?? last, version);
    out.platforms[target] = { signature: p.signature, url: url(file) };
  }
  if (mac) for (const t of MAC_TARGETS) out.platforms[t] = { signature: mac.signature.trim(), url: url(mac.file) };
  const missing = Object.entries(out.platforms).filter(([, p]) => !assets.includes(decodeURIComponent(p.url.split("/").pop())));
  if (missing.length) throw new Error(`latest.json points at files not on the release: ${missing.map(([t, p]) => `${t} → ${p.url}`).join(", ")}`);
  if (!Object.keys(out.platforms).length) throw new Error("latest.json would list no platforms.");
  return out;
}

function main(argv) {
  const [cmd, ...rest] = argv;
  if (cmd === "version") {
    const { version } = JSON.parse(readFileSync(rest[0], "utf8"));
    if (typeof version !== "string" || !version) throw new Error("latest.json has no version");
    console.log(version);
    return;
  }
  if (cmd === "plain-name") {
    const [file, version] = rest;
    if (!file || !version) throw new Error("usage: plain-name <file> <version>");
    console.log(plainName(file, version));
    return;
  }
  if (cmd === "finish") {
    const { values: v } = parseArgs({
      args: rest,
      options: Object.fromEntries(["in", "version", "tag", "repo", "assets", "mac-file", "mac-sig"].map((k) => [k, { type: "string" }])),
    });
    for (const k of ["version", "tag", "repo", "assets"]) if (!v[k]) throw new Error(`--${k} is required`);
    const latest = v.in ? JSON.parse(readFileSync(v.in, "utf8")) : null;
    // One asset per line: "<id>\t<name>", or just "<name>".
    const lines = readFileSync(v.assets, "utf8").split("\n").map((s) => s.trim()).filter(Boolean);
    const rows = lines.map((l) => (l.includes("\t") ? l.split("\t") : [null, l]));
    const assets = rows.map(([, name]) => name);
    const assetIds = Object.fromEntries(rows.filter(([id]) => id));
    const mac = v["mac-file"] ? { file: basename(v["mac-file"]), signature: readFileSync(v["mac-sig"], "utf8") } : null;
    console.log(JSON.stringify(finishLatest({ latest, version: v.version, tag: v.tag, repo: v.repo, assets, assetIds, mac }), null, 2));
    return;
  }
  throw new Error("usage: latest-json.mjs plain-name|version|finish …");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
