import { spawn, spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { randomUUID } from "node:crypto";

export const PROCESS_STATE_DIRECTORY = ".eclipse-processes";

function processExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

function windowsProcessInfo(pid) {
  const script = [
    `$item = Get-CimInstance Win32_Process -Filter 'ProcessId=${pid}' -ErrorAction SilentlyContinue`,
    "if ($null -ne $item) {",
    "  [pscustomobject]@{ executable = $item.ExecutablePath; started = $item.CreationDate.ToUniversalTime().ToString('o') } | ConvertTo-Json -Compress",
    "}",
  ].join("; ");
  const result = spawnSync("powershell.exe", [
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    script,
  ], { encoding: "utf8", windowsHide: true });
  if (result.status !== 0 || !result.stdout.trim()) return null;
  return JSON.parse(result.stdout);
}

function linuxProcessInfo(pid) {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const close = stat.lastIndexOf(")");
    const fields = stat.slice(close + 2).split(" ");
    return {
      executable: realpathSync(`/proc/${pid}/exe`),
      // Field 22 is process start time; fields here begin at field 3.
      started: fields[19],
    };
  } catch {
    return null;
  }
}

function macProcessInfo(pid) {
  const started = spawnSync("ps", ["-p", String(pid), "-o", "lstart="], {
    encoding: "utf8",
  });
  const executable = spawnSync("ps", ["-p", String(pid), "-o", "comm="], {
    encoding: "utf8",
  });
  if (started.status !== 0 || executable.status !== 0) return null;
  if (!started.stdout.trim() || !executable.stdout.trim()) return null;
  return {
    executable: executable.stdout.trim(),
    started: started.stdout.trim(),
  };
}

export function processInfo(pid, platform = process.platform) {
  if (!Number.isSafeInteger(pid) || pid <= 0 || !processExists(pid)) return null;
  if (platform === "win32") return windowsProcessInfo(pid);
  if (platform === "linux") return linuxProcessInfo(pid);
  return macProcessInfo(pid);
}

function sameExecutable(left, right, platform = process.platform) {
  if (!left || !right) return false;
  return platform === "win32"
    ? left.toLowerCase() === right.toLowerCase()
    : left === right;
}

export function leaseMatchesProcess(lease, info, platform = process.platform) {
  return Boolean(
    info &&
      lease?.identity?.started === info.started &&
      sameExecutable(lease?.identity?.executable, info.executable, platform),
  );
}

function leasePath(root, name) {
  const safeName = name.replace(/[^a-z0-9_.-]+/gi, "-");
  return resolve(root, PROCESS_STATE_DIRECTORY, `${safeName}.json`);
}

export function assertManagedProcessAvailable({
  root,
  name,
  platform = process.platform,
}) {
  const path = leasePath(root, name);
  if (!existsSync(path)) return;
  const existing = JSON.parse(readFileSync(path, "utf8"));
  const info = processInfo(existing.pid, platform);
  if (leaseMatchesProcess(existing, info, platform)) {
    throw new Error(
      `${name} is already running as PID ${existing.pid}${existing.logPath ? `; output: ${existing.logPath}` : ""}`,
    );
  }
  rmSync(path, { force: true });
}

function removeMatchingLease(path, token) {
  try {
    const lease = JSON.parse(readFileSync(path, "utf8"));
    if (lease.token === token) rmSync(path, { force: true });
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

function windowsProcessTree(rootPid) {
  const script = [
    "$all = Get-CimInstance Win32_Process",
    `$pending = @(${rootPid})`,
    "$found = @()",
    "while ($pending.Count -gt 0) {",
    "  $parent = $pending[0]",
    "  if ($pending.Count -eq 1) { $pending = @() } else { $pending = @($pending[1..($pending.Count - 1)]) }",
    "  $children = @($all | Where-Object ParentProcessId -eq $parent | Select-Object -ExpandProperty ProcessId)",
    "  $found += $children",
    "  $pending += $children",
    "}",
    `$found += ${rootPid}`,
    "$found | ConvertTo-Json -Compress",
  ].join("; ");
  const result = spawnSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    { encoding: "utf8", windowsHide: true },
  );
  if (result.status !== 0 || !result.stdout.trim()) return [rootPid];
  const parsed = JSON.parse(result.stdout);
  return (Array.isArray(parsed) ? parsed : [parsed]).map(Number).reverse();
}

export function terminateProcessTree(pid, { platform = process.platform } = {}) {
  if (!processExists(pid)) return;
  if (platform !== "win32") {
    try {
      process.kill(-pid, "SIGTERM");
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
    return;
  }

  // Snapshot only descendants of the verified owned root, then stop leaves
  // before their parent so FFmpeg/Vite helper processes cannot be orphaned.
  const tree = windowsProcessTree(pid);
  const ids = tree.filter((id) => Number.isSafeInteger(id) && id > 0).join(",");
  if (!ids) return;
  spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `$ids = @(${ids}); foreach ($id in $ids) { Stop-Process -Id $id -ErrorAction SilentlyContinue }`,
    ],
    { windowsHide: true },
  );
}

export async function spawnManaged({
  root,
  name,
  owner = "interactive",
  command,
  args = [],
  options = {},
  logPath,
  platform = process.platform,
}) {
  const path = leasePath(root, name);
  mkdirSync(resolve(root, PROCESS_STATE_DIRECTORY), { recursive: true });
  assertManagedProcessAvailable({ root, name, platform });

  let logDescriptor;
  const absoluteLogPath = logPath ? resolve(root, logPath) : undefined;
  if (absoluteLogPath) {
    mkdirSync(dirname(absoluteLogPath), { recursive: true });
    logDescriptor = openSync(absoluteLogPath, "a");
    writeFileSync(
      logDescriptor,
      `\n[${new Date().toISOString()}] ${name}: ${command} ${args.join(" ")}\n`,
    );
  }

  const token = randomUUID();
  const child = spawn(command, args, {
    cwd: root,
    ...options,
    detached: platform !== "win32",
    env: {
      ...process.env,
      ...options.env,
      ECLIPSE_MANAGED_PROCESS_OWNER: owner,
      ECLIPSE_MANAGED_PROCESS_TOKEN: token,
    },
    stdio: absoluteLogPath ? ["ignore", logDescriptor, logDescriptor] : "inherit",
  });
  if (logDescriptor !== undefined) closeSync(logDescriptor);

  await new Promise((resolveStarted, reject) => {
    child.once("spawn", resolveStarted);
    child.once("error", reject);
  });
  const identity = processInfo(child.pid, platform);
  if (!identity) {
    terminateProcessTree(child.pid, { platform });
    throw new Error(
      `${name} started as PID ${child.pid}, but its identity could not be verified.`,
    );
  }
  const lease = {
    version: 1,
    token,
    name,
    owner,
    pid: child.pid,
    identity,
    command,
    args,
    cwd: options.cwd ?? root,
    logPath: absoluteLogPath,
    createdAt: new Date().toISOString(),
  };
  const temporary = `${path}.${process.pid}.${token}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(lease, null, 2)}\n`);
  renameSync(temporary, path);

  let stopped = false;
  const closed = new Promise((resolveClosed) => {
    child.once("close", (code, signal) => {
      removeMatchingLease(path, token);
      resolveClosed({ code, signal });
    });
  });
  const stop = () => {
    if (stopped) return;
    stopped = true;
    terminateProcessTree(child.pid, { platform });
  };
  return { child, lease, leasePath: path, closed, stop };
}

export function cleanupOwnedProcesses({
  root,
  owner,
  platform = process.platform,
}) {
  const directory = resolve(root, PROCESS_STATE_DIRECTORY);
  if (!existsSync(directory)) return [];
  const results = [];
  for (const file of readdirSync(directory).filter((item) =>
    item.endsWith(".json"),
  )) {
    const path = resolve(directory, file);
    const lease = JSON.parse(readFileSync(path, "utf8"));
    if (lease.owner !== owner) continue;
    const info = processInfo(lease.pid, platform);
    if (leaseMatchesProcess(lease, info, platform)) {
      terminateProcessTree(lease.pid, { platform });
      results.push({ ...lease, stopped: true });
    } else {
      results.push({ ...lease, stopped: false });
    }
    rmSync(path, { force: true });
  }
  return results;
}
