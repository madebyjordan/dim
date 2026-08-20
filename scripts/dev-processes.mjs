import { accessSync, constants, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";

export function cargoCommand(platform = process.platform) {
  return platform === "win32" ? "cargo.exe" : "cargo";
}

export function backendProcess({
  root,
  release = false,
  args = [],
  env = process.env,
  platform = process.platform,
}) {
  const targetDir = resolve(root, env.CARGO_TARGET_DIR ?? "target");
  const profile = release ? "release" : "debug";
  const executable = `eclipse${platform === "win32" ? ".exe" : ""}`;
  const command = resolve(targetDir, profile, executable);

  accessSync(command, platform === "win32" ? constants.F_OK : constants.X_OK);

  return {
    command,
    args,
    options: {
      cwd: release ? resolve(targetDir, "release") : root,
      env: {
        ...env,
        ECLIPSE_BIND_ADDRESS: release ? "0.0.0.0" : "127.0.0.1",
      },
    },
  };
}

export function frontendProcess(root) {
  const frontendRoot = resolve(root, "eclipse");
  const require = createRequire(resolve(frontendRoot, "package.json"));
  const packageJsonPath = require.resolve("vite/package.json");
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));
  const bin =
    typeof packageJson.bin === "string" ? packageJson.bin : packageJson.bin?.vite;

  if (!bin) throw new Error(`Vite does not declare a CLI in ${packageJsonPath}`);

  return {
    // The package-manager and node_modules/.bin launchers are .cmd shims on Windows. Launching
    // Vite's JavaScript entry point with Node works identically on every supported platform and
    // keeps Vite as a direct child so shutdown does not leave a shell-owned process behind.
    command: process.execPath,
    args: [resolve(dirname(packageJsonPath), bin), "dev"],
    options: { cwd: frontendRoot },
  };
}
