import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, resolve } from "node:path";
import test from "node:test";

import { build, parseBuildArgs, validateMediaTool } from "./build.mjs";

const root = resolve(import.meta.dirname, "..");

function windowsFixture() {
  const directory = mkdtempSync(resolve(tmpdir(), "eclipse-build-test-"));
  const repository = resolve(directory, "repository");
  const bin = resolve(directory, "bin");
  const log = resolve(directory, "corepack.log");
  const stage = resolve(directory, "stage.txt");
  mkdirSync(bin, { recursive: true });
  mkdirSync(repository);
  writeFileSync(resolve(bin, "git.exe"), "git.exe fixture\n");
  const mediaToolSource = resolve(directory, "media-tool.rs");
  const mediaTool = resolve(directory, "media-tool.exe");
  writeFileSync(
    mediaToolSource,
    'fn main() { if std::env::args().nth(1).as_deref() != Some("-version") { std::process::exit(2); } let executable = std::env::current_exe().unwrap(); let name = executable.file_name().unwrap().to_string_lossy().to_ascii_lowercase(); let command = if name.starts_with("ffprobe") { "ffprobe" } else { "ffmpeg" }; println!("{command} version fixture 1"); }\n',
  );
  const compiled = spawnSync("rustc.exe", [mediaToolSource, "-o", mediaTool], {
    encoding: "utf8",
  });
  assert.equal(compiled.status, 0, compiled.stderr);
  copyFileSync(mediaTool, resolve(bin, "ffmpeg.exe"));
  copyFileSync(mediaTool, resolve(bin, "ffprobe.exe"));
  writeFileSync(
    resolve(bin, "corepack.cmd"),
    '@echo off\r\necho %*>>"%BUILD_TEST_LOG%"\r\nif "%BUILD_TEST_FAIL%"=="1" exit /b 37\r\n',
  );
  return {
    directory,
    repository,
    bin,
    mediaTool,
    log,
    stage,
    env: {
      ...process.env,
      PATH: bin,
      PATHEXT: ".EXE;.CMD",
      BUILD_TEST_LOG: log,
    },
  };
}

test("build arguments preserve debug, release, skip, and stage-file behavior", () => {
  assert.deepEqual(parseBuildArgs([]), {
    release: false,
    skipUi: false,
    skipRust: false,
    stageFile: "",
  });
  assert.deepEqual(
    parseBuildArgs([
      "--release",
      "--skip-ui",
      "--skip-rust",
      "--stage-file",
      "progress.txt",
    ]),
    {
      release: true,
      skipUi: true,
      skipRust: true,
      stageFile: "progress.txt",
    },
  );
});

test(
  "native Windows build validates, reuses offline, and repairs media tools",
  { skip: process.platform !== "win32" },
  async () => {
    const item = windowsFixture();
    try {
      await build({
        root: item.repository,
        args: ["--skip-rust", "--stage-file", item.stage],
        env: item.env,
        platform: "win32",
      });

      const commands = readFileSync(item.log, "utf8").replaceAll('"', "");
      assert.match(commands, /pnpm --dir eclipse install --frozen-lockfile/);
      assert.match(commands, /pnpm --dir eclipse build/);
      assert.equal(readFileSync(item.stage, "utf8"), "Complete\n");

      const ffmpeg = resolve(item.repository, "utils/ffmpeg.exe");
      const ffprobe = resolve(item.repository, "utils/ffprobe.exe");
      assert.equal(lstatSync(ffmpeg).isSymbolicLink(), false);
      assert.equal(lstatSync(ffprobe).isSymbolicLink(), false);
      assert.equal(validateMediaTool(ffmpeg).ok, true);
      assert.equal(validateMediaTool(ffprobe).ok, true);

      rmSync(resolve(item.bin, "ffmpeg.exe"));
      rmSync(resolve(item.bin, "ffprobe.exe"));

      await build({
        root: item.repository,
        args: ["--skip-rust"],
        env: item.env,
        platform: "win32",
      });
      assert.equal(validateMediaTool(ffmpeg).ok, true);
      assert.equal(validateMediaTool(ffprobe).ok, true);

      copyFileSync(item.mediaTool, resolve(item.bin, "ffmpeg.exe"));
      writeFileSync(ffmpeg, "broken cached executable\n");
      await build({
        root: item.repository,
        args: ["--skip-ui", "--skip-rust"],
        env: item.env,
        platform: "win32",
      });
      assert.equal(validateMediaTool(ffmpeg).ok, true);
    } finally {
      rmSync(item.directory, { recursive: true, force: true });
    }
  },
);

test(
  "Windows provisioning follows Scoop shim metadata and copies the real executable",
  { skip: process.platform !== "win32" },
  async () => {
    const item = windowsFixture();
    try {
      for (const command of ["ffmpeg", "ffprobe"]) {
        writeFileSync(
          resolve(item.bin, `${command}.exe`),
          "relocated shim cannot execute\n",
        );
        writeFileSync(
          resolve(item.bin, `${command}.shim`),
          `path = "${item.mediaTool}"\n`,
        );
      }

      await build({
        root: item.repository,
        args: ["--skip-ui", "--skip-rust"],
        env: item.env,
        platform: "win32",
      });

      assert.equal(
        validateMediaTool(resolve(item.repository, "utils/ffmpeg.exe")).ok,
        true,
      );
      assert.equal(
        validateMediaTool(resolve(item.repository, "utils/ffprobe.exe")).ok,
        true,
      );
    } finally {
      rmSync(item.directory, { recursive: true, force: true });
    }
  },
);

test(
  "macOS and Linux retain executable discovery and symlink behavior",
  { skip: process.platform === "win32" },
  async () => {
    const directory = mkdtempSync(
      resolve(tmpdir(), "eclipse-build-unix-test-"),
    );
    const repository = resolve(directory, "repository");
    const bin = resolve(directory, "bin");
    mkdirSync(bin, { recursive: true });
    mkdirSync(repository);
    for (const command of ["git", "ffmpeg", "ffprobe"]) {
      writeFileSync(resolve(bin, command), "fixture\n");
      chmodSync(resolve(bin, command), 0o755);
    }
    try {
      await build({
        root: repository,
        args: ["--skip-ui", "--skip-rust"],
        env: { ...process.env, PATH: bin },
        platform: process.platform,
      });
      assert.equal(
        lstatSync(resolve(repository, "utils/ffmpeg")).isSymbolicLink(),
        true,
      );
      assert.equal(
        lstatSync(resolve(repository, "utils/ffprobe")).isSymbolicLink(),
        true,
      );
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

test("build command failures retain their exit code and unknown options are diagnostic", async () => {
  if (process.platform === "win32") {
    const item = windowsFixture();
    try {
      await assert.rejects(
        build({
          root: item.repository,
          args: ["--skip-rust"],
          env: { ...item.env, BUILD_TEST_FAIL: "1" },
          platform: "win32",
        }),
        (error) =>
          error?.exitCode === 37 && /exited with code 37/.test(error.message),
      );
    } finally {
      rmSync(item.directory, { recursive: true, force: true });
    }
  }

  const result = spawnSync(
    process.execPath,
    ["./scripts/build.mjs", "--unknown"],
    {
      cwd: root,
      encoding: "utf8",
    },
  );
  assert.equal(result.status, 2);
  assert.match(result.stderr, /Unknown option: --unknown/);
  assert.match(result.stderr, /Usage: node \.\/scripts\/build\.mjs/);
});

test("public root tasks are cross-platform and shell entrypoints remain wrappers", () => {
  const manifest = JSON.parse(
    readFileSync(resolve(root, "package.json"), "utf8"),
  );
  const bootstrap = readFileSync(resolve(root, "scripts/bootstrap.sh"), "utf8");
  assert.equal(manifest.scripts.build, "node ./scripts/build.mjs");
  for (const [name, command] of Object.entries(manifest.scripts)) {
    assert.doesNotMatch(
      command,
      /(?:^|\s)\.?[/\\][^\s]*\.sh(?:\s|$)/,
      `root package script ${name} directly invokes a shell script`,
    );
  }
  assert.match(bootstrap, /exec node .*scripts\/build\.mjs.*"\$@"/);
  assert.match(
    readFileSync(resolve(root, "scripts/test.sh"), "utf8"),
    /exec node .*scripts\/task\.mjs.*test.*"\$@"/,
  );
  assert.match(
    readFileSync(resolve(root, "scripts/validate-release.sh"), "utf8"),
    /exec node .*scripts\/task\.mjs.*release-validate.*"\$@"/,
  );
});
