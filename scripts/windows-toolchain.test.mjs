import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const detector = resolve(import.meta.dirname, "windows-toolchain.ps1");

function fixture({ compiler = true, sdk = true, installation = true } = {}) {
  const directory = mkdtempSync(resolve(tmpdir(), "eclipse-toolchain-test-"));
  const visualStudio = resolve(directory, "nonstandard", "Visual Studio");
  const sdkRoot = resolve(directory, "Windows SDK");
  if (installation) mkdirSync(visualStudio, { recursive: true });
  if (compiler) {
    const compilerDirectory = resolve(
      visualStudio,
      "VC/Tools/MSVC/14.0/bin/Hostx64/x64",
    );
    mkdirSync(compilerDirectory, { recursive: true });
    writeFileSync(resolve(compilerDirectory, "cl.exe"), "fixture\n");
  }
  if (sdk) {
    const include = resolve(sdkRoot, "Include/10.0/um");
    const library = resolve(sdkRoot, "Lib/10.0/um/x64");
    mkdirSync(include, { recursive: true });
    mkdirSync(library, { recursive: true });
    writeFileSync(resolve(include, "Windows.h"), "fixture\n");
    writeFileSync(resolve(library, "kernel32.lib"), "fixture\n");
  }
  const vswhere = resolve(directory, "vswhere.ps1");
  const json = installation
    ? JSON.stringify([{ installationPath: visualStudio }])
    : "[]";
  writeFileSync(
    vswhere,
    `if ($args -contains '-format') { '${json.replaceAll("'", "''")}' } elseif (${compiler ? "$true" : "$false"}) { '${visualStudio.replaceAll("'", "''")}' }\nexit 0\n`,
  );
  return {
    directory,
    sdkRoot,
    vswhere,
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function detect(item) {
  return spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      detector,
      "-VsWherePath",
      item.vswhere,
      "-IgnoreSystemDiscovery",
    ],
    {
      env: { ...process.env, WindowsSdkDir: item.sdkRoot },
      encoding: "utf8",
    },
  );
}

for (const [name, options, expected] of [
  ["nonstandard complete installation", {}, /^ready\|/],
  ["installation without compiler", { compiler: false }, /^missing-vctools\|/],
  ["compiler without SDK", { sdk: false }, /^missing-sdk\|/],
  [
    "missing Build Tools",
    { compiler: false, sdk: false, installation: false },
    /^missing-build-tools\|/,
  ],
]) {
  test(
    `Windows toolchain detector identifies ${name}`,
    { skip: process.platform !== "win32" },
    () => {
      const item = fixture(options);
      try {
        const result = detect(item);
        assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
        assert.match(result.stdout.trim(), expected);
      } finally {
        item.cleanup();
      }
    },
  );
}

test("toolchain detector checks compiler files, SDK headers, libraries, registry, and vswhere", () => {
  const source = readFileSync(detector, "utf8");
  assert.match(source, /Get-Command cl\.exe/);
  assert.match(source, /VC\\Tools\\MSVC/);
  assert.match(source, /Windows Kits\\Installed Roots/);
  assert.match(source, /Windows\.h/);
  assert.match(source, /kernel32\.lib/);
  assert.match(
    source,
    /Microsoft\.VisualStudio\.Component\.VC\.Tools\.x86\.x64/,
  );
});
