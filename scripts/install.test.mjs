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
  executable(resolve(bin, "cc"));
  executable(resolve(bin, "c++"));
  executable(resolve(bin, "uname"), '#!/usr/bin/env bash\necho Darwin\n');
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
  executable(
    resolve(bin, "open"),
    '#!/usr/bin/env bash\nprintf "%s\\n" "$1" > "$INSTALL_FIXTURE/opened.url"\n',
  );

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
      INSTALL_FIXTURE_BIN: bin,
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

test("Windows support state is explicit and does not bootstrap", () => {
  const item = fixture();
  try {
    const result = run(item, ["--platform", "windows"]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Windows installation is not supported/);
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
  } finally {
    item.cleanup();
  }
});

test("Linux setup validates requirements and reuses the release bootstrap", () => {
  const item = fixture();
  try {
    executable(resolve(item.bin, "uname"), '#!/usr/bin/env bash\necho Linux\n');
    const result = run(item, ["--platform", "linux", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Linux selected/);
    assert.match(result.stdout, /System requirements ready/);
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /Existing configuration preserved/);
    assert.equal(readFileSync(resolve(item.repository, "bootstrap.args"), "utf8"), "--release\n");
    assert.match(
      readFileSync(resolve(item.repository, "rustup.args"), "utf8"),
      /toolchain install 1\.93\.1.*--profile minimal/,
    );
  } finally {
    item.cleanup();
  }
});

test("Linux recovers documented native packages with apt-get", () => {
  const item = fixture();
  try {
    executable(resolve(item.bin, "uname"), '#!/usr/bin/env bash\necho Linux\n');
    executable(
      resolve(item.bin, "sudo"),
      '#!/usr/bin/env bash\nif [[ "${1:-}" == "-v" ]]; then exit 0; fi\nif [[ "${1:-}" == "-n" ]]; then shift; fi\nexec "$@"\n',
    );
    executable(
      resolve(item.bin, "apt-get"),
      `#!/usr/bin/env bash
printf '%s\n' "$*" >> "$INSTALL_FIXTURE/apt.args"
if [[ " $* " == *" install "* ]]; then
  printf '#!/usr/bin/env bash\necho "ffmpeg version 8.0 fixture"\n' > "$INSTALL_FIXTURE_BIN/ffmpeg"
  cp "$INSTALL_FIXTURE_BIN/ffmpeg" "$INSTALL_FIXTURE_BIN/ffprobe"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$INSTALL_FIXTURE_BIN/pkg-config"
  chmod +x "$INSTALL_FIXTURE_BIN/ffmpeg" "$INSTALL_FIXTURE_BIN/ffprobe" "$INSTALL_FIXTURE_BIN/pkg-config"
fi
`,
    );
    rmSync(resolve(item.bin, "ffmpeg"));
    rmSync(resolve(item.bin, "ffprobe"));
    rmSync(resolve(item.bin, "pkg-config"));

    const result = run(item, ["--platform", "linux", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Install missing Debian\/Ubuntu packages \(ffmpeg pkg-config libssl-dev\)/);
    assert.match(result.stdout, /Installed Linux requirements/);
    const apt = readFileSync(resolve(item.repository, "apt.args"), "utf8");
    assert.match(apt, /^update$/m);
    assert.match(apt, /install -y ffmpeg pkg-config libssl-dev/);
  } finally {
    item.cleanup();
  }
});

test("Linux gives distribution-specific guidance when apt-get is unavailable", () => {
  const item = fixture();
  try {
    executable(resolve(item.bin, "uname"), '#!/usr/bin/env bash\necho Linux\n');
    rmSync(resolve(item.bin, "ffmpeg"));
    rmSync(resolve(item.bin, "ffprobe"));
    rmSync(resolve(item.bin, "pkg-config"));
    const result = run(item, ["--platform", "linux", "--yes", "--no-start"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /supports Debian and Ubuntu/);
    assert.match(result.stdout, /Debian\/Ubuntu package: ffmpeg/);
    assert.match(result.stdout, /Debian\/Ubuntu package: libssl-dev/);
  } finally {
    item.cleanup();
  }
});

test("Linux reports a lone unsupported Node version without invoking apt-get", () => {
  const item = fixture();
  try {
    executable(resolve(item.bin, "uname"), '#!/usr/bin/env bash\necho Linux\n');
    executable(resolve(item.bin, "node"), '#!/usr/bin/env bash\necho v22.0.0\nexit 1\n');
    const result = run(item, ["--platform", "linux", "--yes", "--no-start"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Some requirements still need attention/);
    assert.match(result.stdout, /Node\.js 24\.19\.0 or newer/);
    assert.doesNotMatch(`${result.stdout}\n${result.stderr}`, /unbound variable/);
  } finally {
    item.cleanup();
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
    assert.match(result.stdout, /System requirements ready/);
    assert.doesNotMatch(
      result.stdout,
      /Git, Node 24, Corepack, Rust, FFmpeg\/FFprobe, SQLite/,
    );
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /Existing configuration preserved/);
    assert.match(result.stdout, /Eclipse is ready\./);
    assert.match(result.stdout, /scripts\/run\.sh --release/);
    assert.match(result.stdout, /Eclipse was not started/);
    assert.equal(existsSync(resolve(item.repository, "target/release/eclipse.pid")), false);
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
    assert.match(result.stdout, /▶ Start Eclipse\n  Exit/);
    assert.match(result.stdout, /Eclipse started/);
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

test("demo mode exercises the complete flow without performing actions", () => {
  const item = fixture();
  try {
    const config = resolve(item.repository, "target/release/config/config.toml");
    const ffmpeg = resolve(item.repository, "utils/ffmpeg");
    const configBefore = readFileSync(config, "utf8");
    const ffmpegBefore = readFileSync(ffmpeg, "utf8");
    const result = run(item, ["--demo", "--platform", "macos", "--yes"]);

    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Demo mode — all checks and actions are simulated/);
    assert.match(result.stdout, /Found 2 missing or unsupported requirement/);
    assert.match(result.stdout, /Install missing Homebrew packages \(ffmpeg pkg-config\)/);
    assert.match(result.stdout, /System requirements ready/);
    assert.doesNotMatch(
      result.stdout,
      /Git, Node 24, Corepack, Rust, FFmpeg\/FFprobe, SQLite/,
    );
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /Existing configuration preserved/);
    assert.match(result.stdout, /Eclipse is ready\./);
    assert.match(result.stdout, /▶ Start Eclipse\n  Exit/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8000/);
    assert.match(result.stdout, /Opened Eclipse/);
    assert.ok(result.stdout.indexOf("✓ Eclipse installed") < result.stdout.indexOf("Eclipse is ready."));
    assert.ok(result.stdout.indexOf("Eclipse is ready.") < result.stdout.indexOf("▶ Start Eclipse"));
    assert.ok(result.stdout.indexOf("▶ Start Eclipse") < result.stdout.indexOf("Starting Eclipse"));
    assert.equal(readFileSync(config, "utf8"), configBefore);
    assert.equal(readFileSync(ffmpeg, "utf8"), ffmpegBefore);
    for (const path of [
      "bootstrap.args",
      "rustup.args",
      "ready",
      "opened.url",
      "target/release/eclipse.pid",
      "target/release/eclipse.log",
    ]) {
      assert.equal(existsSync(resolve(item.repository, path)), false, path);
    }
  } finally {
    item.cleanup();
  }
});

test("Linux demo exercises apt recovery and the shared launch flow without actions", () => {
  const item = fixture();
  try {
    executable(
      resolve(item.bin, "apt-get"),
      '#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/apt.invoked"\nexit 99\n',
    );
    executable(
      resolve(item.bin, "xdg-open"),
      '#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/xdg-open.invoked"\nexit 99\n',
    );
    const result = run(item, ["--demo", "--platform", "linux", "--yes"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Linux selected/);
    assert.match(result.stdout, /Install missing Debian\/Ubuntu packages \(ffmpeg pkg-config\)/);
    assert.match(result.stdout, /System requirements ready/);
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /▶ Start Eclipse\n  Exit/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8000/);
    assert.equal(existsSync(resolve(item.repository, "apt.invoked")), false);
    assert.equal(existsSync(resolve(item.repository, "xdg-open.invoked")), false);
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
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
  assert.match(source, /select_menu "Which platform are you installing on\?" false "macOS" "Linux" "Windows"/);
  assert.match(source, /select_menu "" true "Start Eclipse" "Exit"/);
  assert.match(source, /linux\)\s+command_available xdg-open && xdg-open/);
  assert.match(source, /▶/);
  assert.match(source, /printf '▶ %s\\n' "\$\{options\[\$index\]\}"/);
  assert.doesNotMatch(source, /printf '%s▶/);
  assert.doesNotMatch(source, /❯|Start Eclipse now\?/);
});
