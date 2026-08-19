import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");

test("the Windows dependency graph excludes incompatible ntapi 0.3 releases", () => {
  const result = spawnSync(
    "cargo",
    ["tree", "--locked", "--target", "x86_64-pc-windows-msvc", "-e", "normal,build"],
    { cwd: root, encoding: "utf8" }
  );

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.doesNotMatch(result.stdout, /ntapi v0\.3\./);
});
