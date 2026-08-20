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
import { spawn, spawnSync } from "node:child_process";
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

function stopFixtureProcess(pid) {
  if (process.platform === "win32") {
    spawnSync(
      bash,
      ["-c", 'kill -TERM "$1" 2>/dev/null || true', "cleanup", String(pid)],
      { stdio: "ignore" },
    );
    return;
  }
  try {
    process.kill(pid, "SIGTERM");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

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
    `#!/usr/bin/env bash
set -euo pipefail
stage_file=""
recorded=()
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "--stage-file" ]]; then
    stage_file=$2
    shift 2
  else
    recorded+=("$1")
    shift
  fi
done
printf '%s\n' "\${recorded[*]}" > bootstrap.args
if [[ -f expected-stopped.pid ]]; then
  pid=$(cat expected-stopped.pid)
  if kill -0 "$pid" 2>/dev/null; then
    touch build.saw-running-process
    exit 90
  fi
fi
if [[ -f expected-stopped.marker ]]; then
  marker=$(cat expected-stopped.marker)
  if [[ -e "$marker" ]]; then
    touch build.saw-running-process
    exit 90
  fi
fi
for stage in "Installing frontend dependencies" "Building frontend" "Building Eclipse backend" "Preparing runtime"; do
  [[ -z "$stage_file" ]] || printf '%s\n' "$stage" > "$stage_file"
  sleep 0.03
done
`,
  );
  executable(resolve(repository, "scripts/run.sh"));
  executable(resolve(bin, "xcode-select"));
  executable(resolve(bin, "node"));
  executable(resolve(bin, "corepack"));
  executable(resolve(bin, "cargo"));
  executable(resolve(bin, "rustc"));
  executable(resolve(bin, "cc"));
  executable(resolve(bin, "c++"));
  executable(resolve(bin, "uname"), "#!/usr/bin/env bash\necho Darwin\n");
  executable(
    resolve(bin, "rustup"),
    '#!/usr/bin/env bash\nprintf "%s\\n" "$*" > "$INSTALL_FIXTURE/rustup.args"\n',
  );
  executable(
    resolve(bin, "ffmpeg"),
    '#!/usr/bin/env bash\necho "ffmpeg version 9.0 fixture"\n',
  );
  executable(
    resolve(bin, "ffprobe"),
    '#!/usr/bin/env bash\necho "ffprobe version 9.0 fixture"\n',
  );
  executable(resolve(bin, "sqlite3"));
  executable(resolve(bin, "pkg-config"));
  executable(
    resolve(bin, "open"),
    '#!/usr/bin/env bash\nprintf "%s\\n" "$1" > "$INSTALL_FIXTURE/opened.url"\n',
  );

  executable(
    resolve(repository, "utils/ffmpeg"),
    "#!/usr/bin/env bash\n# existing ffmpeg\n",
  );
  executable(
    resolve(repository, "utils/ffprobe"),
    "#!/usr/bin/env bash\n# existing ffprobe\n",
  );
  mkdirSync(resolve(repository, "target/release/config"));
  writeFileSync(
    resolve(repository, "target/release/config/config.toml"),
    "port = 8123\n# existing configuration\n",
  );
  writeFileSync(
    resolve(repository, "target/release/config/dim.db"),
    "accounts, libraries, scans, and progress\n",
  );
  writeFileSync(
    resolve(repository, "target/release/config/dim.db-journal"),
    "database recovery state\n",
  );
  writeFileSync(
    resolve(repository, "target/release/config/dim.db-wal"),
    "database write-ahead log\n",
  );
  writeFileSync(
    resolve(repository, "target/release/config/dim.db-shm"),
    "database shared memory\n",
  );
  writeFileSync(
    resolve(repository, "target/release/config/.config.toml.tmp-fixture"),
    "interrupted settings write\n",
  );
  mkdirSync(resolve(repository, "target/release/metadata"));
  mkdirSync(resolve(repository, "target/release/streaming_cache"));
  mkdirSync(resolve(repository, "target/release/logs"));
  writeFileSync(
    resolve(repository, "target/release/metadata/poster.jpg"),
    "metadata\n",
  );
  writeFileSync(
    resolve(repository, "target/release/streaming_cache/segment.m4s"),
    "cache\n",
  );
  writeFileSync(
    resolve(repository, "target/release/logs/eclipse.log"),
    "logs\n",
  );

  return {
    directory,
    repository,
    bin,
    env: {
      PATH: `${bashPath(bin)}:/usr/bin:/bin`,
      CARGO_HOME: bashPath(resolve(directory, "cargo-home")),
      INSTALL_FIXTURE: bashPath(repository),
      INSTALL_FIXTURE_BIN: bashPath(bin),
      NO_COLOR: "1",
    },
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function run(item, args, extraEnv = {}) {
  return spawnSync(
    bash,
    [
      "-c",
      'export PATH="$INSTALL_FIXTURE_BIN:$PATH"; exec ./install.sh "$@"',
      "install.sh",
      ...args,
    ],
    {
      cwd: item.repository,
      env: { ...process.env, ...item.env, ...extraEnv },
      encoding: "utf8",
    },
  );
}

function prepareWindows(item) {
  executable(
    resolve(item.bin, "uname"),
    "#!/usr/bin/env bash\necho MINGW64_NT-10.0\n",
  );
  writeFileSync(resolve(item.repository, "buildtools.ready"), "ready\n");
  executable(
    resolve(item.bin, "powershell.exe"),
    `#!/usr/bin/env bash
command_text="$*"
if [[ "$command_text" == *"Get-CimInstance Win32_Process"* ]]; then
  [[ -z "\${WINDOWS_PROCESS_TARGETS_LOG:-}" ]] || printf '%s\n' "\${ECLIPSE_PROCESS_TARGETS:-}" > "$WINDOWS_PROCESS_TARGETS_LOG"
  if [[ -n "\${WINDOWS_REQUIRED_PROCESS_TARGET:-}" && "\${ECLIPSE_PROCESS_TARGETS:-}" != *"$WINDOWS_REQUIRED_PROCESS_TARGET"* ]]; then
    exit 0
  fi
  if [[ -n "\${WINDOWS_ECLIPSE_PID_FILE:-}" && -f "$WINDOWS_ECLIPSE_PID_FILE" ]]; then
    pid=$(tr -dc '0-9' < "$WINDOWS_ECLIPSE_PID_FILE")
    printf '%s\r\n' "$pid"
  fi
  exit 0
elif [[ "$command_text" == *"ECLIPSE_CLEAN_TARGETS"* ]]; then
  if [[ -n "\${WINDOWS_LOCK_MARKER:-}" && -e "$WINDOWS_LOCK_MARKER" ]]; then
    IFS=';' read -ra locked_paths <<< "\${WINDOWS_LOCKED_PATHS:-\${WINDOWS_LOCKED_PATH:-unknown}}"
    for locked_path in "\${locked_paths[@]}"; do
      printf '%s: The process cannot access the file because it is being used by another process.\n' \
        "$locked_path" >&2
    done
    exit 1
  fi
  exit 0
elif [[ "$command_text" == *"Stop-Process -Force"* ]]; then
  [[ -z "\${WINDOWS_FORCE_STOP_LOG:-}" ]] || touch "$WINDOWS_FORCE_STOP_LOG"
  [[ -z "\${WINDOWS_LOCK_MARKER:-}" ]] || rm -f "$WINDOWS_LOCK_MARKER"
  [[ -z "\${WINDOWS_ECLIPSE_PID_FILE:-}" ]] || rm -f "$WINDOWS_ECLIPSE_PID_FILE"
  exit 0
elif [[ "$command_text" == *"prepare-pnpm.ps1"* ]]; then
  if [[ "\${WINDOWS_PNPM_PREPARE_FAIL:-}" == true ]]; then
    echo 'The user-level Corepack pnpm shim could not be prepared.' >&2
    exit 41
  fi
  touch "$INSTALL_FIXTURE/pnpm-prepared"
  echo 'ready|11.9.0|fixture-pnpm.cmd|prepared'
  exit 0
elif [[ -n "\${WINDOWS_TOOLCHAIN_RESULT:-}" ]]; then
  printf '%s\\n' "$WINDOWS_TOOLCHAIN_RESULT"
elif [[ -f "$INSTALL_FIXTURE/buildtools.ready" ]]; then
  echo 'ready|MSVC compiler and Windows SDK detected'
else
  echo 'missing-build-tools|No Visual Studio installation or MSVC compiler was found'
fi
`,
  );
}

function startSimulatedWindowsEclipse(item, { graceful = true } = {}) {
  const shutdown = bashPath(
    resolve(item.repository, "target/release/eclipse.shutdown"),
  );
  const pidFile = bashPath(resolve(item.repository, "windows-eclipse.pid"));
  const lock = bashPath(resolve(item.repository, "windows-database.lock"));
  const shutdownObservedPath = resolve(
    item.repository,
    "windows-shutdown-observed",
  );
  const shutdownObserved = bashPath(shutdownObservedPath);
  const source = graceful
    ? `
set -euo pipefail
shutdown=$1
pid_file=$2
lock=$3
shutdown_observed=$4
cleanup() { rm -f "$pid_file" "$lock"; }
trap cleanup EXIT
printf '%s\n' "$BASHPID" > "$pid_file"
touch "$lock"
while [[ ! -f "$shutdown" ]]; do /usr/bin/sleep 0.02; done
touch "$shutdown_observed"
`
    : `
set -euo pipefail
pid_file=$2
lock=$3
trap 'rm -f "$pid_file" "$lock"' EXIT
printf '%s\n' "$BASHPID" > "$pid_file"
touch "$lock"
while true; do /usr/bin/sleep 1; done
`;
  const processHandle = spawn(
    bash,
    [
      "-c",
      source,
      "windows-eclipse",
      shutdown,
      pidFile,
      lock,
      shutdownObserved,
    ],
    {
      cwd: item.repository,
      env: { ...process.env, ...item.env },
      stdio: "ignore",
    },
  );
  const ready = spawnSync(
    bash,
    [
      "-c",
      'for attempt in {1..100}; do [[ -f "$1" && -f "$2" ]] && exit 0; /usr/bin/sleep 0.01; done; exit 1',
      "wait-for-eclipse",
      pidFile,
      lock,
    ],
    { env: { ...process.env, ...item.env }, encoding: "utf8" },
  );
  assert.equal(ready.status, 0, "simulated Windows Eclipse did not start");
  return {
    processHandle,
    pidFile,
    lock,
    shutdownObserved: shutdownObservedPath,
    stop: () => {
      try {
        processHandle.kill("SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
    },
  };
}

test("Windows setup validates requirements and reuses the release bootstrap", () => {
  const item = fixture();
  try {
    prepareWindows(item);
    const result = run(item, ["--platform", "windows", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Windows selected/);
    assert.match(result.stdout, /pnpm command ready in new terminals/);
    assert.match(result.stdout, /System requirements ready/);
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /Existing configuration preserved/);
    assert.equal(
      readFileSync(resolve(item.repository, "bootstrap.args"), "utf8"),
      "--release\n",
    );
    assert.equal(existsSync(resolve(item.repository, "pnpm-prepared")), true);
  } finally {
    item.cleanup();
  }
});

test("Windows setup does not report readiness when persistent pnpm preparation fails", () => {
  const item = fixture();
  try {
    prepareWindows(item);
    const result = run(item, ["--platform", "windows", "--yes", "--no-start"], {
      WINDOWS_PNPM_PREPARE_FAIL: "true",
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /pnpm command ready in new terminals failed/);
    assert.match(
      result.stderr,
      /user-level Corepack pnpm shim could not be prepared/,
    );
    assert.doesNotMatch(result.stdout, /System requirements ready/);
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
  } finally {
    item.cleanup();
  }
});

test("Windows rejects non-native shells without suggesting WSL", () => {
  const item = fixture();
  try {
    const result = run(item, ["--platform", "windows", "--yes", "--no-start"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /install\.cmd from CMD or PowerShell/);
    assert.match(result.stderr, /WSL is not required or supported/);
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
  } finally {
    item.cleanup();
  }
});

test("Windows recovers supported requirements through exact WinGet packages", () => {
  const item = fixture();
  try {
    prepareWindows(item);
    rmSync(resolve(item.repository, "buildtools.ready"));
    executable(
      resolve(item.bin, "winget"),
      `#!/usr/bin/env bash
printf '%s\n' "$*" >> "$INSTALL_FIXTURE/winget.args"
case " $* " in
  *" Gyan.FFmpeg "*)
    printf '#!/usr/bin/env bash\necho "ffmpeg version 9.0 fixture"\n' > "$INSTALL_FIXTURE_BIN/ffmpeg"
    cp "$INSTALL_FIXTURE_BIN/ffmpeg" "$INSTALL_FIXTURE_BIN/ffprobe"
    chmod +x "$INSTALL_FIXTURE_BIN/ffmpeg" "$INSTALL_FIXTURE_BIN/ffprobe"
    ;;
  *" SQLite.SQLite "*)
    printf '#!/usr/bin/env bash\nexit 0\n' > "$INSTALL_FIXTURE_BIN/sqlite3"
    chmod +x "$INSTALL_FIXTURE_BIN/sqlite3"
    ;;
  *" OpenJS.NodeJS.LTS "*)
    printf '#!/usr/bin/env bash\nexit 0\n' > "$INSTALL_FIXTURE_BIN/node"
    chmod +x "$INSTALL_FIXTURE_BIN/node"
    ;;
  *" Microsoft.VisualStudio.2022.BuildTools "*) touch "$INSTALL_FIXTURE/buildtools.ready" ;;
esac
`,
    );
    rmSync(resolve(item.bin, "ffmpeg"));
    rmSync(resolve(item.bin, "ffprobe"));
    executable(resolve(item.bin, "node"), "#!/usr/bin/env bash\nexit 1\n");

    const result = run(item, ["--platform", "windows", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Install missing Windows packages/);
    assert.match(result.stdout, /Installed Windows requirements/);
    const winget = readFileSync(
      resolve(item.repository, "winget.args"),
      "utf8",
    );
    assert.match(winget, /--id Gyan\.FFmpeg --exact/);
    // On native Windows the installer deliberately refreshes the standard Node path, where the
    // test runner's supported setup-node installation is visible. Unix hosts simulating Windows
    // remain isolated and exercise the exact Node recovery package here.
    if (process.platform !== "win32") {
      assert.match(
        winget,
        /--id OpenJS\.NodeJS\.LTS --exact --version 24\.19\.0/,
      );
    }
    assert.match(
      winget,
      /--id Microsoft\.VisualStudio\.2022\.BuildTools --exact/,
    );
    assert.match(winget, /Microsoft\.VisualStudio\.Workload\.VCTools/);
  } finally {
    item.cleanup();
  }
});

test("Windows starts Eclipse, waits for readiness, and uses the native browser command", () => {
  const item = fixture();
  try {
    prepareWindows(item);
    executable(
      resolve(item.repository, "scripts/run.sh"),
      '#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/ready"\nexec /bin/sleep 300\n',
    );
    executable(
      resolve(item.bin, "curl"),
      '#!/usr/bin/env bash\n[[ -f "$INSTALL_FIXTURE/ready" ]]\n',
    );
    executable(
      resolve(item.bin, "cmd.exe"),
      '#!/usr/bin/env bash\nprintf "%s\\n" "$*" > "$INSTALL_FIXTURE/cmd.args"\n',
    );
    const result = run(item, ["--platform", "windows", "--yes"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Eclipse started/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8123/);
    assert.match(
      readFileSync(resolve(item.repository, "cmd.args"), "utf8"),
      /\/c start  http:\/\/localhost:8123/,
    );
    const pid = Number(
      readFileSync(
        resolve(item.repository, "target/release/eclipse.pid"),
        "utf8",
      ),
    );
    stopFixtureProcess(pid);
  } finally {
    item.cleanup();
  }
});

test("Windows accepts a capable toolchain without invoking recovery", () => {
  const item = fixture();
  try {
    prepareWindows(item);
    executable(
      resolve(item.bin, "winget"),
      '#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/winget.invoked"\nexit 99\n',
    );
    const result = run(item, ["--platform", "windows", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.equal(existsSync(resolve(item.repository, "winget.invoked")), false);
  } finally {
    item.cleanup();
  }
});

for (const [status, expected] of [
  [
    "missing-vctools|Visual Studio detected, but the MSVC x64 compiler and VCTools component were not found",
    /Desktop development with C\+\+/,
  ],
  [
    "missing-sdk|MSVC compiler detected, but no usable Windows SDK headers and x64 libraries were found",
    /current Windows SDK/,
  ],
  [
    "inconclusive|Visual Studio components were registered, but the MSVC compiler could not be verified",
    /detection was inconclusive/,
  ],
]) {
  test(`Windows reports ${status.split("|")[0]} without automatic Visual Studio recovery`, () => {
    const item = fixture();
    try {
      prepareWindows(item);
      executable(
        resolve(item.bin, "winget"),
        '#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/winget.invoked"\nexit 99\n',
      );
      const result = run(
        item,
        ["--platform", "windows", "--yes", "--no-start"],
        { WINDOWS_TOOLCHAIN_RESULT: status },
      );
      assert.notEqual(result.status, 0);
      assert.match(result.stdout, expected);
      assert.match(result.stdout, /install\.cmd/);
      assert.doesNotMatch(result.stdout, /\/c\/Users|Reopen Git Bash/);
      assert.equal(
        existsSync(resolve(item.repository, "winget.invoked")),
        false,
      );
    } finally {
      item.cleanup();
    }
  });
}

test("Linux setup validates requirements and reuses the release bootstrap", () => {
  const item = fixture();
  try {
    executable(resolve(item.bin, "uname"), "#!/usr/bin/env bash\necho Linux\n");
    const result = run(item, ["--platform", "linux", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Linux selected/);
    assert.match(result.stdout, /System requirements ready/);
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /Existing configuration preserved/);
    assert.equal(
      readFileSync(resolve(item.repository, "bootstrap.args"), "utf8"),
      "--release\n",
    );
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
    executable(resolve(item.bin, "uname"), "#!/usr/bin/env bash\necho Linux\n");
    executable(
      resolve(item.bin, "sudo"),
      '#!/usr/bin/env bash\nif [[ "${1:-}" == "-v" ]]; then exit 0; fi\nif [[ "${1:-}" == "-n" ]]; then shift; fi\nexec "$@"\n',
    );
    executable(
      resolve(item.bin, "apt-get"),
      `#!/usr/bin/env bash
printf '%s\n' "$*" >> "$INSTALL_FIXTURE/apt.args"
if [[ " $* " == *" install "* ]]; then
  printf '#!/usr/bin/env bash\necho "ffmpeg version 9.0 fixture"\n' > "$INSTALL_FIXTURE_BIN/ffmpeg"
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
    assert.match(
      result.stdout,
      /Install missing Debian\/Ubuntu packages \(ffmpeg pkg-config libssl-dev\)/,
    );
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
    executable(resolve(item.bin, "uname"), "#!/usr/bin/env bash\necho Linux\n");
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
    executable(resolve(item.bin, "uname"), "#!/usr/bin/env bash\necho Linux\n");
    executable(
      resolve(item.bin, "node"),
      "#!/usr/bin/env bash\necho v22.0.0\nexit 1\n",
    );
    const result = run(item, ["--platform", "linux", "--yes", "--no-start"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Some requirements still need attention/);
    assert.match(result.stdout, /Node\.js 24\.19\.0 or newer/);
    assert.doesNotMatch(
      `${result.stdout}\n${result.stderr}`,
      /unbound variable/,
    );
  } finally {
    item.cleanup();
  }
});

test("macOS setup reuses the release bootstrap and preserves user-owned files", () => {
  const item = fixture();
  try {
    const config = resolve(
      item.repository,
      "target/release/config/config.toml",
    );
    const ffmpeg = resolve(item.repository, "utils/ffmpeg");
    const result = run(item, ["--platform", "macos", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.equal(
      readFileSync(resolve(item.repository, "bootstrap.args"), "utf8"),
      "--release\n",
    );
    assert.match(
      readFileSync(resolve(item.repository, "rustup.args"), "utf8"),
      /toolchain install 1\.93\.1.*--profile minimal/,
    );
    assert.equal(
      readFileSync(config, "utf8"),
      "port = 8123\n# existing configuration\n",
    );
    assert.equal(
      readFileSync(ffmpeg, "utf8"),
      "#!/usr/bin/env bash\n# existing ffmpeg\n",
    );
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
    assert.equal(
      existsSync(resolve(item.repository, "target/release/eclipse.pid")),
      false,
    );
  } finally {
    item.cleanup();
  }
});

test(
  "an invalid existing media tool is reported and never replaced",
  {
    skip: process.platform === "win32",
  },
  () => {
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
  },
);

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
      readFileSync(
        resolve(item.repository, "target/release/eclipse.pid"),
        "utf8",
      ),
    );
    stopFixtureProcess(pid);
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
    const config = resolve(
      item.repository,
      "target/release/config/config.toml",
    );
    const ffmpeg = resolve(item.repository, "utils/ffmpeg");
    const configBefore = readFileSync(config, "utf8");
    const ffmpegBefore = readFileSync(ffmpeg, "utf8");
    const result = run(item, ["--demo", "--platform", "macos", "--yes"]);

    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(
      result.stdout,
      /Demo mode — all checks and actions are simulated/,
    );
    assert.match(result.stdout, /Found 2 missing or unsupported requirement/);
    assert.match(
      result.stdout,
      /Install missing Homebrew packages \(ffmpeg pkg-config\)/,
    );
    assert.match(result.stdout, /System requirements ready/);
    assert.doesNotMatch(
      result.stdout,
      /Git, Node 24, Corepack, Rust, FFmpeg\/FFprobe, SQLite/,
    );
    assert.match(result.stdout, /Eclipse installed/);
    assert.doesNotMatch(result.stdout, /Existing configuration preserved/);
    for (const stage of [
      "Installing frontend dependencies",
      "Building frontend",
      "Building Eclipse backend… 01:42",
      "Preparing runtime",
    ]) {
      assert.ok(result.stdout.includes(stage));
      assert.ok(
        result.stdout.indexOf(stage) <
          result.stdout.indexOf("✓ Eclipse installed"),
      );
    }
    assert.match(result.stdout, /Eclipse is ready\./);
    assert.match(result.stdout, /▶ Start Eclipse\n  Exit/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8000/);
    assert.match(result.stdout, /Opened Eclipse/);
    assert.ok(
      result.stdout.indexOf("✓ Eclipse installed") <
        result.stdout.indexOf("Eclipse is ready."),
    );
    assert.ok(
      result.stdout.indexOf("Eclipse is ready.") <
        result.stdout.indexOf("▶ Start Eclipse"),
    );
    assert.ok(
      result.stdout.indexOf("▶ Start Eclipse") <
        result.stdout.indexOf("Starting Eclipse"),
    );
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
    assert.match(
      result.stdout,
      /Install missing Debian\/Ubuntu packages \(ffmpeg pkg-config\)/,
    );
    assert.match(result.stdout, /System requirements ready/);
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /▶ Start Eclipse\n  Exit/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8000/);
    assert.equal(existsSync(resolve(item.repository, "apt.invoked")), false);
    assert.equal(
      existsSync(resolve(item.repository, "xdg-open.invoked")),
      false,
    );
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
  } finally {
    item.cleanup();
  }
});

test("Windows demo exercises WinGet recovery and launch without Windows actions", () => {
  const item = fixture();
  try {
    for (const command of ["winget", "powershell.exe", "cmd.exe"]) {
      executable(
        resolve(item.bin, command),
        `#!/usr/bin/env bash\ntouch "$INSTALL_FIXTURE/${command}.invoked"\nexit 99\n`,
      );
    }
    const result = run(item, ["--demo", "--platform", "windows", "--yes"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Windows selected/);
    assert.match(result.stdout, /Found 2 missing or unsupported requirement/);
    assert.match(
      result.stdout,
      /Install missing Windows packages \(Gyan\.FFmpeg Microsoft\.VisualStudio\.2022\.BuildTools\)/,
    );
    assert.match(result.stdout, /System requirements ready/);
    assert.match(result.stdout, /Eclipse installed/);
    assert.match(result.stdout, /▶ Start Eclipse\n  Exit/);
    assert.match(result.stdout, /Eclipse is ready: http:\/\/localhost:8000/);
    for (const marker of [
      "winget.invoked",
      "powershell.exe.invoked",
      "cmd.exe.invoked",
    ]) {
      assert.equal(existsSync(resolve(item.repository, marker)), false, marker);
    }
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
    assert.equal(existsSync(resolve(item.repository, "pnpm-prepared")), false);
  } finally {
    item.cleanup();
  }
});

test("fresh installation skips the existing-installation branch", () => {
  const item = fixture();
  try {
    for (const path of [
      "target/release/config",
      "target/release/metadata",
      "target/release/streaming_cache",
      "target/release/logs",
    ]) {
      rmSync(resolve(item.repository, path), { recursive: true, force: true });
    }

    const result = run(item, ["--platform", "macos", "--yes", "--no-start"]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.doesNotMatch(
      result.stdout,
      /Existing Eclipse installation detected/,
    );
    assert.match(result.stdout, /✓ Eclipse installed/);
    assert.doesNotMatch(result.stdout, /Existing configuration preserved/);
  } finally {
    item.cleanup();
  }
});

test("reinstall detects existing state and preserves every durable artifact", () => {
  const item = fixture();
  try {
    const durable = [
      "target/release/config/config.toml",
      "target/release/config/dim.db",
      "target/release/config/dim.db-journal",
      "target/release/config/dim.db-wal",
      "target/release/config/dim.db-shm",
      "target/release/metadata/poster.jpg",
    ];
    const before = durable.map((path) =>
      readFileSync(resolve(item.repository, path), "utf8"),
    );

    const result = run(item, [
      "--platform",
      "macos",
      "--yes",
      "--existing-action",
      "reinstall",
      "--no-start",
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Existing Eclipse installation detected/);
    assert.match(result.stdout, /▶ Reinstall \/ update Eclipse/);
    assert.match(result.stdout, /Existing configuration preserved/);
    assert.deepEqual(
      durable.map((path) =>
        readFileSync(resolve(item.repository, path), "utf8"),
      ),
      before,
    );
  } finally {
    item.cleanup();
  }
});

test("reinstall performs the one-time legacy executable migration", () => {
  const item = fixture();
  try {
    const legacy = resolve(item.repository, "target/release/dim");
    const database = resolve(item.repository, "target/release/config/dim.db");
    writeFileSync(legacy, "legacy executable\n");
    const before = readFileSync(database, "utf8");

    const result = run(item, [
      "--platform",
      "macos",
      "--yes",
      "--existing-action",
      "reinstall",
      "--no-start",
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.equal(existsSync(legacy), false);
    assert.equal(readFileSync(database, "utf8"), before);
  } finally {
    item.cleanup();
  }
});

test("reset removes settings and disposable runtime state but preserves accounts and metadata", () => {
  const item = fixture();
  try {
    const result = run(item, [
      "--platform",
      "macos",
      "--yes",
      "--existing-action",
      "reset",
      "--no-start",
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(
      result.stdout,
      /Reset removes host settings, streaming cache, and logs/,
    );
    assert.match(result.stdout, /existing browser sessions must sign in again/);
    assert.equal(
      existsSync(resolve(item.repository, "target/release/config/config.toml")),
      false,
    );
    assert.equal(
      existsSync(
        resolve(
          item.repository,
          "target/release/config/.config.toml.tmp-fixture",
        ),
      ),
      false,
    );
    assert.equal(
      readFileSync(
        resolve(item.repository, "target/release/config/dim.db"),
        "utf8",
      ),
      "accounts, libraries, scans, and progress\n",
    );
    assert.equal(
      readFileSync(
        resolve(item.repository, "target/release/config/dim.db-journal"),
        "utf8",
      ),
      "database recovery state\n",
    );
    assert.equal(
      readFileSync(
        resolve(item.repository, "target/release/metadata/poster.jpg"),
        "utf8",
      ),
      "metadata\n",
    );
    assert.equal(
      existsSync(resolve(item.repository, "target/release/streaming_cache")),
      false,
    );
    assert.equal(
      existsSync(resolve(item.repository, "target/release/logs")),
      false,
    );
  } finally {
    item.cleanup();
  }
});

test("clean install removes only managed state and produces a no-owner database state", () => {
  const item = fixture();
  try {
    const externalMetadata = resolve(item.directory, "external-metadata");
    mkdirSync(externalMetadata);
    writeFileSync(resolve(externalMetadata, "keep.jpg"), "external\n");
    writeFileSync(
      resolve(item.repository, "target/release/config/config.toml"),
      `port = 8123\nmetadata_dir = "${externalMetadata.replaceAll("\\", "/")}"\n`,
    );

    const result = run(item, [
      "--platform",
      "macos",
      "--yes",
      "--existing-action",
      "clean",
      "--no-start",
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    for (const path of [
      "target/release/config/config.toml",
      "target/release/config/.config.toml.tmp-fixture",
      "target/release/config/dim.db",
      "target/release/config/dim.db-journal",
      "target/release/metadata",
      "target/release/streaming_cache",
      "target/release/logs",
    ]) {
      assert.equal(existsSync(resolve(item.repository, path)), false, path);
    }
    assert.equal(
      readFileSync(resolve(externalMetadata, "keep.jpg"), "utf8"),
      "external\n",
    );
    assert.match(result.stdout, /Preserved externally configured path/);
    assert.match(result.stdout, /Preparing clean install/);
    assert.match(result.stdout, /Checking existing data/);
    assert.match(result.stdout, /Existing data ready for removal/);
    assert.match(result.stdout, /Removing existing Eclipse data/);
    assert.match(result.stdout, /Existing Eclipse data removed/);
    assert.doesNotMatch(result.stdout, /Stopping Eclipse/);
    assert.equal(existsSync(resolve(item.repository, "install.sh")), true);
    assert.equal(
      existsSync(resolve(item.repository, "scripts/bootstrap.sh")),
      true,
    );
    assert.match(result.stdout, /clean first run/);
  } finally {
    item.cleanup();
  }
});

test("Windows clean install waits for the exact legacy process before deleting locked state", () => {
  const item = fixture();
  let running;
  try {
    prepareWindows(item);
    assert.equal(
      existsSync(resolve(item.repository, "target/release/dim.exe")),
      false,
    );
    running = startSimulatedWindowsEclipse(item);
    const targetsLog = resolve(item.repository, "windows-process-targets.log");
    const forceLog = resolve(item.repository, "windows-force-stop.log");
    const unrelated = resolve(item.repository, "unrelated-process.marker");
    writeFileSync(unrelated, "must survive\n");
    writeFileSync(
      resolve(item.repository, "expected-stopped.marker"),
      `${running.lock}\n`,
    );

    const result = run(
      item,
      [
        "--platform",
        "windows",
        "--yes",
        "--existing-action",
        "clean",
        "--no-start",
      ],
      {
        WINDOWS_ECLIPSE_PID_FILE: running.pidFile,
        WINDOWS_LOCK_MARKER: running.lock,
        WINDOWS_LOCKED_PATHS:
          "C:\\fixture\\config\\dim.db;C:\\fixture\\config\\dim.db-wal;C:\\fixture\\config\\dim.db-shm",
        WINDOWS_REQUIRED_PROCESS_TARGET: "dim.exe",
        WINDOWS_PROCESS_TARGETS_LOG: bashPath(targetsLog),
        WINDOWS_FORCE_STOP_LOG: bashPath(forceLog),
      },
    );

    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Stopping Eclipse/);
    assert.match(result.stdout, /Eclipse stopped/);
    assert.match(result.stdout, /Checking existing data/);
    assert.match(result.stdout, /Existing data ready for removal/);
    assert.match(result.stdout, /Existing Eclipse data removed/);
    assert.equal(existsSync(running.shutdownObserved), true);
    assert.ok(
      result.stdout.indexOf("Eclipse stopped") <
        result.stdout.indexOf("Checking existing data"),
    );
    assert.ok(
      result.stdout.indexOf("Existing data ready for removal") <
        result.stdout.indexOf("Removing existing Eclipse data"),
    );
    assert.equal(
      existsSync(resolve(item.repository, "target/release/config/dim.db")),
      false,
    );
    assert.equal(
      existsSync(resolve(item.repository, "target/release/config/dim.db-wal")),
      false,
    );
    assert.equal(
      existsSync(resolve(item.repository, "target/release/config/dim.db-shm")),
      false,
    );
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), true);
    assert.equal(
      existsSync(resolve(item.repository, "build.saw-running-process")),
      false,
    );
    assert.match(readFileSync(targetsLog, "utf8"), /dim\.exe/i);
    assert.equal(existsSync(forceLog), false);
    assert.equal(readFileSync(unrelated, "utf8"), "must survive\n");
  } finally {
    running?.stop();
    item.cleanup();
  }
});

test("Windows clean install uses explicit verified force-stop after the graceful timeout", () => {
  const item = fixture();
  let running;
  let unrelatedProcess;
  try {
    prepareWindows(item);
    executable(resolve(item.bin, "sleep"));
    running = startSimulatedWindowsEclipse(item, { graceful: false });
    unrelatedProcess = spawn(bash, ["-c", "exec /usr/bin/sleep 300"], {
      stdio: "ignore",
    });
    const forceLog = resolve(item.repository, "windows-force-stop.log");
    const unrelated = resolve(item.repository, "unrelated-process.marker");
    writeFileSync(unrelated, "must survive\n");

    const result = run(
      item,
      [
        "--platform",
        "windows",
        "--yes",
        "--existing-action",
        "clean",
        "--no-start",
      ],
      {
        WINDOWS_ECLIPSE_PID_FILE: running.pidFile,
        WINDOWS_LOCK_MARKER: running.lock,
        WINDOWS_LOCKED_PATHS:
          "C:\\fixture\\config\\dim.db;C:\\fixture\\config\\dim.db-wal;C:\\fixture\\config\\dim.db-shm",
        WINDOWS_REQUIRED_PROCESS_TARGET: "dim.exe",
        WINDOWS_FORCE_STOP_LOG: bashPath(forceLog),
      },
    );

    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(
      result.stdout,
      /did not finish shutting down within 20 seconds/,
    );
    assert.match(
      result.stdout,
      /Force stop only the verified Eclipse process\(es\) for this installation\?/,
    );
    assert.equal(existsSync(forceLog), true);
    assert.equal(
      existsSync(resolve(item.repository, "target/release/config/dim.db")),
      false,
    );
    assert.equal(readFileSync(unrelated, "utf8"), "must survive\n");
    assert.doesNotThrow(() => process.kill(unrelatedProcess.pid, 0));
  } finally {
    running?.stop();
    try {
      unrelatedProcess?.kill("SIGKILL");
    } catch {}
    item.cleanup();
  }
});

test("Windows clean install reports a persistent lock before removing any state", () => {
  const item = fixture();
  try {
    prepareWindows(item);
    const lock = resolve(item.repository, "persistent-database.lock");
    const database = resolve(item.repository, "target/release/config/dim.db");
    const config = resolve(
      item.repository,
      "target/release/config/config.toml",
    );
    const beforeDatabase = readFileSync(database, "utf8");
    const beforeConfig = readFileSync(config, "utf8");
    writeFileSync(lock, "locked\n");

    const result = run(
      item,
      [
        "--platform",
        "windows",
        "--yes",
        "--existing-action",
        "clean",
        "--no-start",
      ],
      {
        WINDOWS_LOCK_MARKER: bashPath(lock),
        WINDOWS_LOCKED_PATHS:
          "C:\\fixture\\config\\dim.db;C:\\fixture\\config\\dim.db-wal;C:\\fixture\\config\\dim.db-shm",
      },
    );

    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /No exact Eclipse process could be verified, but existing Eclipse data is locked/,
    );
    assert.match(result.stderr, /C:\\fixture\\config\\dim\.db/);
    assert.match(result.stderr, /C:\\fixture\\config\\dim\.db-wal/);
    assert.match(result.stderr, /C:\\fixture\\config\\dim\.db-shm/);
    assert.match(result.stderr, /Process lifecycle diagnostic/);
    assert.match(result.stderr, /Full lifecycle log:/);
    assert.equal(readFileSync(database, "utf8"), beforeDatabase);
    assert.equal(readFileSync(config, "utf8"), beforeConfig);
    assert.equal(
      readFileSync(
        resolve(item.repository, "target/release/config/dim.db-wal"),
        "utf8",
      ),
      "database write-ahead log\n",
    );
    assert.equal(
      readFileSync(
        resolve(item.repository, "target/release/config/dim.db-shm"),
        "utf8",
      ),
      "database shared memory\n",
    );
    assert.equal(
      existsSync(
        resolve(item.repository, "target/release/metadata/poster.jpg"),
      ),
      true,
    );
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
    assert.doesNotMatch(result.stdout, /Existing Eclipse data removed/);
  } finally {
    item.cleanup();
  }
});

test(
  "declining verified force-stop leaves all clean-install state untouched",
  {
    skip:
      process.platform !== "linux" ||
      !existsSync("/usr/bin/script") ||
      !existsSync("/bin/sleep"),
  },
  () => {
    const item = fixture();
    let processHandle;
    try {
      executable(
        resolve(item.bin, "uname"),
        "#!/usr/bin/env bash\necho Linux\n",
      );
      executable(resolve(item.bin, "sleep"));
      const binary = resolve(item.repository, "target/release/eclipse");
      copyFileSync("/bin/sleep", binary);
      chmodSync(binary, 0o755);
      processHandle = spawn(
        bash,
        ["-c", 'trap "" TERM; exec "$1" 300', "eclipse", binary],
        { stdio: "ignore" },
      );
      const ready = spawnSync(
        "/bin/bash",
        [
          "-c",
          'for attempt in {1..100}; do [[ "$(readlink "/proc/$1/exe" 2>/dev/null)" == "$2" ]] && exit 0; /bin/sleep 0.01; done; exit 1',
          "wait-for-eclipse",
          String(processHandle.pid),
          binary,
        ],
        { encoding: "utf8" },
      );
      assert.equal(ready.status, 0, "force-stop fixture did not start");
      const database = resolve(item.repository, "target/release/config/dim.db");
      const before = readFileSync(database, "utf8");
      const command = `cd '${item.repository}' && export PATH="$INSTALL_FIXTURE_BIN:$PATH" && ./install.sh --platform linux --no-start`;
      const result = spawnSync(
        "/usr/bin/script",
        ["-q", "-e", "-c", command, "/dev/null"],
        {
          env: { ...process.env, ...item.env },
          input: "jj\ny\ny\nn\n",
          encoding: "utf8",
        },
      );

      assert.notEqual(result.status, 0);
      assert.match(result.stdout, /installation cannot safely continue/);
      assert.equal(readFileSync(database, "utf8"), before);
      assert.equal(
        existsSync(resolve(item.repository, "bootstrap.args")),
        false,
      );
      assert.equal(
        existsSync(resolve(item.repository, "pnpm-prepared")),
        false,
      );
      assert.doesNotThrow(() => process.kill(processHandle.pid, 0));
    } finally {
      if (processHandle?.pid) {
        try {
          process.kill(processHandle.pid, "SIGKILL");
        } catch {}
      }
      item.cleanup();
    }
  },
);

test("destructive automation requires explicit confirmation", () => {
  const item = fixture();
  try {
    const database = resolve(item.repository, "target/release/config/dim.db");
    const before = readFileSync(database, "utf8");
    const result = run(item, [
      "--platform",
      "macos",
      "--existing-action",
      "clean",
      "--no-start",
    ]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /requires --yes/);
    assert.equal(readFileSync(database, "utf8"), before);
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
  } finally {
    item.cleanup();
  }
});

test("Exit makes no changes and does not run requirements or installation", () => {
  const item = fixture();
  try {
    const database = resolve(item.repository, "target/release/config/dim.db");
    const before = readFileSync(database, "utf8");
    const result = run(item, [
      "--platform",
      "macos",
      "--yes",
      "--existing-action",
      "exit",
      "--no-start",
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /▶ Exit/);
    assert.match(result.stdout, /No changes made/);
    assert.equal(readFileSync(database, "utf8"), before);
    assert.equal(existsSync(resolve(item.repository, "bootstrap.args")), false);
    assert.equal(existsSync(resolve(item.repository, "rustup.args")), false);
  } finally {
    item.cleanup();
  }
});

test(
  "running Eclipse is stopped before the release executable can be replaced",
  { skip: process.platform === "win32" },
  () => {
    const item = fixture();
    let processHandle;
    try {
      const binary = resolve(item.repository, "target/release/eclipse");
      copyFileSync("/bin/sleep", binary);
      chmodSync(binary, 0o755);
      processHandle = spawn(binary, ["300"], { stdio: "ignore" });
      writeFileSync(
        resolve(item.repository, "expected-stopped.pid"),
        `${processHandle.pid}\n`,
      );

      const result = run(item, [
        "--platform",
        "macos",
        "--yes",
        "--existing-action",
        "reinstall",
        "--no-start",
      ]);
      assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
      assert.match(result.stdout, /Running Eclipse detected/);
      assert.match(result.stdout, /stopped before replacement/);
      assert.equal(
        existsSync(resolve(item.repository, "build.saw-running-process")),
        false,
      );
      assert.throws(() => process.kill(processHandle.pid, 0));
    } finally {
      if (processHandle?.pid) {
        try {
          process.kill(processHandle.pid, "SIGKILL");
        } catch {}
      }
      item.cleanup();
    }
  },
);

test("all existing-install demo branches are mutation-free", () => {
  for (const scenario of ["reinstall", "reset", "clean", "exit"]) {
    const item = fixture();
    try {
      const statePath = resolve(
        item.repository,
        "target/release/config/dim.db",
      );
      const before = readFileSync(statePath, "utf8");
      const result = run(item, [
        "--demo",
        "--demo-scenario",
        scenario,
        "--platform",
        "windows",
        "--yes",
        "--no-start",
      ]);
      assert.equal(
        result.status,
        0,
        `${scenario}\n${result.stdout}\n${result.stderr}`,
      );
      assert.match(result.stdout, /Existing Eclipse installation detected/);
      if (scenario !== "exit")
        assert.match(result.stdout, /Running Eclipse detected/);
      if (scenario === "clean") {
        assert.match(result.stdout, /Preparing clean install/);
        assert.match(result.stdout, /Stopping Eclipse/);
        assert.match(result.stdout, /Eclipse stopped/);
        assert.match(result.stdout, /Checking existing data/);
        assert.match(result.stdout, /Existing data ready for removal/);
        assert.match(result.stdout, /Existing Eclipse data removal simulated/);
      }
      assert.equal(readFileSync(statePath, "utf8"), before);
      assert.equal(
        existsSync(resolve(item.repository, "bootstrap.args")),
        false,
      );
      assert.equal(
        existsSync(resolve(item.repository, "pnpm-prepared")),
        false,
      );
      assert.equal(
        existsSync(resolve(item.repository, "target/release/eclipse.shutdown")),
        false,
      );
    } finally {
      item.cleanup();
    }
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
  assert.match(
    source,
    /select_menu "Which platform are you installing on\?" false "macOS" "Linux" "Windows"/,
  );
  assert.match(source, /select_menu "" true "Start Eclipse" "Exit"/);
  assert.match(source, /linux\)\s+command_available xdg-open && xdg-open/);
  assert.match(
    source,
    /windows\)\s+command_available cmd\.exe && MSYS2_ARG_CONV_EXCL='\*' cmd\.exe \/c start/,
  );
  assert.match(source, /sqlite\) package=SQLite\.SQLite/);
  assert.match(source, /▶/);
  assert.match(source, /printf '▶ %s\\n' "\$\{options\[\$index\]\}"/);
  assert.doesNotMatch(source, /printf '%s▶/);
  assert.doesNotMatch(source, /❯|Start Eclipse now\?/);
});
