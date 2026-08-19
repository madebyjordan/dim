import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  stat,
  symlink,
  writeFile,
} from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const projectRequire = createRequire(resolve(root, "eclipse/package.json"));

test("contract generation does not require a Unix-style Prettier bin shim", async (context) => {
  const fixture = await mkdtemp(resolve(tmpdir(), "eclipse-contract-"));
  context.after(() => rm(fixture, { recursive: true, force: true }));

  const fixtureProject = resolve(fixture, "eclipse");
  const fixtureBin = resolve(fixtureProject, "node_modules/.bin");
  const fixtureTarget = resolve(fixtureProject, "src/lib/api/generated.ts");

  await Promise.all([
    mkdir(resolve(fixture, "scripts"), { recursive: true }),
    mkdir(resolve(fixture, "api-contract"), { recursive: true }),
    mkdir(dirname(fixtureTarget), { recursive: true }),
    mkdir(fixtureBin, { recursive: true }),
  ]);

  await Promise.all([
    copyFile(
      resolve(root, "scripts/generate-api-contract.mjs"),
      resolve(fixture, "scripts/generate-api-contract.mjs")
    ),
    copyFile(
      resolve(root, "api-contract/openapi.json"),
      resolve(fixture, "api-contract/openapi.json")
    ),
    copyFile(resolve(root, "eclipse/package.json"), resolve(fixtureProject, "package.json")),
    copyFile(resolve(root, "eclipse/.prettierrc"), resolve(fixtureProject, ".prettierrc")),
    copyFile(resolve(root, "eclipse/src/lib/api/generated.ts"), fixtureTarget),
  ]);

  const generated = await readFile(fixtureTarget, "utf8");
  await writeFile(
    fixtureTarget,
    generated.replaceAll("\r\n", "\n").replaceAll("\n", "\r\n"),
    "utf8"
  );

  for (const dependency of ["prettier", "prettier-plugin-svelte"]) {
    const dependencyEntry = projectRequire.resolve(`${dependency}/package.json`);
    await symlink(
      dirname(dependencyEntry),
      resolve(fixtureProject, "node_modules", dependency),
      "junction"
    );
  }

  await writeFile(
    resolve(fixtureBin, "prettier.cmd"),
    "@echo off\r\nnode ..\\prettier\\bin\\prettier.cjs %*\r\n",
    "utf8"
  );

  await assert.rejects(stat(resolve(fixtureBin, "prettier")), { code: "ENOENT" });
  assert.equal((await stat(resolve(fixtureBin, "prettier.cmd"))).isFile(), true);

  const result = spawnSync(
    process.execPath,
    [resolve(fixture, "scripts/generate-api-contract.mjs"), "--check"],
    { cwd: fixture, encoding: "utf8" }
  );

  assert.equal(result.status, 0, result.stderr || result.stdout);

  await writeFile(fixtureTarget, `${await readFile(fixtureTarget, "utf8")}\r\n// stale\r\n`);
  const staleResult = spawnSync(
    process.execPath,
    [resolve(fixture, "scripts/generate-api-contract.mjs"), "--check"],
    { cwd: fixture, encoding: "utf8" }
  );

  assert.equal(staleResult.status, 1);
  assert.match(staleResult.stderr, /Generated API types are stale/);
});
