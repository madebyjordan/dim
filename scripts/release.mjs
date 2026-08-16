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

export function latestStableVersion(releases) {
  const stable = releases
    .filter((release) => !release.isDraft && !release.isPrerelease)
    .map((release) => release.tagName?.match(/^v(.+)$/)?.[1])
    .filter((version) => version && STABLE_SEMVER.test(version))
    .sort((left, right) => {
      const a = left.split(".").map(Number);
      const b = right.split(".").map(Number);
      return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
    });
  return stable.at(-1);
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

function findLatestStableRelease(repository) {
  const releases = JSON.parse(
    output("gh", [
      "release",
      "list",
      "--repo",
      repository,
      "--limit",
      "100",
      "--json",
      "tagName,isDraft,isPrerelease",
    ])
  );
  const version = latestStableVersion(releases);
  if (!version) {
    throw new Error(
      "No stable fork release exists; run pnpm release:initial first"
    );
  }
  return version;
}

function assertTagMissing(remote, tag) {
  if (output("git", ["tag", "--list", tag]))
    throw new Error(`Local tag ${tag} already exists`);
  if (output("git", ["ls-remote", "--tags", remote, `refs/tags/${tag}`])) {
    throw new Error(`Remote tag ${tag} already exists`);
  }
}

function assertReleaseTagExists(remote, version) {
  const tag = `v${version}`;
  if (!output("git", ["ls-remote", "--tags", remote, `refs/tags/${tag}`])) {
    throw new Error(
      `Base release tag ${tag} does not exist; run pnpm release:initial first`
    );
  }
}

function parseArguments(argv) {
  const [mode, ...rawFlags] = argv;
  const flags = rawFlags.filter((flag) => flag !== "--");
  if (!["initial", "patch", "minor", "major"].includes(mode)) {
    throw new Error(
      "Usage: pnpm release:<initial|patch|minor|major> [-- --dry-run]"
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

function pendingFiles() {
  const files = [
    ...output("git", ["diff", "--name-only", "--relative"]).split("\n"),
    ...output("git", ["diff", "--cached", "--name-only", "--relative"]).split(
      "\n"
    ),
    ...output("git", ["ls-files", "--others", "--exclude-standard"]).split(
      "\n"
    ),
  ].filter(Boolean);
  return [...new Set(files)].sort();
}

function sameFiles(left, right) {
  return (
    left.length === right.length &&
    left.every((file, index) => file === right[index])
  );
}

function remoteBranchHead(remote, branch) {
  const line = output("git", [
    "ls-remote",
    "--heads",
    remote,
    `refs/heads/${branch}`,
  ]);
  return line.split(/\s+/)[0];
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

    assertCommand("git");
    assertCommand("gh");
    assertCommand("pnpm");
    if (output("git", ["rev-parse", "--show-toplevel"]) !== root) {
      throw new Error(`Run the release from the repository root: ${root}`);
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
    const divergence = output("git", [
      "rev-list",
      "--left-right",
      "--count",
      `HEAD...${expectedUpstream}`,
    ]);
    const [localAhead, remoteAhead] = divergence.split(/\s+/).map(Number);
    if (remoteAhead > 0) {
      throw new Error(
        localAhead > 0
          ? `${branch} has diverged from ${expectedUpstream} (${localAhead} local-only, ${remoteAhead} remote-only commits); reconcile manually before releasing`
          : `${branch} is behind ${expectedUpstream} by ${remoteAhead} commit(s); update it manually before releasing`
      );
    }

    const remoteUrl = output("git", ["remote", "get-url", remote]);
    const repository =
      process.env.DIM_RELEASE_REPOSITORY || parseGitHubRepository(remoteUrl);
    if (!/^[^/\s]+\/[^/\s]+$/.test(repository)) {
      throw new Error(`Invalid GitHub repository identifier: ${repository}`);
    }
    const baseVersion =
      mode === "initial" ? undefined : findLatestStableRelease(repository);
    if (baseVersion) assertReleaseTagExists(remote, baseVersion);
    const targetVersion =
      mode === "initial" ? INITIAL_VERSION : nextVersion(baseVersion, mode);
    const tag = `v${targetVersion}`;

    console.log(`${dryRun ? "Dry run" : "Release"}: ${mode} -> ${tag}`);
    console.log(
      `Canonical version: Cargo.toml workspace.package.version (${currentVersion})`
    );
    if (baseVersion) console.log(`Latest stable fork release: v${baseVersion}`);
    console.log(
      `Remote ancestry: ${localAhead} local commit(s) ahead, 0 remote-only commits`
    );
    assertTagMissing(remote, tag);
    assertReleaseMissing(repository, tag);
    if (mode === "initial" && currentVersion !== INITIAL_VERSION) {
      console.log(`Version change: ${currentVersion} -> ${INITIAL_VERSION}`);
    }

    const changedVersion = currentVersion !== targetVersion;
    const versionFiles = changedVersion ? ["Cargo.lock", "Cargo.toml"] : [];
    const filesToCommit = [
      ...new Set([...pendingFiles(), ...versionFiles]),
    ].sort();
    if (!filesToCommit.length) {
      throw new Error(
        `Nothing to release: no pending project changes and canonical version is already ${targetVersion}`
      );
    }
    console.log("Files to include in the release commit:");
    for (const file of filesToCommit) console.log(`  ${file}`);

    console.log("Running release validation before any repository mutation...");
    validateRelease();

    console.log(`Release commit: chore: release ${tag}`);
    console.log(`Push: HEAD -> ${expectedUpstream} (no force)`);
    console.log(
      `Tag after successful commit push: annotated ${tag} at the pushed release commit`
    );
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
      const manifestBeforeMutation = readFileSync(manifestPath, "utf8");
      if (manifestBeforeMutation !== currentManifest) {
        throw new Error(
          "Cargo.toml changed during validation; inspect it and retry"
        );
      }
      writeFileSync(
        manifestPath,
        updateCanonicalVersion(currentManifest, targetVersion)
      );
      execute("cargo", ["check", "--workspace", "--offline"]);
      verifyWorkspaceVersions(targetVersion);
    } else {
      verifyWorkspaceVersions(targetVersion);
    }

    const actualFiles = pendingFiles();
    if (!sameFiles(actualFiles, filesToCommit)) {
      throw new Error(
        `Release file set changed during validation/version synchronization; expected [${filesToCommit.join(
          ", "
        )}], found [${actualFiles.join(", ")}]`
      );
    }
    execute("git", ["add", "--all", "--", "."]);
    execute("git", ["commit", "-m", `chore: release ${tag}`]);
    const releaseCommit = output("git", ["rev-parse", "HEAD"]);
    execute("git", ["push", remote, `HEAD:refs/heads/${branch}`]);
    const pushedCommit = remoteBranchHead(remote, branch);
    if (pushedCommit !== releaseCommit) {
      throw new Error(
        `${expectedUpstream} is ${
          pushedCommit || "unreadable"
        }, not release commit ${releaseCommit}; tag was not created`
      );
    }
    execute("git", ["tag", "-a", tag, releaseCommit, "-m", `Dim ${tag}`]);
    const taggedCommit = output("git", ["rev-list", "-n", "1", tag]);
    if (taggedCommit !== releaseCommit) {
      throw new Error(
        `Tag ${tag} does not point at release commit ${releaseCommit}`
      );
    }
    execute("git", ["push", remote, `refs/tags/${tag}`]);
    console.log(
      `Pushed release commit ${releaseCommit} and ${tag}; GitHub Actions will publish https://github.com/${repository}/releases/tag/${tag}`
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
