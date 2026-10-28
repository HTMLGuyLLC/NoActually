// Run with: node --test scripts/
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { finishLatest, plainName } from "./latest-json.mjs";

const REPO = "HTMLGuyLLC/NoActually";
const dl = (file) => `https://github.com/${REPO}/releases/download/v0.2.0/${file}`;

test("plainName matches the release page names", () => {
  assert.equal(plainName("No.Actually..._0.2.0_x64-setup.exe", "0.2.0"), "No-Actually-0.2.0-Windows-Setup.exe");
  assert.equal(plainName("No.Actually..._0.2.0_x64_en-US.msi", "0.2.0"), "No-Actually-0.2.0-Windows-IT-Installer.msi");
  assert.equal(plainName("No.Actually..._0.2.0_amd64.AppImage", "0.2.0"), "No-Actually-0.2.0-Linux.AppImage");
  assert.equal(plainName("No.Actually..._universal.app.tar.gz", "0.2.0"), "No-Actually-0.2.0-Mac-Update.app.tar.gz");
  assert.equal(plainName("No-Actually-0.2.0-Mac.dmg", "0.2.0"), "No-Actually-0.2.0-Mac.dmg", "already plain");
  assert.equal(plainName("SHA256SUMS.txt", "0.2.0"), "SHA256SUMS.txt", "unknown files keep their name");
  assert.equal(plainName("No.Actually..._0.2.0_x64-setup.exe.sig", "0.2.0"), "No.Actually..._0.2.0_x64-setup.exe.sig");
});

const ci = {
  version: "0.2.0",
  notes: "See the changelog.",
  pub_date: "2026-11-01T10:00:00Z",
  platforms: {
    "windows-x86_64": { signature: "W", url: dl("No.Actually..._0.2.0_x64-setup.exe") },
    "windows-x86_64-msi": { signature: "M", url: dl("No.Actually..._0.2.0_x64_en-US.msi") },
    "linux-x86_64": { signature: "L", url: dl("No.Actually..._0.2.0_amd64.AppImage") },
  },
};
const assets = [
  "No-Actually-0.2.0-Windows-Setup.exe",
  "No-Actually-0.2.0-Windows-IT-Installer.msi",
  "No-Actually-0.2.0-Linux.AppImage",
  "No-Actually-0.2.0-Mac.dmg",
  "No-Actually-0.2.0-Mac-Update.app.tar.gz",
];
const mac = { file: "No-Actually-0.2.0-Mac-Update.app.tar.gz", signature: "S\n" };

test("re-points CI entries at the renamed files and adds the Mac update", () => {
  const out = finishLatest({ latest: ci, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac });
  assert.equal(out.version, "0.2.0");
  assert.equal(out.notes, "See the changelog.");
  assert.equal(out.pub_date, "2026-11-01T10:00:00Z");
  assert.deepEqual(out.platforms["windows-x86_64"], { signature: "W", url: dl("No-Actually-0.2.0-Windows-Setup.exe") });
  assert.equal(out.platforms["windows-x86_64-msi"].url, dl("No-Actually-0.2.0-Windows-IT-Installer.msi"));
  assert.equal(out.platforms["linux-x86_64"].url, dl("No-Actually-0.2.0-Linux.AppImage"));
  for (const t of ["darwin-universal", "darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"]) {
    assert.deepEqual(out.platforms[t], { signature: "S", url: dl("No-Actually-0.2.0-Mac-Update.app.tar.gz") });
  }
});

test("running it twice gives the same file", () => {
  const once = finishLatest({ latest: ci, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac });
  const twice = finishLatest({ latest: once, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac });
  assert.deepEqual(twice, once);
});

test("the local Mac build replaces a CI Mac entry", () => {
  const withMac = { ...ci, platforms: { ...ci.platforms, "darwin-aarch64": { signature: "CI", url: dl("No.Actually..._universal.app.tar.gz") } } };
  const out = finishLatest({ latest: withMac, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac });
  assert.equal(out.platforms["darwin-aarch64"].signature, "S");
});

test("works with no CI file (Mac only)", () => {
  const out = finishLatest({ latest: null, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac, now: new Date("2026-11-02T03:04:05.678Z") });
  assert.equal(out.pub_date, "2026-11-02T03:04:05Z");
  assert.deepEqual(Object.keys(out.platforms).filter((t) => !t.startsWith("darwin")), []);
});

test("refuses a file that points at missing downloads", () => {
  assert.throws(
    () => finishLatest({ latest: ci, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets: assets.filter((a) => !a.endsWith(".AppImage")), mac }),
    /linux-x86_64/,
  );
  assert.throws(() => finishLatest({ latest: null, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac: null }), /no platforms/);
});

test("follows tauri-action's asset-id links on a draft release", () => {
  const byId = {
    ...ci,
    platforms: {
      "windows-x86_64": { signature: "W", url: dl("683870400") },
      "linux-x86_64": { signature: "L", url: dl("683868082") },
    },
  };
  // finish-release.sh renames the files first, so the ids already point at the plain names.
  const assetIds = { 683870400: "No-Actually-0.2.0-Windows-Setup.exe", 683868082: "No-Actually-0.2.0-Linux.AppImage" };
  const out = finishLatest({ latest: byId, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, assetIds, mac });
  assert.equal(out.platforms["windows-x86_64"].url, dl("No-Actually-0.2.0-Windows-Setup.exe"));
  assert.equal(out.platforms["linux-x86_64"].url, dl("No-Actually-0.2.0-Linux.AppImage"));
  assert.throws(
    () => finishLatest({ latest: byId, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac }),
    /files not on the release/,
    "an unknown id is still refused",
  );
});

test("the command line reads `<id>\\t<name>` asset lists", () => {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-"));
  try {
    const latest = { ...ci, platforms: { "linux-x86_64": { signature: "L", url: dl("42") } } };
    writeFileSync(join(dir, "in.json"), JSON.stringify(latest));
    writeFileSync(join(dir, "assets.txt"), "42\tNo-Actually-0.2.0-Linux.AppImage\nNo-Actually-0.2.0-Mac.dmg\n");
    const script = new URL("./latest-json.mjs", import.meta.url).pathname;
    const args = ["finish", "--version", "0.2.0", "--tag", "v0.2.0", "--repo", REPO, "--assets", join(dir, "assets.txt"), "--in", join(dir, "in.json")];
    const r = spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(r.stdout).platforms["linux-x86_64"].url, dl("No-Actually-0.2.0-Linux.AppImage"));
  } finally {
    rmSync(dir, { recursive: true });
  }
});
