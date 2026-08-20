import assert from "node:assert/strict";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const script = resolve(root, "scripts/prepare-pnpm.ps1");

function run(command, args, env, cwd = root) {
  return spawnSync(command, args, { cwd, env, encoding: "utf8" });
}

function bashPath(path) {
  const normalized = path.replaceAll("\\", "/");
  return `/${normalized[0].toLowerCase()}${normalized.slice(2)}`;
}

test(
  "Windows prepares idempotent user-level Corepack shims for fresh terminals",
  { skip: process.platform !== "win32", timeout: 120_000 },
  () => {
    const directory = mkdtempSync(resolve(tmpdir(), "eclipse-pnpm-shim-test-"));
    const mask = resolve(directory, "mask");
    const shims = resolve(directory, "shims");
    const maskCommand = resolve(mask, "pnpm.cmd");
    const nodeDirectory = dirname(process.execPath);
    const env = {
      ...process.env,
      PATH: `${mask}${delimiter}${nodeDirectory}${delimiter}${process.env.PATH}`,
    };

    try {
      // The input environment deliberately has no usable global pnpm command.
      mkdirSync(mask);
      writeFileSync(
        maskCommand,
        "@echo off\r\necho Unexpected global pnpm command. 1>&2\r\nexit /b 86\r\n",
        { flag: "wx" },
      );
      const unavailable = run("cmd.exe", ["/d", "/c", "pnpm --version"], env);
      assert.equal(unavailable.status, 86);

      const corepack = run(
        "cmd.exe",
        ["/d", "/c", "corepack pnpm --version"],
        env,
      );
      assert.equal(
        corepack.status,
        0,
        `${corepack.stdout}\n${corepack.stderr}`,
      );
      assert.equal(corepack.stdout.trim(), "11.9.0");

      const prepared = run(
        "powershell.exe",
        [
          "-NoProfile",
          "-NonInteractive",
          "-ExecutionPolicy",
          "Bypass",
          "-File",
          script,
          "-RepositoryRoot",
          root,
          "-ShimDirectory",
          shims,
          "-PathTarget",
          "Process",
          "-ForceShim",
        ],
        env,
      );
      assert.equal(
        prepared.status,
        0,
        `${prepared.stdout}\n${prepared.stderr}`,
      );
      assert.match(prepared.stdout, /ready\|11\.9\.0\|.*pnpm\.cmd\|prepared/i);

      const pnpmCommand = resolve(shims, "pnpm.cmd");
      assert.equal(existsSync(pnpmCommand), true);
      assert.match(readFileSync(pnpmCommand, "utf8"), /corepack.*pnpm/i);
      assert.equal(existsSync(resolve(shims, "pnpm.ps1")), false);

      const freshEnv = {
        ...process.env,
        PATH: `${shims}${delimiter}${nodeDirectory}${delimiter}${process.env.PATH}`,
      };
      const cmd = run("cmd.exe", ["/d", "/c", "pnpm --version"], freshEnv);
      assert.equal(cmd.status, 0, `${cmd.stdout}\n${cmd.stderr}`);
      assert.equal(cmd.stdout.trim(), "11.9.0");

      const powershell = run(
        "powershell.exe",
        ["-NoProfile", "-NonInteractive", "-Command", "pnpm --version"],
        freshEnv,
      );
      assert.equal(
        powershell.status,
        0,
        `${powershell.stdout}\n${powershell.stderr}`,
      );
      assert.equal(powershell.stdout.trim(), "11.9.0");

      const gitBash = resolve(
        process.env.ProgramFiles ?? "C:/Program Files",
        "Git/bin/bash.exe",
      );
      if (existsSync(gitBash)) {
        const bash = run(
          gitBash,
          [
            "-c",
            'export PATH="$1:$PATH"; cd "$2"; pnpm --version',
            "pnpm-test",
            bashPath(shims),
            bashPath(root),
          ],
          process.env,
        );
        assert.equal(bash.status, 0, `${bash.stdout}\n${bash.stderr}`);
        assert.equal(bash.stdout.trim(), "11.9.0");
      }

      const modifiedBefore = statSync(pnpmCommand).mtimeMs;
      const repeated = run(
        "powershell.exe",
        [
          "-NoProfile",
          "-NonInteractive",
          "-ExecutionPolicy",
          "Bypass",
          "-File",
          script,
          "-RepositoryRoot",
          root,
          "-ShimDirectory",
          shims,
          "-PathTarget",
          "Process",
        ],
        env,
      );
      assert.equal(
        repeated.status,
        0,
        `${repeated.stdout}\n${repeated.stderr}`,
      );
      assert.match(repeated.stdout, /ready\|11\.9\.0\|.*pnpm\.cmd\|existing/i);
      assert.equal(statSync(pnpmCommand).mtimeMs, modifiedBefore);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

test("Windows pnpm preparation persists only a user-level PATH entry", () => {
  const source = readFileSync(script, "utf8");
  assert.match(source, /corepack\.Source enable pnpm --install-directory/);
  assert.match(
    source,
    /SetEnvironmentVariable\('Path', \$newUserPath, 'User'\)/,
  );
  assert.doesNotMatch(source, /SetEnvironmentVariable\([^\n]*'Machine'/);
  assert.doesNotMatch(source, /npm(?:\.cmd)?\s+install\s+-g\s+pnpm/i);
});
