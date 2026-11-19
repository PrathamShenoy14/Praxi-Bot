import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import test from "node:test";
import { fileURLToPath, URL } from "node:url";

const script = fileURLToPath(new URL("../check-prereqs.mjs", import.meta.url));

test("reports all tool checks when Rust and pnpm are missing", (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "praxi-check-prereqs-"));
  t.after(() => rmdirSync(scratch));

  // Remove all spellings of PATH so Windows gets only the isolated path.
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => key.toUpperCase() !== "PATH"),
  );
  env.PATH = scratch;

  const { error, status, stdout, stderr } = spawnSync(process.execPath, [script], {
    cwd: scratch,
    env,
    encoding: "utf8",
  });

  assert.ifError(error);
  assert.equal(status, 1, stdout + stderr);
  assert.match(stdout, /^Rust: missing\b/m);
  assert.match(stdout, /^Node\.js: (ok|too old)\b/m);
  assert.ok(stdout.includes(process.versions.node));
  assert.match(stdout, /^pnpm: missing\b/m);
  assert.match(stdout, /^C toolchain: (ok|missing)\b/m);
  assert.match(stdout, /^Linux apt packages: skipped\b/m);
});
