import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";

const target = process.argv[2];
if (!target)
  throw new Error("Usage: npm run website:sync -- /path/to/stratadb.org");
const root = resolve(target);
const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
if (pkg.name !== "stratadb.org")
  throw new Error("Target must be the stratadb.org website repository.");
const source = new URL("../web/", import.meta.url);
const assets = join(root, "public/demos/colonies/assets");
const content = join(root, "src/demos/colonies");
await mkdir(assets, { recursive: true });
await mkdir(content, { recursive: true });
const files = {};
for (const name of (await readdir(source))
  .filter((name) => /\.(mjs|css)$/.test(name))
  .sort()) {
  let data = await readFile(new URL(name, source), "utf8");
  // The host owns the shared fonts. Preserve the game's module-relative worker
  // imports; they also work when the HTML route is requested without a slash.
  if (name.endsWith("css")) data = data.replaceAll("./fonts/", "/fonts/");
  data = data.replaceAll("\u2014", name.endsWith("mjs") ? "\\u2014" : "-");
  await writeFile(join(assets, name), data);
  files[name] = createHash("sha256").update(data).digest("hex");
}
let html = await readFile(new URL("index.html", source), "utf8");
html = html
  .replaceAll("./fonts/", "/fonts/")
  .replaceAll('"./app.css"', '"/demos/colonies/assets/app.css"')
  .replaceAll('"./app.mjs"', '"/demos/colonies/assets/app.mjs"')
  .replaceAll('"./playful.css"', '"/demos/colonies/assets/playful.css"')
  .replaceAll("https://stratadb.org/", "/")
  .replaceAll("\u2014", "&#8212;")
  .replace(
    "</head>",
    '<link rel="canonical" href="https://stratadb.org/demos/colonies/" />\n<link rel="icon" type="image/svg+xml" href="/logo.svg" />\n<meta property="og:type" content="website" />\n<meta property="og:title" content="Strata Colonies: one cell, a different future" />\n<meta property="og:description" content="Change a colony, revisit its past, and fork another future with a real StrataDB database in your browser." />\n<meta property="og:url" content="https://stratadb.org/demos/colonies/" />\n<meta property="og:image" content="https://stratadb.org/og/default.png" />\n<meta name="twitter:card" content="summary_large_image" />\n</head>',
  );
await writeFile(join(content, "index.html"), html);
files["index.html"] = createHash("sha256").update(html).digest("hex");
await writeFile(
  join(content, "source-manifest.json"),
  JSON.stringify(
    { source: "strata-apps/strata-colonies/web", files },
    null,
    2,
  ) + "\n",
);
console.log(
  `Synced Colonies sources into ${root}. Run npm run check in the website to verify the hashes and exercise the demo against the release it stages.`,
);
