import { spawn } from "node:child_process";
import {
  accessSync,
  chmodSync,
  constants,
  copyFileSync,
  lstatSync,
  mkdirSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, extname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function parseBuildArgs(args) {
  const options = {
    release: false,
    skipUi: false,
    skipRust: false,
    stageFile: "",
  };

  for (let index = 0; index < args.length; index += 1) {
    switch (args[index]) {
      case "--release":
        options.release = true;
        break;
      case "--skip-ui":
        options.skipUi = true;
        break;
      case "--skip-rust":
        options.skipRust = true;
        break;
      case "--stage-file":
        if (!args[index + 1])
          throw new BuildUsageError("--stage-file requires a path.");
        options.stageFile = args[index + 1];
        index += 1;
        break;
      case "-h":
      case "--help":
        options.help = true;
        break;
      default:
        throw new BuildUsageError(`Unknown option: ${args[index]}`);
    }
  }
  return options;
}

export class BuildUsageError extends Error {
  exitCode = 2;
}

class BuildCommandError extends Error {
  constructor(command, code, signal) {
    super(
      `${command} ${signal ? `stopped after ${signal}` : `exited with code ${code}`}`,
    );
    this.exitCode = code || 1;
  }
}

export function findExecutable(
  command,
  { env = process.env, platform = process.platform } = {},
) {
  const path = env.PATH ?? env.Path ?? "";
  const extensions =
    platform === "win32"
      ? extname(command)
        ? [""]
        : (env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD").split(";")
      : [""];

  for (const directory of path.split(platform === "win32" ? ";" : ":")) {
    if (!directory) continue;
    for (const extension of extensions) {
      const candidate = resolve(
        directory,
        `${command}${extension.toLowerCase()}`,
      );
      try {
        accessSync(
          candidate,
          platform === "win32" ? constants.F_OK : constants.X_OK,
        );
        return candidate;
      } catch {}
    }
  }
  throw new Error(`Missing required command: ${command}`);
}

function windowsCommandLine(command, args) {
  const quote = (value) => {
    if (/["\r\n]/.test(value)) {
      throw new Error(
        `Unsupported character in Windows command argument: ${value}`,
      );
    }
    return `"${value}"`;
  };
  return `"${quote(command)} ${args.map(quote).join(" ")}"`;
}

export async function runCommand(command, args, { cwd, env, platform }) {
  await new Promise((resolveDone, reject) => {
    let child;
    try {
      const windowsShim =
        platform === "win32" && /\.(?:cmd|bat)$/i.test(command);
      child = spawn(
        windowsShim
          ? (env.ComSpec ?? process.env.ComSpec ?? "cmd.exe")
          : command,
        windowsShim
          ? ["/d", "/s", "/c", windowsCommandLine(command, args)]
          : args,
        {
          cwd,
          env,
          stdio: "inherit",
          windowsVerbatimArguments: windowsShim,
        },
      );
    } catch (error) {
      reject(error);
      return;
    }
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolveDone();
      else reject(new BuildCommandError(command, code, signal));
    });
  });
}

function writeStage(stageFile, stage) {
  if (stageFile) writeFileSync(stageFile, `${stage}\n`);
}

function ensureMediaTool(source, destination, platform) {
  let destinationExists = false;
  try {
    lstatSync(destination);
    destinationExists = true;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }

  if (destinationExists) {
    try {
      accessSync(
        destination,
        platform === "win32" ? constants.F_OK : constants.X_OK,
      );
    } catch {
      throw new Error(
        `${destination} exists but is not executable; leaving it unchanged.`,
      );
    }
    console.log(`Using existing ${destination}`);
    return;
  }

  mkdirSync(dirname(destination), { recursive: true });
  if (platform === "win32") {
    copyFileSync(source, destination);
    chmodSync(destination, 0o755);
  } else {
    symlinkSync(source, destination);
  }
}

export async function build({
  root = defaultRoot,
  args = process.argv.slice(2),
  env = process.env,
  platform = process.platform,
} = {}) {
  const options = parseBuildArgs(args);
  if (options.help) {
    console.log(
      "Usage: node ./scripts/build.mjs [--release] [--skip-ui] [--skip-rust] [--stage-file PATH]",
    );
    return;
  }

  findExecutable("git", { env, platform });
  const ffmpeg = findExecutable("ffmpeg", { env, platform });
  const ffprobe = findExecutable("ffprobe", { env, platform });
  const mediaSuffix = platform === "win32" ? ".exe" : "";
  const binarySuffix = platform === "win32" ? ".exe" : "";

  let corepack;
  if (!options.skipUi) {
    const [major, minor] = process.versions.node.split(".").map(Number);
    if (major !== 24 || minor < 19) {
      throw new Error(
        `Eclipse requires Node.js 24.19.0 or newer in the 24.x line; found ${process.version}.`,
      );
    }
    corepack = findExecutable("corepack", { env, platform });
  }

  let cargo;
  if (!options.skipRust) {
    cargo = findExecutable(platform === "win32" ? "cargo.exe" : "cargo", {
      env,
      platform,
    });
    findExecutable(platform === "win32" ? "rustc.exe" : "rustc", {
      env,
      platform,
    });
  }

  mkdirSync(resolve(root, "utils"), { recursive: true });
  ensureMediaTool(
    ffmpeg,
    resolve(root, `utils/ffmpeg${mediaSuffix}`),
    platform,
  );
  ensureMediaTool(
    ffprobe,
    resolve(root, `utils/ffprobe${mediaSuffix}`),
    platform,
  );

  if (!options.skipUi) {
    writeStage(options.stageFile, "Installing frontend dependencies");
    console.log("Installing locked Eclipse dependencies...");
    await runCommand(
      corepack,
      ["pnpm", "--dir", "eclipse", "install", "--frozen-lockfile"],
      { cwd: root, env, platform },
    );
    writeStage(options.stageFile, "Building frontend");
    await runCommand(corepack, ["pnpm", "--dir", "eclipse", "build"], {
      cwd: root,
      env,
      platform,
    });
  }

  if (!options.skipRust) {
    const targetDir = isAbsolute(env.CARGO_TARGET_DIR ?? "")
      ? env.CARGO_TARGET_DIR
      : resolve(root, env.CARGO_TARGET_DIR ?? "target");
    const cargoEnv = { ...env, CARGO_TARGET_DIR: targetDir };
    const cargoArgs = ["build", "--locked"];
    if (options.release) cargoArgs.push("--release");

    writeStage(options.stageFile, "Building Eclipse backend");
    console.log("Building Eclipse...");
    await runCommand(cargo, cargoArgs, {
      cwd: root,
      env: cargoEnv,
      platform,
    });

    writeStage(options.stageFile, "Preparing runtime");
    if (options.release) {
      ensureMediaTool(
        ffmpeg,
        resolve(targetDir, `release/utils/ffmpeg${mediaSuffix}`),
        platform,
      );
      ensureMediaTool(
        ffprobe,
        resolve(targetDir, `release/utils/ffprobe${mediaSuffix}`),
        platform,
      );
    }

    const profile = options.release ? "release" : "debug";
    const binary = resolve(targetDir, profile, `eclipse${binarySuffix}`);
    console.log(`Eclipse is ready at ${binary}`);
    console.log(
      `Run it with corepack pnpm dev${options.release ? " --release" : ""}`,
    );
  }

  writeStage(options.stageFile, "Complete");
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  build().catch((error) => {
    console.error(error.message);
    if (error instanceof BuildUsageError) {
      console.error(
        "Usage: node ./scripts/build.mjs [--release] [--skip-ui] [--skip-rust] [--stage-file PATH]",
      );
    }
    process.exitCode = error.exitCode ?? 1;
  });
}
