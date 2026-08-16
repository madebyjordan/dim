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
  nextVersion,
  parseCanonicalVersion,
  parseGitHubRepository,
  updateCanonicalVersion,
} from "./release.mjs";

const root = resolve(import.meta.dirname, "..");

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
    '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.3.0"\n'
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
exit 1
`
  );
  writeFileSync(
    resolve(bin, "pnpm"),
    '#!/usr/bin/env bash\nif [[ "${1:-}" == "--version" ]]; then echo "fixture"; exit 0; fi\nif [[ "${1:-}" == "release:validate" ]]; then echo "validation passed"; exit 0; fi\nexit 1\n'
  );
  chmodSync(resolve(bin, "gh"), 0o755);
  chmodSync(resolve(bin, "pnpm"), 0o755);

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
    repository,
    env: { PATH: `${bin}:${process.env.PATH}` },
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function dryRun(testFixture, extraEnv = {}) {
  return command(
    testFixture.repository,
    process.execPath,
    ["scripts/release.mjs", "initial", "--dry-run"],
    { DIM_RELEASE_REPOSITORY: "example/dim", ...testFixture.env, ...extraEnv }
  );
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

test("the real dry-run path validates remote state without mutation", () => {
  const item = fixture();
  try {
    const before = command(item.repository, "git", [
      "rev-parse",
      "HEAD",
    ]).stdout.trim();
    const result = dryRun(item);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Dry run complete/);
    assert.equal(
      command(item.repository, "git", ["rev-parse", "HEAD"]).stdout.trim(),
      before
    );
    assert.equal(
      command(item.repository, "git", ["tag", "--list"]).stdout.trim(),
      ""
    );
    assert.equal(
      command(item.repository, "git", ["status", "--porcelain"]).stdout.trim(),
      ""
    );
  } finally {
    item.cleanup();
  }
});

test("dry-run guards reject dirty state, wrong branch, divergence, duplicate tags, and releases", () => {
  for (const guard of ["dirty", "branch", "diverge", "tag", "release"]) {
    const item = fixture();
    try {
      if (guard === "dirty")
        writeFileSync(resolve(item.repository, "dirty.txt"), "dirty");
      if (guard === "branch")
        command(item.repository, "git", ["switch", "-c", "feature"]);
      if (guard === "diverge") {
        writeFileSync(resolve(item.repository, "ahead.txt"), "ahead");
        command(item.repository, "git", ["add", "ahead.txt"]);
        command(item.repository, "git", ["commit", "-m", "ahead"]);
      }
      if (guard === "tag") command(item.repository, "git", ["tag", "v0.3.0"]);
      const result = dryRun(
        item,
        guard === "release" ? { FAKE_RELEASE_EXISTS: "v0.3.0" } : {}
      );
      assert.notEqual(result.status, 0, guard);
      assert.match(
        result.stderr,
        guard === "dirty"
          ? /not clean/
          : guard === "branch"
          ? /branch must be master/
          : guard === "diverge"
          ? /HEAD must exactly match origin\/master/
          : guard === "tag"
          ? /tag v0\.3\.0 already exists/i
          : /GitHub Release v0\.3\.0 already exists/
      );
    } finally {
      item.cleanup();
    }
  }
});
