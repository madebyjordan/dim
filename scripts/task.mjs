import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { build, findExecutable, runCommand } from "./build.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const platform = process.platform;
const env = process.env;

function executable(name) {
  return findExecutable(platform === "win32" ? `${name}.exe` : name, {
    env,
    platform,
  });
}

function corepack() {
  return findExecutable("corepack", { env, platform });
}

async function run(command, args) {
  await runCommand(command, args, { cwd: root, env, platform });
}

async function testTask() {
  console.log("Running installer and release command tests...");
  await run(process.execPath, [
    "--test",
    "./scripts/build.test.mjs",
    "./scripts/generate-api-contract.test.mjs",
    "./scripts/install.test.mjs",
    "./scripts/package-manager.test.mjs",
    "./scripts/prepare-pnpm.test.mjs",
    "./scripts/rust-dependencies.test.mjs",
    "./scripts/windows-launcher.test.mjs",
    "./scripts/windows-scripts.test.mjs",
    "./scripts/windows-toolchain.test.mjs",
    "./scripts/release.test.mjs",
  ]);

  console.log("Running Eclipse frontend tests...");
  await run(corepack(), ["pnpm", "--dir", "eclipse", "test"]);
  await run(corepack(), ["pnpm", "--dir", "eclipse", "check"]);

  console.log("Running locked Rust workspace tests...");
  // These legacy scanner cases can wait indefinitely on external metadata/probe work. Normal
  // branch CI runs them separately; keep the public task deterministic as before.
  await run(executable("cargo"), [
    "test",
    "--workspace",
    "--tests",
    "--locked",
    "--",
    "--skip",
    "scanner::tests::mediafile::test_construct_mediafile",
    "--skip",
    "scanner::tests::mediafile::rescan_keeps_metadata_aligned_after_existing_files_are_filtered",
  ]);
}

async function validateReleaseTask() {
  console.log(
    "Validating frontend lockfile, formatting, contract, types, lint, tests, and build...",
  );
  const pinnedPnpm = corepack();
  await run(pinnedPnpm, ["pnpm", "install", "--frozen-lockfile"]);
  await run(pinnedPnpm, [
    "pnpm",
    "--dir",
    "eclipse",
    "exec",
    "prettier",
    "--check",
    "src",
  ]);
  await run(pinnedPnpm, ["pnpm", "--dir", "eclipse", "contract:check"]);
  await run(pinnedPnpm, ["pnpm", "--dir", "eclipse", "check"]);
  await run(pinnedPnpm, ["pnpm", "--dir", "eclipse", "test"]);
  await run(pinnedPnpm, ["pnpm", "--dir", "eclipse", "build"]);

  console.log(
    "Validating Rust formatting, locked tests, and optimized source build...",
  );
  await run(executable("cargo"), ["fmt", "--all", "--", "--check"]);
  // The release gate intentionally keeps the former shell task's bounded scanner exclusions;
  // normal branch CI retains coverage for both cases.
  await run(executable("cargo"), [
    "test",
    "--workspace",
    "--tests",
    "--locked",
    "--",
    "--skip",
    "scanner::tests::mediafile::test_construct_mediafile",
    "--skip",
    "scanner::tests::mediafile::rescan_keeps_metadata_aligned_after_existing_files_are_filtered",
  ]);
  await build({ root, args: ["--release"], env, platform });
  console.log("Release validation passed.");
}

async function main() {
  const [task, ...args] = process.argv.slice(2);
  switch (task) {
    case "setup": {
      const installer = resolve(
        root,
        platform === "win32" ? "install.cmd" : "install.sh",
      );
      await run(installer, args);
      break;
    }
    case "test":
      if (args.length)
        throw new Error(`Unknown test arguments: ${args.join(" ")}`);
      await testTask();
      break;
    case "release-validate":
      if (args.length)
        throw new Error(
          `Unknown release validation arguments: ${args.join(" ")}`,
        );
      await validateReleaseTask();
      break;
    default:
      throw new Error(`Unknown task: ${task ?? "(missing)"}`);
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = error.exitCode ?? 1;
});
