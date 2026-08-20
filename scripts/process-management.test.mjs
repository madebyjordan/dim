import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";
import {
  cleanupOwnedProcesses,
  leaseMatchesProcess,
  spawnManaged,
} from "./process-management.mjs";

const wait = (milliseconds) => new Promise((resolveWait) => setTimeout(resolveWait, milliseconds));

async function waitUntil(check, message, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (check()) return;
    await wait(50);
  }
  assert.fail(message);
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

test("lease identity rejects a recycled PID", () => {
  const info = { executable: "C:\\tools\\node.exe", started: "new" };
  assert.equal(leaseMatchesProcess({ identity: { ...info } }, info, "win32"), true);
  assert.equal(
    leaseMatchesProcess({ identity: { ...info, started: "old" } }, info, "win32"),
    false,
  );
});

test("managed processes are logged, guarded against duplicates, and cleaned", async () => {
  const root = mkdtempSync(resolve(tmpdir(), "eclipse-process-test-"));
  let managed;
  try {
    managed = await spawnManaged({
      root,
      name: "validation",
      owner: "codex",
      command: process.execPath,
      args: ["-e", "console.log('validation-ready'); setInterval(() => {}, 1000)"],
      logPath: "logs/validation.log",
    });
    await assert.rejects(
      spawnManaged({
        root,
        name: "validation",
        owner: "codex",
        command: process.execPath,
        args: ["-e", "setInterval(() => {}, 1000)"],
      }),
      new RegExp(`already running as PID ${managed.child.pid}`),
    );
    await waitUntil(
      () => readFileSync(resolve(root, "logs/validation.log"), "utf8").includes("validation-ready"),
      "managed output was not captured",
    );
    managed.stop();
    await managed.closed;
    assert.equal(alive(managed.child.pid), false);
  } finally {
    managed?.stop();
    rmSync(root, { recursive: true, force: true });
  }
});

test("owner cleanup stops only matching process trees", async () => {
  const root = mkdtempSync(resolve(tmpdir(), "eclipse-owner-test-"));
  const childPidPath = resolve(root, "child.pid");
  const parentScript = resolve(root, "parent.cjs");
  writeFileSync(
    parentScript,
    [
      "const { spawn } = require('node:child_process');",
      "const { writeFileSync } = require('node:fs');",
      "const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)']);",
      "writeFileSync(process.argv[2], String(child.pid));",
      "setInterval(() => {}, 1000);",
    ].join("\n"),
  );
  let codex;
  let interactive;
  try {
    codex = await spawnManaged({
      root,
      name: "codex-tree",
      owner: "codex",
      command: process.execPath,
      args: [parentScript, childPidPath],
      logPath: "logs/codex-tree.log",
    });
    interactive = await spawnManaged({
      root,
      name: "developer-process",
      owner: "interactive",
      command: process.execPath,
      args: ["-e", "setInterval(() => {}, 1000)"],
      logPath: "logs/developer.log",
    });
    await waitUntil(() => {
      try {
        return Number(readFileSync(childPidPath, "utf8")) > 0;
      } catch {
        return false;
      }
    }, "child process PID was not recorded");
    const childPid = Number(readFileSync(childPidPath, "utf8"));
    const cleaned = cleanupOwnedProcesses({ root, owner: "codex" });
    assert.deepEqual(cleaned.map(({ name, stopped }) => ({ name, stopped })), [
      { name: "codex-tree", stopped: true },
    ]);
    await waitUntil(
      () => !alive(codex.child.pid) && !alive(childPid),
      "owned parent or child remained alive",
    );
    assert.equal(alive(interactive.child.pid), true);
  } finally {
    codex?.stop();
    interactive?.stop();
    if (codex) await codex.closed;
    if (interactive) await interactive.closed;
    rmSync(root, { recursive: true, force: true });
  }
});
