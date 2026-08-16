#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

export const INITIAL_VERSION = "0.3.0";
export const RELEASE_WORKFLOW = ".github/workflows/release.yml";
const STABLE_SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function parseCanonicalVersion(manifest) {
  const workspacePackage = manifest.match(
    /\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/
  )?.[1];
  const version = workspacePackage?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error("Cargo.toml has no workspace.package.version");
  return version;
}

export function nextVersion(current, bump) {
  const match = current.match(STABLE_SEMVER);
  if (!match) {
    throw new Error(
      `Canonical version ${current} is not a stable semantic version; create the initial release first`
    );
  }
  let [, major, minor, patch] = match.map(Number);
  if (bump === "patch") patch += 1;
  else if (bump === "minor") {
    minor += 1;
    patch = 0;
  } else if (bump === "major") {
    major += 1;
    minor = 0;
    patch = 0;
  } else throw new Error(`Unsupported release type: ${bump}`);
  return `${major}.${minor}.${patch}`;
}

export function updateCanonicalVersion(manifest, version) {
  const current = parseCanonicalVersion(manifest);
  return manifest.replace(
    `[workspace.package]\nversion = "${current}"`,
    `[workspace.package]\nversion = "${version}"`
  );
}

export function parseGitHubRepository(remoteUrl) {
  const match = remoteUrl.match(
    /github\.com[/:]([^/\s]+)\/([^/\s]+?)(?:\.git)?$/
  );
  if (!match)
    throw new Error(`Release remote is not a GitHub repository: ${remoteUrl}`);
  return `${match[1]}/${match[2]}`;
}

function execute(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
    env: process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0 && !options.allowFailure) {
    const detail = `${result.stdout || ""}${result.stderr || ""}`.trim();
    throw new Error(
      `${command} ${args.join(" ")} failed${detail ? `: ${detail}` : ""}`
    );
  }
  return result;
}

function output(command, args) {
  return execute(command, args, { capture: true }).stdout.trim();
}

function assertCommand(command) {
  const result = spawnSync(command, ["--version"], { stdio: "ignore" });
  if (result.status !== 0)
    throw new Error(`Required command is unavailable: ${command}`);
}

function assertReleaseMissing(repository, tag) {
  const result = execute(
    "gh",
    ["release", "view", tag, "--repo", repository, "--json", "tagName"],
    { capture: true, allowFailure: true }
  );
  if (result.status === 0)
    throw new Error(`GitHub Release ${tag} already exists`);
  const detail = `${result.stdout}${result.stderr}`;
  if (!/release not found|HTTP 404/i.test(detail)) {
    throw new Error(`Could not verify GitHub Release ${tag}: ${detail.trim()}`);
  }
}

function assertReleaseExists(repository, tag) {
  execute(
    "gh",
    ["release", "view", tag, "--repo", repository, "--json", "tagName"],
    {
      capture: true,
    }
  );
}

function assertTagMissing(remote, tag) {
  if (output("git", ["tag", "--list", tag]))
    throw new Error(`Local tag ${tag} already exists`);
  if (output("git", ["ls-remote", "--tags", remote, `refs/tags/${tag}`])) {
    throw new Error(`Remote tag ${tag} already exists`);
  }
}

function assertCurrentReleaseExists(remote, repository, version) {
  const tag = `v${version}`;
  if (!output("git", ["ls-remote", "--tags", remote, `refs/tags/${tag}`])) {
    throw new Error(
      `Base release tag ${tag} does not exist; run release:initial first`
    );
  }
  assertReleaseExists(repository, tag);
}

function parseArguments(argv) {
  const [mode, ...rawFlags] = argv;
  const flags = rawFlags.filter((flag) => flag !== "--");
  if (!["initial", "patch", "minor", "major"].includes(mode)) {
    throw new Error(
      "Usage: release.mjs <initial|patch|minor|major> [--dry-run]"
    );
  }
  const unknown = flags.filter((flag) => flag !== "--dry-run");
  if (unknown.length)
    throw new Error(`Unknown release option: ${unknown.join(" ")}`);
  return { mode, dryRun: flags.includes("--dry-run") };
}

function verifyWorkspaceVersions(expected) {
  const metadata = JSON.parse(
    output("cargo", ["metadata", "--format-version", "1", "--no-deps"])
  );
  const members = new Set(metadata.workspace_members);
  const mismatches = metadata.packages
    .filter((pkg) => members.has(pkg.id) && pkg.version !== expected)
    .map((pkg) => `${pkg.name}=${pkg.version}`);
  if (mismatches.length) {
    throw new Error(
      `Workspace version drift remains: ${mismatches.join(", ")}`
    );
  }
}

function validateRelease() {
  execute("pnpm", ["release:validate"]);
}

export function main(argv = process.argv.slice(2)) {
  try {
    const { mode, dryRun } = parseArguments(argv);
    const branch = process.env.DIM_RELEASE_BRANCH || "master";
    const remote = process.env.DIM_RELEASE_REMOTE || "origin";
    const expectedUpstream = `${remote}/${branch}`;
    const manifestPath = resolve(root, "Cargo.toml");
    const currentManifest = readFileSync(manifestPath, "utf8");
    const currentVersion = parseCanonicalVersion(currentManifest);
    const targetVersion =
      mode === "initial" ? INITIAL_VERSION : nextVersion(currentVersion, mode);
    const tag = `v${targetVersion}`;

    console.log(`${dryRun ? "Dry run" : "Release"}: ${mode} -> ${tag}`);
    console.log(
      `Canonical version: Cargo.toml workspace.package.version (${currentVersion})`
    );

    assertCommand("git");
    assertCommand("gh");
    assertCommand("pnpm");
    if (output("git", ["rev-parse", "--show-toplevel"]) !== root) {
      throw new Error(`Run the release from the repository root: ${root}`);
    }
    if (output("git", ["status", "--porcelain", "--untracked-files=all"])) {
      throw new Error(
        "Working tree is not clean; commit or stash every change before releasing"
      );
    }
    const currentBranch = output("git", ["branch", "--show-current"]);
    if (currentBranch !== branch) {
      throw new Error(
        `Release branch must be ${branch}; current branch is ${
          currentBranch || "detached"
        }`
      );
    }
    const upstream = output("git", [
      "rev-parse",
      "--abbrev-ref",
      "@{upstream}",
    ]);
    if (upstream !== expectedUpstream) {
      throw new Error(
        `Release branch upstream must be ${expectedUpstream}; found ${upstream}`
      );
    }
    execute("gh", ["auth", "status", "--hostname", "github.com"], {
      capture: true,
    });
    execute("git", ["fetch", "--prune", "--tags", remote]);
    const head = output("git", ["rev-parse", "HEAD"]);
    const remoteHead = output("git", ["rev-parse", expectedUpstream]);
    const divergence = output("git", [
      "rev-list",
      "--left-right",
      "--count",
      `HEAD...${expectedUpstream}`,
    ]);
    if (head !== remoteHead || divergence !== "0\t0") {
      throw new Error(
        `HEAD must exactly match ${expectedUpstream}; divergence is ${divergence}`
      );
    }

    const remoteUrl = output("git", ["remote", "get-url", remote]);
    const repository =
      process.env.DIM_RELEASE_REPOSITORY || parseGitHubRepository(remoteUrl);
    if (!/^[^/\s]+\/[^/\s]+$/.test(repository)) {
      throw new Error(`Invalid GitHub repository identifier: ${repository}`);
    }
    assertTagMissing(remote, tag);
    assertReleaseMissing(repository, tag);
    if (mode !== "initial")
      assertCurrentReleaseExists(remote, repository, currentVersion);
    if (mode === "initial" && currentVersion !== INITIAL_VERSION) {
      console.log(`Version change: ${currentVersion} -> ${INITIAL_VERSION}`);
    }

    console.log("Running release validation before any repository mutation...");
    validateRelease();

    const changedVersion = currentVersion !== targetVersion;
    const versionFiles = changedVersion ? ["Cargo.toml", "Cargo.lock"] : [];
    console.log(
      `Version files: ${
        versionFiles.length
          ? versionFiles.join(", ")
          : "none (already synchronized)"
      }`
    );
    console.log(
      `Release commit: ${changedVersion ? `chore: release ${tag}` : "none"}`
    );
    console.log(`Tag: annotated ${tag} at ${head}`);
    console.log(`Workflow: ${RELEASE_WORKFLOW}`);
    console.log(
      `Artifacts: dim-${tag}-linux-x86_64.tar.gz and dim-${tag}-linux-x86_64.tar.gz.sha256`
    );
    console.log(
      `Container: ghcr.io/${repository.toLowerCase()}:${targetVersion}`
    );
    if (dryRun) {
      console.log(
        "Dry run complete; no files, commits, tags, pushes, or releases were created."
      );
      return;
    }

    if (changedVersion) {
      writeFileSync(
        manifestPath,
        updateCanonicalVersion(currentManifest, targetVersion)
      );
      execute("cargo", ["check", "--workspace", "--offline"]);
      verifyWorkspaceVersions(targetVersion);
      const changed = output("git", ["diff", "--name-only"])
        .split("\n")
        .filter(Boolean);
      const unexpected = changed.filter(
        (file) => !["Cargo.toml", "Cargo.lock"].includes(file)
      );
      if (unexpected.length) {
        throw new Error(
          `Version synchronization changed unexpected files: ${unexpected.join(
            ", "
          )}`
        );
      }
      execute("git", ["add", "Cargo.toml", "Cargo.lock"]);
      execute("git", ["commit", "-m", `chore: release ${tag}`]);
    } else {
      verifyWorkspaceVersions(targetVersion);
    }

    execute("git", ["tag", "-a", tag, "-m", `Dim ${tag}`]);
    execute("git", [
      "push",
      "--atomic",
      remote,
      `HEAD:refs/heads/${branch}`,
      `refs/tags/${tag}`,
    ]);
    console.log(
      `Pushed ${tag}; GitHub Actions will publish https://github.com/${repository}/releases/tag/${tag}`
    );
  } catch (error) {
    console.error(`Release aborted: ${error.message}`);
    process.exitCode = 1;
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
)
  main();
