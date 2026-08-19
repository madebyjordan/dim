import assert from "node:assert/strict";
import {
  chmodSync,
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

function executable(path, source = "#!/usr/bin/env bash\nexit 0\n") {
  writeFileSync(path, source);
  chmodSync(path, 0o755);
}

function fixture() {
  const directory = mkdtempSync(resolve(tmpdir(), "eclipse-install-test-"));
  const repository = resolve(directory, "repo");
  const bin = resolve(directory, "bin");
  mkdirSync(resolve(repository, "scripts"), { recursive: true });
  mkdirSync(resolve(repository, "utils"));
  mkdirSync(resolve(repository, "target/release/utils"), { recursive: true });
  mkdirSync(bin);
  copyFileSync(resolve(root, "install.sh"), resolve(repository, "install.sh"));
  chmodSync(resolve(repository, "install.sh"), 0o755);

  executable(
    resolve(repository, "scripts/bootstrap.sh"),
    '#!/usr/bin/env bash\nprintf "%s\\n" "$*" > bootstrap.args\n',
  );
  executable(resolve(repository, "scripts/run.sh"));
  executable(resolve(bin, "xcode-select"));
  executable(resolve(bin, "node"));
  executable(resolve(bin, "corepack"));
  executable(resolve(bin, "cargo"));
  executable(resolve(bin, "rustc"));
  executable(
    resolve(bin, "rustup"),
    '#!/usr/bin/env bash\nprintf "%s\\n" "$*" > "$INSTALL_FIXTURE/rustup.args"\n',
  );
  executable(
    resolve(bin, "ffmpeg"),
    '#!/usr/bin/env bash\necho "ffmpeg version 8.0 fixture"\n',
  );
  executable(
    resolve(bin, "ffprobe"),
    '#!/usr/bin/env bash\necho "ffprobe version 8.0 fixture"\n',
  );
  executable(resolve(bin, "sqlite3"));
  executable(resolve(bin, "pkg-config"));

  writeFileSync(resolve(repository, "utils/ffmpeg"), "existing ffmpeg\n");
  writeFileSync(resolve(repository, "utils/ffprobe"), "existing ffprobe\n");
  chmodSync(resolve(repository, "utils/ffmpeg"), 0o755);
  chmodSync(resolve(repository, "utils/ffprobe"), 0o755);
  mkdirSync(resolve(repository, "target/release/config"));
  writeFileSync(
    resolve(repository, "target/release/config/config.toml"),
    "port = 8123\n# existing configuration\n",
  );

  return {
    directory,
    repository,
    bin,
    env: {
      PATH: `${bin}:/usr/bin:/bin`,
      CARGO_HOME: resolve(directory, "cargo-home"),
      INSTALL_FIXTURE: repository,
      NO_COLOR: "1",
    },
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function run(item, args) {
  return spawnSync("/bin/bash", ["./install.sh", ...args], {
    cwd: item.repository,
    env: { ...process.env, ...item.env },
    encoding: "utf8",
  });
}

test("platform support states are explicit and do not bootstrap", () => {
  for (const [platform, message] of [
    ["linux", "Linux installer is not available"],
    ["windows", "Windows installation is not supported"],
  ]) {
    const item = fixture();
    try {
      const result = run(item, ["--platform", platform]);
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, new RegExp(message));
      assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
    } finally {
      item.cleanup();
    }
  }
});

test("macOS setup reuses the release bootstrap and preserves user-owned files", () => {
  const item = fixture();
  try {
    const config = resolve(item.repository, "target/release/config/config.toml");
    const ffmpeg = resolve(item.repository, "utils/ffmpeg");
    const result = run(item, ["--platform", "macos", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.equal(readFileSync(resolve(item.repository, "bootstrap.args"), "utf8"), "--release\n");
    assert.match(
      readFileSync(resolve(item.repository, "rustup.args"), "utf8"),
      /toolchain install 1\.93\.1.*--profile minimal/,
    );
    assert.equal(
      readFileSync(config, "utf8"),
      "port = 8123\n# existing configuration\n",
    );
    assert.equal(readFileSync(ffmpeg, "utf8"), "existing ffmpeg\n");
    assert.match(result.stdout, /Existing configuration found; it will be preserved/);
    assert.match(result.stdout, /scripts\/run\.sh --release/);
    assert.match(result.stdout, /http:\/\/localhost:8123/);
  } finally {
    item.cleanup();
  }
});

test("an invalid existing media tool is reported and never replaced", () => {
  const item = fixture();
  try {
    const ffprobe = resolve(item.repository, "utils/ffprobe");
    chmodSync(ffprobe, 0o644);
    const before = readFileSync(ffprobe, "utf8");
    const result = run(item, ["--platform", "macos", "--yes", "--no-start"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /already exists but is not executable/);
    assert.equal(readFileSync(ffprobe, "utf8"), before);
  } finally {
    item.cleanup();
  }
});

test("successful startup waits for readiness and opens the configured URL", () => {
  const item = fixture();
  try {
    executable(
      resolve(item.repository, "scripts/run.sh"),
      '#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/ready"\nexec /bin/sleep 300\n',
    );
    executable(
      resolve(item.bin, "curl"),
      '#!/usr/bin/env bash\n[[ -f "$INSTALL_FIXTURE/ready" ]]\n',
    );
    executable(
      resolve(item.bin, "open"),
      '#!/usr/bin/env bash\nprintf "%s\\n" "$1" > "$INSTALL_FIXTURE/opened.url"\n',
    );
    const result = run(item, ["--platform", "macos", "--yes"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Eclipse is running/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8123/);
    assert.equal(
      readFileSync(resolve(item.repository, "opened.url"), "utf8"),
      "http://localhost:8123\n",
    );
    const pid = Number(
      readFileSync(resolve(item.repository, "target/release/eclipse.pid"), "utf8"),
    );
    process.kill(pid, "SIGTERM");
  } finally {
    item.cleanup();
  }
});

test("missing Homebrew requirements give a concrete recovery path", () => {
  const item = fixture();
  try {
    rmSync(resolve(item.bin, "ffmpeg"));
    rmSync(resolve(item.bin, "ffprobe"));
    rmSync(resolve(item.bin, "pkg-config"));
    const result = run(item, ["--platform", "macos", "--yes", "--no-start"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Homebrew is not installed/);
    assert.match(result.stdout, /https:\/\/brew\.sh/);
    assert.match(result.stdout, /Homebrew package: ffmpeg/);
    assert.match(result.stdout, /install\.sh/);
  } finally {
    item.cleanup();
  }
});

test("the default entrypoint begins with the platform selector", () => {
  const source = readFileSync(resolve(root, "install.sh"), "utf8");
  const setup = source.lastIndexOf("printf '\\n%sEclipse Setup%s");
  const selector = source.indexOf("select_platform", setup);
  const dispatch = source.indexOf('case "$ECLIPSE_SELECTED_PLATFORM"', setup);
  assert.ok(setup >= 0);
  assert.ok(selector > setup);
  assert.ok(dispatch > selector);
  assert.match(source, /local options=\("macOS" "Linux" "Windows"\)/);
  assert.match(source, /❯/);
});
