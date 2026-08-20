import assert from "node:assert/strict";
import {
  chmodSync,
  copyFileSync,
  lstatSync,
  mkdtempSync,
  mkdirSync,
  realpathSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const bash =
  process.platform === "win32"
    ? resolve(
        process.env.ProgramFiles ?? "C:/Program Files",
        "Git/bin/bash.exe",
      )
    : "/bin/bash";

function bashPath(path) {
  if (process.platform !== "win32") return path;
  const normalized = path.replaceAll("\\", "/");
  return `/${normalized[0].toLowerCase()}${normalized.slice(2)}`;
}

function executable(path, source = "#!/usr/bin/env bash\nexit 0\n") {
  writeFileSync(path, source);
  chmodSync(path, 0o755);
}

function fixture() {
  const directory = mkdtempSync(
    resolve(tmpdir(), "eclipse-windows-scripts-test-"),
  );
  const repository = resolve(directory, "repo");
  const bin = resolve(directory, "bin");
  mkdirSync(resolve(repository, "scripts"), { recursive: true });
  mkdirSync(bin);
  copyFileSync(
    resolve(root, "scripts/bootstrap.sh"),
    resolve(repository, "scripts/bootstrap.sh"),
  );
  copyFileSync(
    resolve(root, "scripts/run.sh"),
    resolve(repository, "scripts/run.sh"),
  );
  chmodSync(resolve(repository, "scripts/bootstrap.sh"), 0o755);
  chmodSync(resolve(repository, "scripts/run.sh"), 0o755);
  executable(
    resolve(bin, "uname"),
    "#!/usr/bin/env bash\necho MINGW64_NT-10.0\n",
  );
  executable(resolve(bin, "git"));
  executable(
    resolve(bin, "ffmpeg"),
    "#!/usr/bin/env bash\necho ffmpeg-fixture\n",
  );
  executable(
    resolve(bin, "ffprobe"),
    "#!/usr/bin/env bash\necho ffprobe-fixture\n",
  );

  return {
    directory,
    repository,
    bin,
    env: {
      ...process.env,
      PATH: `${bashPath(bin)}:/usr/bin:/bin`,
      WINDOWS_SCRIPT_FIXTURE: bashPath(directory),
      WINDOWS_SCRIPT_BIN: bashPath(bin),
    },
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function runBash(item, args) {
  return spawnSync(
    bash,
    [
      "-c",
      'export PATH="$WINDOWS_SCRIPT_BIN:$PATH"; exec "$@"',
      "bash",
      ...args,
    ],
    {
      cwd: item.repository,
      env: item.env,
      encoding: "utf8",
    },
  );
}

test("bootstrap remains an argument-forwarding compatibility wrapper", () => {
  const bootstrap = readFileSync(resolve(root, "scripts/bootstrap.sh"), "utf8");
  assert.match(bootstrap, /exec node .*scripts\/build\.mjs.*"\$@"/);
});

test("Windows run script launches eclipse.exe from the release runtime directory", () => {
  const item = fixture();
  try {
    const release = resolve(item.repository, "target/release");
    mkdirSync(release, { recursive: true });
    executable(
      resolve(release, "eclipse.exe"),
      `#!/usr/bin/env bash
pwd > "$WINDOWS_SCRIPT_FIXTURE/runtime.cwd"
printf '%s\\n' "$*" > "$WINDOWS_SCRIPT_FIXTURE/runtime.args"
`,
    );

    const result = runBash(item, [
      "./scripts/run.sh",
      "--release",
      "--bind-address",
      "127.0.0.1",
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const runtimeCwd = readFileSync(
      resolve(item.directory, "runtime.cwd"),
      "utf8",
    ).trim();
    if (process.platform === "win32")
      assert.match(runtimeCwd, /\/repo\/target\/release$/);
    else assert.equal(runtimeCwd, realpathSync(release));
    assert.equal(
      readFileSync(resolve(item.directory, "runtime.args"), "utf8"),
      "--bind-address 127.0.0.1\n",
    );
  } finally {
    item.cleanup();
  }
});

test("runtime scripts no longer assume a dim executable", () => {
  const build = readFileSync(resolve(root, "scripts/build.mjs"), "utf8");
  const run = readFileSync(resolve(root, "scripts/run.sh"), "utf8");
  assert.match(build, /`eclipse\$\{binarySuffix\}`/);
  assert.match(run, /ECLIPSE_PROFILE\/eclipse\$ECLIPSE_BINARY_SUFFIX/);
  assert.doesNotMatch(build, /release\/dim|dim\.exe/);
  assert.doesNotMatch(run, /\/dim\$DIM_BINARY_SUFFIX|\/dim\.exe/);
});
