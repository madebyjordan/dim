import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const extraArgs = process.argv.slice(2);
const release = extraArgs[0] === "--release";
const backendArgs = release ? extraArgs : [];
const children = new Map();
let stopping = false;
let exitCode = 0;

function start(name, command, args, options = {}) {
  const child = spawn(command, args, {
    cwd: root,
    stdio: "inherit",
    ...options,
  });
  children.set(name, child);
  child.once("error", (error) => {
    console.error(`${name} failed to start: ${error.message}`);
    exitCode = 1;
    stop();
  });
  return child;
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
  start("Dim", resolve(root, "scripts/run.sh"), backendArgs, {
    env: { ...process.env, DIM_BIND_ADDRESS: "0.0.0.0" },
  });
} else {
  start("Dim backend", resolve(root, "scripts/run.sh"), [], {
    env: { ...process.env, DIM_BIND_ADDRESS: "127.0.0.1" },
  });
  start("Eclipse dev server", "corepack", ["pnpm", "--dir", "eclipse", "dev"]);
}

await new Promise((resolveDone) => {
  let remaining = children.size;
  for (const [name, child] of children) {
    child.once("exit", (code, signal) => {
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
