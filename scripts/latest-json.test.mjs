// Run with: node --test scripts/
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { finishLatest, missingPlatforms, plainName, REQUIRED } from "./latest-json.mjs";

const REPO = "HTMLGuyLLC/NoActually";
const dl = (file) => `https://github.com/${REPO}/releases/download/v0.2.0/${file}`;

test("plainName matches the release page names", () => {
  assert.equal(plainName("No.Actually..._0.2.0_x64-setup.exe", "0.2.0"), "No-Actually-0.2.0-Windows-Setup-x64.exe");
  assert.equal(plainName("No.Actually..._0.2.0_x64_en-US.msi", "0.2.0"), "No-Actually-0.2.0-Windows-IT-Installer-x64.msi");
  assert.equal(plainName("No.Actually..._0.2.0_amd64.AppImage", "0.2.0"), "No-Actually-0.2.0-x86_64.AppImage");
  assert.equal(plainName("No.Actually..._universal.app.tar.gz", "0.2.0"), "No-Actually-0.2.0-Mac-Update-universal.app.tar.gz");
  assert.equal(plainName("No-Actually-0.2.0-Mac-universal.dmg", "0.2.0"), "No-Actually-0.2.0-Mac-universal.dmg", "already plain");
  assert.equal(plainName("SHA256SUMS.txt", "0.2.0"), "SHA256SUMS.txt", "unknown files keep their name");
  assert.equal(plainName("No.Actually..._0.2.0_x64-setup.exe.sig", "0.2.0"), "No.Actually..._0.2.0_x64-setup.exe.sig");
});

test("every download names its arch, and the AppImage doesn't say Linux (AppImage catalog rule)", () => {
  const built = ["x64-setup.exe", "x64_en-US.msi", "amd64.AppImage", "amd64.deb", "x86_64.rpm", "universal.dmg", "universal.app.tar.gz"];
  for (const b of built) {
    const name = plainName(`No.Actually..._0.2.0_${b}`, "0.2.0");
    assert.match(name, /-(x64|x86_64|amd64|universal)\./, name);
  }
  assert.doesNotMatch(plainName("No.Actually..._0.2.0_amd64.AppImage", "0.2.0"), /linux/i);
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
  "No-Actually-0.2.0-Windows-Setup-x64.exe",
  "No-Actually-0.2.0-Windows-IT-Installer-x64.msi",
  "No-Actually-0.2.0-x86_64.AppImage",
  "No-Actually-0.2.0-Mac-universal.dmg",
  "No-Actually-0.2.0-Mac-Update-universal.app.tar.gz",
];
const mac = { file: "No-Actually-0.2.0-Mac-Update-universal.app.tar.gz", signature: "S\n" };

test("re-points CI entries at the renamed files and adds the Mac update", () => {
  const out = finishLatest({ latest: ci, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac });
  assert.equal(out.version, "0.2.0");
  assert.equal(out.notes, "See the changelog.");
  assert.equal(out.pub_date, "2026-11-01T10:00:00Z");
  assert.deepEqual(out.platforms["windows-x86_64"], { signature: "W", url: dl("No-Actually-0.2.0-Windows-Setup-x64.exe") });
  assert.equal(out.platforms["windows-x86_64-msi"].url, dl("No-Actually-0.2.0-Windows-IT-Installer-x64.msi"));
  assert.equal(out.platforms["linux-x86_64"].url, dl("No-Actually-0.2.0-x86_64.AppImage"));
  for (const t of ["darwin-universal", "darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"]) {
    assert.deepEqual(out.platforms[t], { signature: "S", url: dl("No-Actually-0.2.0-Mac-Update-universal.app.tar.gz") });
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
  const out = finishLatest({ latest: null, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac, allowMissing: true, now: new Date("2026-11-02T03:04:05.678Z") });
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
  const assetIds = { 683870400: "No-Actually-0.2.0-Windows-Setup-x64.exe", 683868082: "No-Actually-0.2.0-x86_64.AppImage" };
  const out = finishLatest({ latest: byId, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, assetIds, mac });
  assert.equal(out.platforms["windows-x86_64"].url, dl("No-Actually-0.2.0-Windows-Setup-x64.exe"));
  assert.equal(out.platforms["linux-x86_64"].url, dl("No-Actually-0.2.0-x86_64.AppImage"));
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
    writeFileSync(join(dir, "assets.txt"), "42\tNo-Actually-0.2.0-x86_64.AppImage\nNo-Actually-0.2.0-Mac-universal.dmg\n");
    const script = new URL("./latest-json.mjs", import.meta.url).pathname;
    const args = ["finish", "--version", "0.2.0", "--tag", "v0.2.0", "--repo", REPO, "--assets", join(dir, "assets.txt"), "--in", join(dir, "in.json")];
    const strict = spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
    assert.notEqual(strict.status, 0, "a platform is missing");
    assert.match(strict.stderr, /missing platforms: windows-x86_64/);
    const r = spawnSync(process.execPath, [script, ...args, "--allow-missing"], { encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(r.stdout).platforms["linux-x86_64"].url, dl("No-Actually-0.2.0-x86_64.AppImage"));
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("version prints what a latest.json offers, and refuses one with none", () => {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-"));
  try {
    const script = new URL("./latest-json.mjs", import.meta.url).pathname;
    const run = (body) => {
      writeFileSync(join(dir, "latest.json"), body);
      return spawnSync(process.execPath, [script, "version", join(dir, "latest.json")], { encoding: "utf8" });
    };
    const good = run(JSON.stringify({ version: "0.2.0" }));
    assert.equal(good.status, 0, good.stderr);
    assert.equal(good.stdout.trim(), "0.2.0");
    assert.notEqual(run("{}").status, 0);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("a release must list every platform, unless --allow-missing", () => {
  const base = { latest: ci, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets };
  assert.deepEqual(missingPlatforms(finishLatest({ ...base, mac }).platforms), []);
  const noLinux = { ...ci, platforms: { "windows-x86_64": ci.platforms["windows-x86_64"] } };
  assert.throws(() => finishLatest({ ...base, latest: noLinux, mac }), /missing platforms: linux-x86_64/);
  assert.throws(() => finishLatest({ ...base, mac: null }), /missing platforms: darwin-universal/, "no Mac update");
  const out = finishLatest({ ...base, mac: null, allowMissing: true });
  assert.ok(!("darwin-universal" in out.platforms) && "linux-x86_64" in out.platforms);
  assert.deepEqual(missingPlatforms(undefined), REQUIRED);
});

test("check passes a complete latest.json and fails one with a platform missing", () => {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-"));
  try {
    const script = new URL("./latest-json.mjs", import.meta.url).pathname;
    const check = (body, ...flags) => {
      writeFileSync(join(dir, "latest.json"), JSON.stringify(body));
      return spawnSync(process.execPath, [script, "check", join(dir, "latest.json"), ...flags], { encoding: "utf8" });
    };
    const full = finishLatest({ latest: ci, version: "0.2.0", tag: "v0.2.0", repo: REPO, assets, mac });
    assert.equal(check(full).status, 0);
    const { "linux-x86_64": _gone, ...rest } = full.platforms;
    const partial = check({ ...full, platforms: rest });
    assert.equal(partial.status, 1);
    assert.match(partial.stderr, /missing platforms: linux-x86_64/);
    assert.equal(check({ ...full, platforms: rest }, "--allow-missing").status, 0);
  } finally {
    rmSync(dir, { recursive: true });
  }
});
