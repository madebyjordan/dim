import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  backendProcess,
  cargoCommand,
  frontendProcess,
} from "./dev-processes.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const extraArgs = process.argv.slice(2);
const release = extraArgs[0] === "--release";
const backendArgs = release ? extraArgs.slice(1) : [];
const children = new Map();
let stopping = false;
let exitCode = 0;

function start(name, command, args, options = {}) {
  let child;
  try {
    child = spawn(command, args, {
      cwd: root,
      stdio: "inherit",
      ...options,
    });
  } catch (error) {
    console.error(`${name} failed to start: ${error.message}`);
    exitCode = 1;
    stop();
    return null;
  }
  children.set(name, child);
  child.once("error", (error) => {
    console.error(`${name} failed to start: ${error.message}`);
    exitCode = 1;
    stop();
  });
  return child;
}

async function runBeforeStart(name, command, args) {
  await new Promise((resolveDone, reject) => {
    const child = spawn(command, args, { cwd: root, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolveDone();
      else
        reject(
          new Error(
            `${name} ${signal ? `stopped after ${signal}` : `exited with code ${code}`}`,
          ),
        );
    });
  });
}

function terminate(child, signal = "SIGTERM") {
  if (child.exitCode !== null || child.signalCode !== null) return;
  try {
    child.kill(signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

function stop(signal = "SIGTERM") {
  if (stopping) return;
  stopping = true;
  for (const child of children.values()) terminate(child, signal);
}

process.once("SIGINT", () => {
  exitCode = 130;
  stop();
});
process.once("SIGTERM", () => {
  exitCode = 143;
  stop();
});

if (release) {
  const backend = backendProcess({ root, release, args: backendArgs });
  start("Eclipse", backend.command, backend.args, backend.options);
} else {
  // Development must rebuild first or a restart can silently run stale Rust while Vite serves
  // current UI code. Native Windows cannot spawn run.sh, so launch the resulting binary itself.
  await runBeforeStart("Eclipse backend build", cargoCommand(), [
    "build",
    "--locked",
    "-p",
    "dim",
  ]);
  const backend = backendProcess({ root });
  const frontend = frontendProcess(root);
  const backendChild = start(
    "Eclipse backend",
    backend.command,
    backend.args,
    backend.options,
  );
  if (backendChild) {
    start(
      "Eclipse dev server",
      frontend.command,
      frontend.args,
      frontend.options,
    );
  }
}

await new Promise((resolveDone) => {
  let remaining = children.size;
  if (remaining === 0) {
    resolveDone();
    return;
  }
  for (const [name, child] of children) {
    child.once("close", (code, signal) => {
      if (!stopping) {
        exitCode = code ?? 1;
        console.error(
          `${name} exited ${signal ? `after ${signal}` : `with code ${code}`}; stopping development services.`,
        );
        stop();
      }
      remaining -= 1;
      if (remaining === 0) resolveDone();
    });
  }
});

process.exitCode = exitCode;
