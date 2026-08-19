import assert from "node:assert/strict";
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");

test("native Windows launcher delegates to the shared installer and has Git recovery", () => {
  const source = readFileSync(resolve(root, "install.cmd"), "utf8");
  assert.match(
    source,
    /"%ECLIPSE_BASH%" "%ECLIPSE_INSTALL_ROOT%install\.sh" %\*/,
  );
  assert.match(source, /call winget install --id Git\.Git --exact/);
  assert.match(
    source,
    /Demo mode will not install Git or make any system changes/,
  );
  assert.match(source, /CMD or PowerShell/);
  assert.doesNotMatch(source, /WSL|\/c\/Users/);
});

function windowsFixture() {
  const directory = mkdtempSync(
    resolve(tmpdir(), "eclipse-windows-launcher-test-"),
  );
  const repository = resolve(directory, "repo");
  const bin = resolve(directory, "bin");
  mkdirSync(repository);
  mkdirSync(bin);
  copyFileSync(
    resolve(root, "install.cmd"),
    resolve(repository, "install.cmd"),
  );
  writeFileSync(
    resolve(repository, "install.sh"),
    "shared installer fixture\n",
  );
  return {
    directory,
    repository,
    bin,
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function fakeBash(path) {
  writeFileSync(
    path,
    '@echo off\r\n> "%LAUNCH_CAPTURE%" echo %*\r\nexit /b 0\r\n',
  );
}

test(
  "install.cmd forwards arguments from CMD to the shared installer",
  { skip: process.platform !== "win32" },
  () => {
    const item = windowsFixture();
    try {
      const bash = resolve(item.directory, "bash.cmd");
      const capture = resolve(item.directory, "launch.args");
      fakeBash(bash);
      const result = spawnSync(
        "cmd.exe",
        ["/d", "/c", "install.cmd", "--demo", "--platform", "windows"],
        {
          cwd: item.repository,
          env: {
            ...process.env,
            ECLIPSE_BASH_EXE: bash,
            LAUNCH_CAPTURE: capture,
          },
          encoding: "utf8",
        },
      );
      assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
      const forwarded = readFileSync(capture, "utf8");
      assert.match(forwarded, /install\.sh.*--demo --platform windows/i);
    } finally {
      item.cleanup();
    }
  },
);

test(
  "install.cmd forwards arguments when invoked from PowerShell",
  { skip: process.platform !== "win32" },
  () => {
    const item = windowsFixture();
    try {
      const bash = resolve(item.directory, "bash.cmd");
      const capture = resolve(item.directory, "launch.args");
      fakeBash(bash);
      const result = spawnSync(
        "powershell.exe",
        [
          "-NoProfile",
          "-Command",
          "& .\\install.cmd --demo --platform windows",
        ],
        {
          cwd: item.repository,
          env: {
            ...process.env,
            ECLIPSE_BASH_EXE: bash,
            LAUNCH_CAPTURE: capture,
          },
          encoding: "utf8",
        },
      );
      assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
      assert.match(readFileSync(capture, "utf8"), /--demo --platform windows/);
    } finally {
      item.cleanup();
    }
  },
);

test(
  "install.cmd bootstraps missing Git with WinGet and continues",
  { skip: process.platform !== "win32" },
  () => {
    const item = windowsFixture();
    try {
      const bash = resolve(item.directory, "installed-bash.cmd");
      const template = resolve(item.directory, "bash-template.cmd");
      const capture = resolve(item.directory, "launch.args");
      const wingetCapture = resolve(item.directory, "winget.args");
      fakeBash(template);
      writeFileSync(
        resolve(item.bin, "winget.cmd"),
        '@echo off\r\n> "%WINGET_CAPTURE%" echo %*\r\ncopy /y "%FAKE_BASH_TEMPLATE%" "%ECLIPSE_BASH_EXE%" >nul\r\nexit /b 0\r\n',
      );
      const result = spawnSync(
        "cmd.exe",
        ["/d", "/c", "install.cmd", "--yes", "--platform", "windows"],
        {
          cwd: item.repository,
          env: {
            ...process.env,
            PATH: `${item.bin};${process.env.PATH}`,
            ECLIPSE_BASH_EXE: bash,
            ECLIPSE_SKIP_GIT_DISCOVERY: "1",
            FAKE_BASH_TEMPLATE: template,
            LAUNCH_CAPTURE: capture,
            WINGET_CAPTURE: wingetCapture,
          },
          encoding: "utf8",
        },
      );
      assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
      assert.match(
        readFileSync(wingetCapture, "utf8"),
        /--id Git\.Git --exact/,
      );
      assert.equal(existsSync(capture), true);
    } finally {
      item.cleanup();
    }
  },
);

test(
  "install.cmd demo never bootstraps missing Git",
  { skip: process.platform !== "win32" },
  () => {
    const item = windowsFixture();
    try {
      const marker = resolve(item.directory, "winget.invoked");
      writeFileSync(
        resolve(item.bin, "winget.cmd"),
        `@echo off\r\ntype nul > "${marker}"\r\nexit /b 99\r\n`,
      );
      const result = spawnSync(
        "cmd.exe",
        ["/d", "/c", "install.cmd", "--demo"],
        {
          cwd: item.repository,
          env: {
            ...process.env,
            PATH: `${item.bin};${process.env.PATH}`,
            ECLIPSE_BASH_EXE: resolve(item.directory, "missing-bash.exe"),
            ECLIPSE_SKIP_GIT_DISCOVERY: "1",
          },
          encoding: "utf8",
        },
      );
      assert.notEqual(result.status, 0);
      assert.match(result.stdout, /will not install Git/);
      assert.equal(existsSync(marker), false);
    } finally {
      item.cleanup();
    }
  },
);
