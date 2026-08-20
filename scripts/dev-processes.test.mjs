import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

import {
  backendProcess,
  cargoCommand,
  frontendProcess,
} from "./dev-processes.mjs";

const root = resolve(import.meta.dirname, "..");

function fixture(platform, profile = "debug") {
  const directory = mkdtempSync(resolve(tmpdir(), "eclipse-dev-process-test-"));
  const executable = resolve(
    directory,
    "target",
    profile,
    `eclipse${platform === "win32" ? ".exe" : ""}`,
  );
  mkdirSync(resolve(executable, ".."), { recursive: true });
  writeFileSync(executable, "fixture");
  if (platform !== "win32") chmodSync(executable, 0o755);
  return { directory, executable };
}

test("Windows resolves native executables instead of Unix scripts and command shims", () => {
  const item = fixture("win32");
  try {
    const backend = backendProcess({
      root: item.directory,
      env: {},
      platform: "win32",
    });
    const frontend = frontendProcess(root);

    assert.equal(cargoCommand("win32"), "cargo.exe");
    assert.equal(backend.command, item.executable);
    assert.equal(backend.options.cwd, item.directory);
    assert.equal(backend.options.env.ECLIPSE_BIND_ADDRESS, "127.0.0.1");
    assert.equal(frontend.command, process.execPath);
    assert.match(frontend.args[0], /[\\/]vite[\\/]bin[\\/]vite\.js$/);
    assert.deepEqual(frontend.args.slice(1), ["dev"]);
    assert.equal(frontend.options.cwd, resolve(root, "eclipse"));
    assert.doesNotMatch(frontend.command, /\.(?:cmd|bat)$/i);
  } finally {
    rmSync(item.directory, { recursive: true, force: true });
  }
});

test("backend target, runtime directory, environment, and release arguments are preserved", () => {
  const platform = process.platform;
  const item = fixture(platform, "release");
  try {
    const args = ["--bind-address", "127.0.0.1"];
    const backend = backendProcess({
      root: item.directory,
      release: true,
      args,
      env: { CARGO_TARGET_DIR: "target", PRESERVED: "yes" },
      platform,
    });

    assert.equal(backend.command, item.executable);
    assert.equal(backend.options.cwd, resolve(item.directory, "target/release"));
    assert.deepEqual(backend.args, args);
    assert.equal(backend.options.env.PRESERVED, "yes");
    assert.equal(backend.options.env.ECLIPSE_BIND_ADDRESS, "0.0.0.0");
  } finally {
    rmSync(item.directory, { recursive: true, force: true });
  }
});

test("resolved backend and frontend commands are directly spawnable", () => {
  const item = fixture(process.platform);
  try {
    copyFileSync(process.execPath, item.executable);
    if (process.platform !== "win32") chmodSync(item.executable, 0o755);
    const backend = backendProcess({ root: item.directory, args: ["--version"] });
    const backendResult = spawnSync(backend.command, backend.args, {
      ...backend.options,
      encoding: "utf8",
    });
    assert.equal(backendResult.status, 0, backendResult.stderr);
    assert.match(backendResult.stdout, /^v\d+/);

    const frontend = frontendProcess(root);
    const frontendResult = spawnSync(frontend.command, [frontend.args[0], "--version"], {
      ...frontend.options,
      encoding: "utf8",
    });
    assert.equal(frontendResult.status, 0, frontendResult.stderr);
    assert.match(frontendResult.stdout, /^vite\//);
  } finally {
    rmSync(item.directory, { recursive: true, force: true });
  }
});

test(
  "native Windows reproduces EFTYPE when the former run.sh child is spawned directly",
  { skip: process.platform !== "win32" },
  () => {
    assert.throws(
      () => spawn(resolve(root, "scripts/run.sh"), [], { stdio: "ignore" }),
      (error) => error?.code === "EFTYPE" && error?.syscall === "spawn",
    );
  },
);
