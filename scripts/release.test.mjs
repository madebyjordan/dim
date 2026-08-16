import assert from "node:assert/strict";
import {
  chmodSync,
  copyFileSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import {
  INITIAL_VERSION,
  latestStableVersion,
  nextVersion,
  parseCanonicalVersion,
  parseGitHubRepository,
  updateCanonicalVersion,
} from "./release.mjs";

const root = resolve(import.meta.dirname, "..");
const realGit = spawnSync("which", ["git"], { encoding: "utf8" }).stdout.trim();

function command(cwd, executable, args, env = {}) {
  return spawnSync(executable, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
}

function fixture() {
  const directory = mkdtempSync(resolve(tmpdir(), "dim-release-test-"));
  const repository = resolve(directory, "repo");
  const remote = resolve(directory, "origin.git");
  const bin = resolve(directory, "bin");
  mkdirSync(resolve(repository, "scripts"), { recursive: true });
  mkdirSync(bin);
  copyFileSync(
    resolve(root, "scripts/release.mjs"),
    resolve(repository, "scripts/release.mjs")
  );
  writeFileSync(
    resolve(repository, "Cargo.toml"),
    '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.4.0-dev"\n'
  );
  writeFileSync(resolve(repository, "Cargo.lock"), "# fixture\n");
  writeFileSync(
    resolve(bin, "gh"),
    `#!/usr/bin/env bash
if [[ "\${1:-}" == "--version" ]]; then echo "gh fixture"; exit 0; fi
if [[ "\${1:-}" == "auth" ]]; then exit 0; fi
if [[ "\${1:-}" == "release" && "\${2:-}" == "view" ]]; then
  if [[ "\${FAKE_RELEASE_EXISTS:-}" == "\${3:-}" ]]; then echo '{"tagName":"'"\${3}"'"}'; exit 0; fi
  echo "release not found" >&2; exit 1
fi
if [[ "\${1:-}" == "release" && "\${2:-}" == "list" ]]; then
  echo "\${FAKE_RELEASE_LIST:-[]}"; exit 0
fi
exit 1
`
  );
  writeFileSync(
    resolve(bin, "pnpm"),
    '#!/usr/bin/env bash\nif [[ "${1:-}" == "--version" ]]; then echo "fixture"; exit 0; fi\nif [[ "${1:-}" == "release:validate" ]]; then if [[ "${FAKE_VALIDATION_FAIL:-}" == "1" ]]; then echo "validation failed" >&2; exit 1; fi; echo "validation passed"; exit 0; fi\nexit 1\n'
  );
  writeFileSync(
    resolve(bin, "cargo"),
    '#!/usr/bin/env bash\nif [[ "${1:-}" == "--version" ]]; then echo "cargo fixture"; exit 0; fi\nif [[ "${1:-}" == "check" ]]; then version=$(awk -F\'"\' \'/^version = / { print $2; exit }\' Cargo.toml); printf \'# fixture\\nversion = "%s"\\n\' "$version" > Cargo.lock; exit 0; fi\nif [[ "${1:-}" == "metadata" ]]; then echo \'{"packages":[],"workspace_members":[]}\'; exit 0; fi\nexit 1\n'
  );
  writeFileSync(
    resolve(bin, "git"),
    `#!/usr/bin/env bash
if [[ "\${FAKE_COMMIT_PUSH_FAIL:-}" == "1" && "\${1:-}" == "push" && "$*" == *"HEAD:refs/heads/master"* ]]; then
  echo "simulated commit push failure" >&2; exit 1
fi
exec "${realGit}" "$@"
`
  );
  chmodSync(resolve(bin, "gh"), 0o755);
  chmodSync(resolve(bin, "pnpm"), 0o755);
  chmodSync(resolve(bin, "cargo"), 0o755);
  chmodSync(resolve(bin, "git"), 0o755);

  command(directory, "git", ["init", "--bare", remote]);
  command(repository, "git", ["init", "-b", "master"]);
  command(repository, "git", ["config", "user.name", "Release Test"]);
  command(repository, "git", ["config", "user.email", "release@example.com"]);
  command(repository, "git", ["add", "."]);
  command(repository, "git", ["commit", "-m", "fixture"]);
  command(repository, "git", ["remote", "add", "origin", remote]);
  command(repository, "git", ["push", "-u", "origin", "master"]);
  command(repository, "git", ["remote", "set-url", "--push", "origin", remote]);
  // The fetch URL is GitHub-shaped for repository derivation; insteadOf keeps all network access local.
  command(repository, "git", [
    "remote",
    "set-url",
    "origin",
    "https://github.com/example/dim.git",
  ]);
  command(repository, "git", [
    "config",
    "url.file://" + remote + ".insteadOf",
    "https://github.com/example/dim.git",
  ]);

  return {
    directory,
    repository,
    remote,
    env: { PATH: `${bin}:${process.env.PATH}` },
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function runRelease(
  testFixture,
  { mode = "initial", dryRun = true, env = {} } = {}
) {
  return command(
    testFixture.repository,
    process.execPath,
    ["scripts/release.mjs", mode, ...(dryRun ? ["--dry-run"] : [])],
    { DIM_RELEASE_REPOSITORY: "example/dim", ...testFixture.env, ...env }
  );
}

function commitFile(repository, name, contents = name) {
  writeFileSync(resolve(repository, name), contents);
  assert.equal(command(repository, realGit, ["add", name]).status, 0);
  const committed = command(repository, realGit, [
    "commit",
    "-m",
    `add ${name}`,
  ]);
  assert.equal(committed.status, 0, committed.stderr);
}

function advanceRemote(item) {
  const peer = resolve(item.directory, `peer-${Date.now()}`);
  const cloned = command(item.directory, realGit, [
    "clone",
    "--branch",
    "master",
    item.remote,
    peer,
  ]);
  assert.equal(cloned.status, 0, cloned.stderr);
  assert.equal(
    command(peer, realGit, ["config", "user.name", "Remote Test"]).status,
    0
  );
  assert.equal(
    command(peer, realGit, ["config", "user.email", "remote@example.com"])
      .status,
    0
  );
  commitFile(peer, "remote.txt", `${Date.now()}\n`);
  const pushed = command(peer, realGit, ["push", "origin", "master"]);
  assert.equal(pushed.status, 0, pushed.stderr);
}

function snapshot(item) {
  return {
    head: command(item.repository, realGit, [
      "rev-parse",
      "HEAD",
    ]).stdout.trim(),
    status: command(item.repository, realGit, [
      "status",
      "--porcelain",
      "--untracked-files=all",
    ]).stdout,
    tags: command(item.repository, realGit, ["tag", "--list"]).stdout,
    remote: command(item.repository, realGit, [
      "ls-remote",
      "--heads",
      item.remote,
      "refs/heads/master",
    ]).stdout,
    manifest: readFileSync(resolve(item.repository, "Cargo.toml"), "utf8"),
  };
}

test("initial release is deliberate and normal bumps are stable semver", () => {
  assert.equal(INITIAL_VERSION, "0.3.0");
  assert.equal(nextVersion("0.3.0", "patch"), "0.3.1");
  assert.equal(nextVersion("0.3.0", "minor"), "0.4.0");
  assert.equal(nextVersion("0.3.0", "major"), "1.0.0");
  assert.throws(
    () => nextVersion("0.4.0-dev", "patch"),
    /initial release first/
  );
  assert.throws(
    () => nextVersion("0.3.0", "preview"),
    /Unsupported release type/
  );
  assert.equal(
    latestStableVersion([
      { tagName: "v0.3.9", isDraft: false, isPrerelease: false },
      { tagName: "v0.10.0", isDraft: false, isPrerelease: false },
      { tagName: "v1.0.0-rc.1", isDraft: false, isPrerelease: true },
      { tagName: "v2.0.0", isDraft: true, isPrerelease: false },
    ]),
    "0.10.0"
  );
});

test("only workspace.package.version is mutated", () => {
  const manifest = `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.4.0-dev"\n\n[dependencies]\nexample = "9.9.9"\n`;
  const updated = updateCanonicalVersion(manifest, "0.3.0");
  assert.equal(parseCanonicalVersion(updated), "0.3.0");
  assert.match(updated, /example = "9\.9\.9"/);
});

test("GitHub release repository is derived from the configured remote", () => {
  assert.equal(
    parseGitHubRepository("https://github.com/madebyjordan/dim.git"),
    "madebyjordan/dim"
  );
  assert.equal(
    parseGitHubRepository("git@github.com:madebyjordan/dim.git"),
    "madebyjordan/dim"
  );
  assert.throws(
    () => parseGitHubRepository("https://example.com/dim.git"),
    /not a GitHub/
  );
});

test("a version tag triggers exactly one publishing workflow", () => {
  const workflowDir = resolve(root, ".github/workflows");
  const tagTriggered = readdirSync(workflowDir)
    .filter((name) => /\.ya?ml$/.test(name))
    .filter((name) => {
      const source = readFileSync(resolve(workflowDir, name), "utf8");
      const trigger = source.split(/^jobs:/m)[0];
      return /^\s+tags:/m.test(trigger);
    });
  assert.deepEqual(tagTriggered, ["release.yml"]);

  const release = readFileSync(resolve(workflowDir, "release.yml"), "utf8");
  assert.equal((release.match(/gh release create/g) || []).length, 1);
  assert.match(release, /concurrency:/);
  assert.match(release, /cancel-in-progress: false/);
});

test("every workspace application crate inherits the canonical version", () => {
  for (const manifest of [
    "dim/Cargo.toml",
    "dim-auth/Cargo.toml",
    "dim-core/Cargo.toml",
    "dim-database/Cargo.toml",
    "dim-events/Cargo.toml",
    "dim-extern-api/Cargo.toml",
    "dim-utils/Cargo.toml",
    "dim-web/Cargo.toml",
  ]) {
    assert.match(
      readFileSync(resolve(root, manifest), "utf8"),
      /^version\.workspace = true$/m,
      manifest
    );
  }
});

test("the pre-commit hook uses the repository-pinned Yarn sequentially", () => {
  const hook = readFileSync(resolve(root, "ui/.husky/pre-commit"), "utf8");
  assert.match(hook, /corepack yarn run lint-staged/);
  assert.match(hook, /corepack yarn run fmt/);
  assert.doesNotMatch(hook, /^\s*yarn run/m);
  assert.doesNotMatch(hook, /&/);
});

test("clean synchronized checkout is a valid release input", () => {
  const item = fixture();
  try {
    const before = snapshot(item);
    const result = runRelease(item);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Remote ancestry: 0 local commit/);
    assert.match(result.stdout, /Cargo\.lock/);
    assert.match(result.stdout, /Cargo\.toml/);
    assert.match(result.stdout, /Dry run complete/);
    assert.deepEqual(snapshot(item), before);
  } finally {
    item.cleanup();
  }
});

test("dirty synchronized checkout reports pending tracked and untracked files", () => {
  const item = fixture();
  try {
    writeFileSync(resolve(item.repository, "Cargo.lock"), "dirty lock\n");
    writeFileSync(resolve(item.repository, "new-project-file.txt"), "new\n");
    const result = runRelease(item);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /new-project-file\.txt/);
    assert.match(result.stdout, /Running release validation/);
  } finally {
    item.cleanup();
  }
});

test("local commits ahead of the remote are allowed", () => {
  const item = fixture();
  try {
    commitFile(item.repository, "ahead.txt");
    const result = runRelease(item);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Remote ancestry: 1 local commit/);
  } finally {
    item.cleanup();
  }
});

test("normal bump dry-run derives its target from the latest stable fork release", () => {
  const item = fixture();
  try {
    const manifestPath = resolve(item.repository, "Cargo.toml");
    writeFileSync(
      manifestPath,
      updateCanonicalVersion(readFileSync(manifestPath, "utf8"), "0.3.0")
    );
    writeFileSync(
      resolve(item.repository, "Cargo.lock"),
      '# fixture\nversion = "0.3.0"\n'
    );
    command(item.repository, realGit, ["add", "Cargo.toml", "Cargo.lock"]);
    command(item.repository, realGit, ["commit", "-m", "release base"]);
    command(item.repository, realGit, ["tag", "v0.3.0"]);
    command(item.repository, realGit, ["push", "origin", "master", "v0.3.0"]);
    writeFileSync(resolve(item.repository, "next.txt"), "next\n");

    const result = runRelease(item, {
      mode: "patch",
      env: {
        FAKE_RELEASE_LIST: JSON.stringify([
          { tagName: "v0.3.0", isDraft: false, isPrerelease: false },
        ]),
      },
    });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Latest stable fork release: v0\.3\.0/);
    assert.match(result.stdout, /Dry run: patch -> v0\.3\.1/);
  } finally {
    item.cleanup();
  }
});

test("remote-ahead and genuinely diverged branches abort before validation", () => {
  for (const state of ["behind", "diverged"]) {
    const item = fixture();
    try {
      if (state === "diverged") commitFile(item.repository, "local.txt");
      advanceRemote(item);
      const result = runRelease(item);
      assert.notEqual(result.status, 0, state);
      assert.match(
        result.stderr,
        state === "behind" ? /is behind origin\/master/ : /has diverged/
      );
      assert.doesNotMatch(result.stdout, /Running release validation/);
    } finally {
      item.cleanup();
    }
  }
});

test("validation failure leaves files, index, commits, tags, and remote unchanged", () => {
  const item = fixture();
  try {
    writeFileSync(resolve(item.repository, "pending.txt"), "pending\n");
    const before = snapshot(item);
    const result = runRelease(item, {
      dryRun: false,
      env: { FAKE_VALIDATION_FAIL: "1" },
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /release:validate failed/);
    assert.deepEqual(snapshot(item), before);
  } finally {
    item.cleanup();
  }
});

test("commit push failure prevents local and remote tag creation", () => {
  const item = fixture();
  try {
    const remoteBefore = snapshot(item).remote;
    const result = runRelease(item, {
      dryRun: false,
      env: { FAKE_COMMIT_PUSH_FAIL: "1" },
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /simulated commit push failure/);
    assert.equal(
      command(item.repository, realGit, ["tag", "--list"]).stdout,
      ""
    );
    assert.equal(snapshot(item).remote, remoteBefore);
  } finally {
    item.cleanup();
  }
});

test("release tag points exactly at the successfully pushed release commit", () => {
  const item = fixture();
  try {
    writeFileSync(resolve(item.repository, "pending.txt"), "pending\n");
    const result = runRelease(item, { dryRun: false });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const releaseCommit = command(item.repository, realGit, [
      "rev-parse",
      "HEAD",
    ]).stdout.trim();
    const tagCommit = command(item.repository, realGit, [
      "rev-list",
      "-n",
      "1",
      "v0.3.0",
    ]).stdout.trim();
    const pushedCommit = snapshot(item).remote.split(/\s+/)[0];
    assert.equal(tagCommit, releaseCommit);
    assert.equal(pushedCommit, releaseCommit);
    assert.match(
      command(item.repository, realGit, [
        "ls-remote",
        "--tags",
        item.remote,
        "refs/tags/v0.3.0",
      ]).stdout,
      /refs\/tags\/v0\.3\.0/
    );
    assert.equal(
      command(item.repository, realGit, [
        "log",
        "-1",
        "--format=%s",
      ]).stdout.trim(),
      "chore: release v0.3.0"
    );
  } finally {
    item.cleanup();
  }
});

test("dirty and local-ahead dry-run causes zero mutation", () => {
  const item = fixture();
  try {
    commitFile(item.repository, "ahead.txt");
    writeFileSync(resolve(item.repository, "dirty.txt"), "dirty\n");
    const before = snapshot(item);
    const result = runRelease(item);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /dirty\.txt/);
    assert.match(result.stdout, /1 local commit/);
    assert.deepEqual(snapshot(item), before);
  } finally {
    item.cleanup();
  }
});

test("branch, duplicate tag, and duplicate release guards remain enforced", () => {
  for (const guard of ["branch", "tag", "release"]) {
    const item = fixture();
    try {
      if (guard === "branch")
        command(item.repository, realGit, ["switch", "-c", "feature"]);
      if (guard === "tag") command(item.repository, realGit, ["tag", "v0.3.0"]);
      const result = runRelease(item, {
        env: guard === "release" ? { FAKE_RELEASE_EXISTS: "v0.3.0" } : {},
      });
      assert.notEqual(result.status, 0, guard);
      assert.match(
        result.stderr,
        guard === "branch"
          ? /branch must be master/
          : guard === "tag"
          ? /tag v0\.3\.0 already exists/i
          : /GitHub Release v0\.3\.0 already exists/
      );
    } finally {
      item.cleanup();
    }
  }
});
