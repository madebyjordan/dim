import assert from "node:assert/strict";
import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");

function run(command, args, env) {
  return spawnSync(command, args, {
    cwd: root,
    env,
    encoding: "utf8",
    shell: process.platform === "win32",
  });
}

test(
  "frontend bootstrap works through Corepack when a direct pnpm command is unavailable",
  { timeout: 120_000 },
  () => {
    const directory = mkdtempSync(resolve(tmpdir(), "eclipse-corepack-test-"));
    const sentinel = resolve(
      directory,
      process.platform === "win32" ? "pnpm.cmd" : "pnpm",
    );
    if (process.platform === "win32") {
      writeFileSync(
        sentinel,
        "@echo off\r\necho Direct pnpm invocation is disabled for this test. 1>&2\r\nexit /b 86\r\n",
      );
    } else {
      writeFileSync(
        sentinel,
        "#!/usr/bin/env sh\necho 'Direct pnpm invocation is disabled for this test.' >&2\nexit 86\n",
      );
      chmodSync(sentinel, 0o755);
    }

    const env = {
      ...process.env,
      PATH: `${directory}${delimiter}${process.env.PATH}`,
      NO_COLOR: "1",
    };

    try {
      const direct = run("pnpm", ["--version"], env);
      assert.equal(direct.status, 86, `${direct.stdout}\n${direct.stderr}`);

      const version = run("corepack", ["pnpm", "--version"], env);
      assert.equal(version.status, 0, `${version.stdout}\n${version.stderr}`);
      assert.equal(version.stdout.trim(), "11.9.0");

      const install = run(
        "corepack",
        ["pnpm", "--dir", "eclipse", "install", "--frozen-lockfile"],
        env,
      );
      assert.equal(install.status, 0, `${install.stdout}\n${install.stderr}`);

      const build = run("corepack", ["pnpm", "--dir", "eclipse", "build"], env);
      assert.equal(build.status, 0, `${build.stdout}\n${build.stderr}`);
      assert.match(
        `${build.stdout}\n${build.stderr}`,
        /corepack pnpm contract:check && vite build/,
      );
      assert.doesNotMatch(
        `${build.stdout}\n${build.stderr}`,
        /pnpm.*not recognized|pnpm: command not found/i,
      );
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

test("package scripts do not assume a globally resolvable pnpm command", () => {
  for (const path of ["package.json", "eclipse/package.json"]) {
    const manifest = JSON.parse(readFileSync(resolve(root, path), "utf8"));
    for (const [name, script] of Object.entries(manifest.scripts ?? {})) {
      assert.doesNotMatch(
        script,
        /(?:^|&&|\|\||;)\s*pnpm(?:\s|$)/,
        `${path} script ${name} must invoke repository-pinned pnpm through Corepack`,
      );
    }
  }

  const build = readFileSync(resolve(root, "scripts/build.mjs"), "utf8");
  assert.match(
    build,
    /\["pnpm", "--dir", "eclipse", "install", "--frozen-lockfile"\]/,
  );
  assert.match(build, /\["pnpm", "--dir", "eclipse", "build"\]/);
});
