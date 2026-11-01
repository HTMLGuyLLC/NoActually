// Run with: node --test scripts/*.test.mjs
// The release scripts' argument checks. Each must refuse before it runs git or gh, so these need neither.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const script = (name) => new URL(`./${name}`, import.meta.url).pathname;
const run = (name, ...args) => spawnSync("bash", [script(name), ...args], { encoding: "utf8" });

test("finish-release.sh wants exactly one release tag", () => {
  for (const args of [[], ["0.2.0"], ["v0.2"], ["v0.2.0-beta.1"], ["v0.2.0", "extra"], ["v0.2.0", "--bogus"]]) {
    const r = run("finish-release.sh", ...args);
    assert.equal(r.status, 1, JSON.stringify(args));
    assert.match(r.stderr, /usage: scripts\/finish-release\.sh|isn't a release tag|unknown option/, JSON.stringify(args));
  }
});

test("publish-release.sh wants a release tag and a channel", () => {
  const cases = [
    [[], /usage: scripts\/publish-release\.sh/],
    [["v0.2.0"], /usage: scripts\/publish-release\.sh/],
    [["v0.2.0", "gamma"], /isn't a channel/],
    [["0.2.0", "beta"], /isn't a release tag/],
    [["v0.2.0-beta.1", "beta"], /isn't a release tag/],
    [["v0.2.0", "beta", "extra"], /usage: scripts\/publish-release\.sh/],
    [["v0.2.0", "beta", "--bogus"], /unknown option '--bogus'/],
  ];
  for (const [args, message] of cases) {
    const r = run("publish-release.sh", ...args);
    assert.equal(r.status, 1, JSON.stringify(args));
    assert.match(r.stderr, message, JSON.stringify(args));
  }
});

test("start-release.sh wants exactly one X.Y.Z tag", () => {
  for (const args of [["0.2.0"], ["v0.2"], ["v0.2.0-beta.1"], ["latest"]]) {
    const r = run("start-release.sh", ...args);
    assert.equal(r.status, 1, JSON.stringify(args));
    assert.match(r.stderr, /isn't a release tag/, JSON.stringify(args));
  }
  const none = run("start-release.sh");
  assert.notEqual(none.status, 0);
  assert.match(none.stderr, /usage: scripts\/start-release\.sh/);
  const two = run("start-release.sh", "v0.2.0", "v0.2.1");
  assert.equal(two.status, 1);
  assert.match(two.stderr, /usage: scripts\/start-release\.sh/);
});
