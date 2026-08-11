import { readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const source = resolve(root, "api-contract/openapi.json");
const target = resolve(root, "ui/src/api/generated.ts");
const contract = JSON.parse(await readFile(source, "utf8"));

function type(schema) {
  if (schema.$ref) return schema.$ref.split("/").at(-1);
  if (schema.oneOf) return schema.oneOf.map(type).join(" | ");
  if (schema.const) return JSON.stringify(schema.const);
  if (schema.enum) return schema.enum.map(JSON.stringify).join(" | ");
  if (Array.isArray(schema.type)) return schema.type.map((value) => type({ ...schema, type: value })).join(" | ");
  if (schema.type === "array") return `Array<${type(schema.items)}> ` .trim();
  if (schema.type === "integer" || schema.type === "number") return "number";
  if (schema.type === "boolean") return "boolean";
  if (schema.type === "null") return "null";
  if (schema.type === "object") {
    if (!schema.properties) return "Record<string, unknown>";
    const required = new Set(schema.required ?? []);
    return `{ ${Object.entries(schema.properties).map(([key, value]) => `${JSON.stringify(key)}${required.has(key) ? "" : "?"}: ${type(value)};`).join(" ")} }`;
  }
  return "string";
}

const schemas = Object.entries(contract.components.schemas)
  .map(([name, schema]) => `export type ${name} = ${type(schema)};`)
  .join("\n\n");
const operations = Object.entries(contract.paths).flatMap(([path, item]) =>
  Object.entries(item).filter(([method]) => ["get", "post", "put", "patch", "delete"].includes(method)).map(([, operation]) => `  ${operation.operationId}: ${JSON.stringify(path)};`)
).join("\n");
const unformatted = `// Generated from api-contract/openapi.json. Do not edit.\n\n${schemas}\n\nexport interface ApiOperations {\n${operations}\n}\n`;
const output = execFileSync(resolve(root, "ui/node_modules/.bin/prettier"), ["--stdin-filepath", target], {
  input: unformatted,
  encoding: "utf8",
});

if (process.argv.includes("--check")) {
  const current = await readFile(target, "utf8").catch(() => "");
  if (current !== output) {
    console.error("Generated API types are stale. Run yarn contract:generate.");
    process.exit(1);
  }
} else {
  await writeFile(target, output);
}
