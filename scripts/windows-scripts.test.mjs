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
      PATH: `${bin}:/usr/bin:/bin`,
      WINDOWS_SCRIPT_FIXTURE: directory,
    },
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

test("Windows bootstrap copies media executables and preserves existing copies", () => {
  const item = fixture();
  try {
    const first = spawnSync(
      "/bin/bash",
      ["./scripts/bootstrap.sh", "--skip-ui", "--skip-rust"],
      {
        cwd: item.repository,
        env: item.env,
        encoding: "utf8",
      },
    );
    assert.equal(first.status, 0, `${first.stdout}\n${first.stderr}`);

    const ffmpeg = resolve(item.repository, "utils/ffmpeg.exe");
    const ffprobe = resolve(item.repository, "utils/ffprobe.exe");
    assert.equal(lstatSync(ffmpeg).isSymbolicLink(), false);
    assert.equal(lstatSync(ffprobe).isSymbolicLink(), false);
    assert.equal(
      readFileSync(ffmpeg, "utf8"),
      "#!/usr/bin/env bash\necho ffmpeg-fixture\n",
    );
    assert.equal(
      readFileSync(ffprobe, "utf8"),
      "#!/usr/bin/env bash\necho ffprobe-fixture\n",
    );

    writeFileSync(ffmpeg, "preserved ffmpeg\n");
    chmodSync(ffmpeg, 0o755);
    const second = spawnSync(
      "/bin/bash",
      ["./scripts/bootstrap.sh", "--skip-ui", "--skip-rust"],
      {
        cwd: item.repository,
        env: item.env,
        encoding: "utf8",
      },
    );
    assert.equal(second.status, 0, `${second.stdout}\n${second.stderr}`);
    assert.equal(readFileSync(ffmpeg, "utf8"), "preserved ffmpeg\n");
  } finally {
    item.cleanup();
  }
});

test("Windows run script launches dim.exe from the release runtime directory", () => {
  const item = fixture();
  try {
    const release = resolve(item.repository, "target/release");
    mkdirSync(release, { recursive: true });
    executable(
      resolve(release, "dim.exe"),
      `#!/usr/bin/env bash
pwd > "$WINDOWS_SCRIPT_FIXTURE/runtime.cwd"
printf '%s\\n' "$*" > "$WINDOWS_SCRIPT_FIXTURE/runtime.args"
`,
    );

    const result = spawnSync(
      "/bin/bash",
      ["./scripts/run.sh", "--release", "--bind-address", "127.0.0.1"],
      {
        cwd: item.repository,
        env: item.env,
        encoding: "utf8",
      },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.equal(
      readFileSync(resolve(item.directory, "runtime.cwd"), "utf8").trim(),
      realpathSync(release),
    );
    assert.equal(
      readFileSync(resolve(item.directory, "runtime.args"), "utf8"),
      "--bind-address 127.0.0.1\n",
    );
  } finally {
    item.cleanup();
  }
});
