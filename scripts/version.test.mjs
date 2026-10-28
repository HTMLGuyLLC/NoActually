// Run with: node --test scripts/*.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const SCRIPT = new URL("./version.sh", import.meta.url).pathname;

// A throwaway repo with just the files version.sh reads.
function repo({ cargo = "0.3.0", pkg = "0.3.0", conf = "0.3.0" } = {}) {
  const root = mkdtempSync(join(tmpdir(), "version-sh-"));
  mkdirSync(join(root, "scripts"));
  mkdirSync(join(root, "app/src-tauri"), { recursive: true });
  cpSync(SCRIPT, join(root, "scripts/version.sh"));
  writeFileSync(
    join(root, "Cargo.toml"),
    `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${cargo}"\nedition = "2021"\n\n[workspace.dependencies]\nversion = "9.9.9"\n`,
  );
  writeFileSync(join(root, "app/package.json"), JSON.stringify({ version: pkg }));
  writeFileSync(join(root, "app/src-tauri/tauri.conf.json"), JSON.stringify(conf ? { version: conf } : {}));
  return root;
}

function run(root, ...args) {
  return spawnSync("bash", [join(root, "scripts/version.sh"), ...args], { encoding: "utf8" });
}

test("prints the [workspace.package] version, not a later `version =` line", () => {
  const root = repo();
  try {
    const r = run(root);
    assert.equal(r.status, 0);
    assert.equal(r.stdout.trim(), "0.3.0");
  } finally {
    rmSync(root, { recursive: true });
  }
});

test("check passes when the versions agree, or tauri.conf.json leaves it out", () => {
  for (const conf of ["0.3.0", ""]) {
    const root = repo({ conf });
    try {
      assert.equal(run(root, "check").status, 0, `conf=${conf || "<unset>"}`);
    } finally {
      rmSync(root, { recursive: true });
    }
  }
});

test("check fails when a version differs", () => {
  for (const opts of [{ pkg: "0.2.9" }, { conf: "0.2.9" }]) {
    const root = repo(opts);
    try {
      const r = run(root, "check");
      assert.equal(r.status, 1, JSON.stringify(opts));
      assert.match(r.stderr, /differs/);
    } finally {
      rmSync(root, { recursive: true });
    }
  }
});
