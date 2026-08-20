import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  cleanupOwnedProcesses,
  spawnManaged,
} from "./process-management.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [action, ...input] = process.argv.slice(2);

function option(name, required = false) {
  const index = input.indexOf(`--${name}`);
  const value = index >= 0 ? input[index + 1] : undefined;
  if (required && !value) throw new Error(`--${name} is required.`);
  return value;
}

if (action === "cleanup") {
  const owner = option("owner", true);
  const results = cleanupOwnedProcesses({ root, owner });
  for (const process of results) {
    console.log(
      `${process.stopped ? "Stopped" : "Removed stale record for"} ${process.name} PID ${process.pid}.`,
    );
  }
  if (results.length === 0) {
    console.log(`No managed processes owned by ${owner}.`);
  }
} else if (action === "run") {
  const separator = input.indexOf("--");
  if (separator < 0 || !input[separator + 1]) {
    throw new Error("A command is required after --.");
  }
  const name = option("name", true);
  const owner = option("owner", true);
  const logPath = option("log");
  const managed = await spawnManaged({
    root,
    name,
    owner,
    command: input[separator + 1],
    args: input.slice(separator + 2),
    logPath,
  });
  console.log(
    `${name} started as PID ${managed.child.pid}${managed.lease.logPath ? `; output: ${managed.lease.logPath}` : ""}`,
  );
  let signalExitCode = 0;
  const stop = (code) => {
    signalExitCode = code;
    managed.stop();
  };
  process.once("SIGINT", () => stop(130));
  process.once("SIGTERM", () => stop(143));
  const result = await managed.closed;
  process.exitCode = signalExitCode || result.code || (result.signal ? 1 : 0);
} else {
  throw new Error(
    "Usage: managed-process.mjs run --name NAME --owner OWNER [--log PATH] -- COMMAND [ARGS...]\n" +
      "       managed-process.mjs cleanup --owner OWNER",
  );
}
