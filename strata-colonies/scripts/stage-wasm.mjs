import { mkdir, readFile, copyFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";

const source = process.argv[2];
if (!source)
  throw new Error(
    "Usage: npm run wasm:stage -- /path/to/released/playground/pkg",
  );
const target = new URL("../web/pkg/", import.meta.url);
await mkdir(target, { recursive: true });
// The floor the worker enforces, not an exact pin: a newer engine is fine.
const manifest = { minimumVersion: "1.2.2", files: {} };
for (const name of ["strata_wasm.js", "strata_wasm_bg.wasm"]) {
  const data = await readFile(join(resolve(source), name));
  await copyFile(join(resolve(source), name), new URL(name, target));
  manifest.files[name] = {
    bytes: data.length,
    sha256: createHash("sha256").update(data).digest("hex"),
  };
}
await writeFile(
  new URL("manifest.json", target),
  JSON.stringify(manifest, null, 2) + "\n",
);
console.log(
  `Staged WASM; runtime must report Strata ${manifest.minimumVersion} or newer.`,
  manifest.files,
);
