import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  backendProcess,
  cargoCommand,
  frontendProcess,
} from "./dev-processes.mjs";
import {
  assertManagedProcessAvailable,
  cleanupOwnedProcesses,
  spawnManaged,
} from "./process-management.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const extraArgs = process.argv.slice(2);
const release = extraArgs[0] === "--release";
const backendArgs = release ? extraArgs.slice(1) : [];
const owner = process.env.ECLIPSE_PROCESS_OWNER ?? "interactive";
const managed = new Map();
let stopping = false;
let exitCode = 0;

function processLogPath(name) {
  const directory = process.env.ECLIPSE_DEV_LOG_DIR;
  return directory ? resolve(directory, `${name}.log`) : undefined;
}

async function start(name, command, args, options = {}) {
  const process = await spawnManaged({
    root,
    name,
    owner,
    command,
    args,
    options,
    logPath: processLogPath(name),
  });
  managed.set(name, process);
  console.log(
    `${name} started as PID ${process.child.pid}${process.lease.logPath ? `; output: ${process.lease.logPath}` : ""}`,
  );
  return process;
}

async function runBeforeStart(name, command, args) {
  const process = await start(name, command, args);
  const result = await process.closed;
  managed.delete(name);
  if (result.code !== 0) {
    throw new Error(
      `${name} ${result.signal ? `stopped after ${result.signal}` : `exited with code ${result.code}`}`,
    );
  }
}

function stop() {
  if (stopping) return;
  stopping = true;
  for (const process of managed.values()) process.stop();
}

process.once("SIGINT", () => {
  exitCode = 130;
  stop();
});
process.once("SIGTERM", () => {
  exitCode = 143;
  stop();
});

try {
  // Only clean leases bearing this explicit owner. Codex sets owner=codex; an
  // interactive developer's processes are never inferred from names or ports.
  if (owner === "codex") {
    const stale = cleanupOwnedProcesses({ root, owner });
    for (const process of stale) {
      console.log(
        `${process.stopped ? "Stopped" : "Removed stale record for"} ${process.name} PID ${process.pid}.`,
      );
    }
  }

  // Detect an interactive or differently owned instance before rebuilding or
  // launching any part of a second development stack.
  assertManagedProcessAvailable({ root, name: "eclipse-backend" });
  if (!release) assertManagedProcessAvailable({ root, name: "eclipse-vite" });

  if (release) {
    const backend = backendProcess({ root, release, args: backendArgs });
    await start("eclipse-backend", backend.command, backend.args, backend.options);
  } else {
    // Rebuild first so a restart cannot serve current UI code with stale Rust.
    await runBeforeStart("eclipse-backend-build", cargoCommand(), [
      "build",
      "--locked",
      "-p",
      "dim",
    ]);
    const backend = backendProcess({ root });
    const frontend = frontendProcess(root);
    await start(
      "eclipse-backend",
      backend.command,
      backend.args,
      backend.options,
    );
    await start(
      "eclipse-vite",
      frontend.command,
      frontend.args,
      frontend.options,
    );
  }
} catch (error) {
  console.error(`Development services failed to start: ${error.message}`);
  exitCode = 1;
  stop();
}

await Promise.all(
  [...managed.entries()].map(async ([name, process]) => {
    const { code, signal } = await process.closed;
    managed.delete(name);
    if (!stopping) {
      exitCode = code ?? 1;
      console.error(
        `${name} exited ${signal ? `after ${signal}` : `with code ${code}`}; stopping development services.`,
      );
      stop();
    }
  }),
);

process.exitCode = exitCode;
