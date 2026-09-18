import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { fileURLToPath } from "node:url";

export async function serve(port = 7421) {
  const root = fileURLToPath(new URL("../web/", import.meta.url));
  const types = {
    ".html": "text/html",
    ".mjs": "text/javascript",
    ".js": "text/javascript",
    ".wasm": "application/wasm",
    ".json": "application/json",
    ".css": "text/css",
    ".woff2": "font/woff2",
  };
  const server = createServer(async (req, res) => {
    try {
      const path = decodeURIComponent(
        new URL(req.url, "http://localhost").pathname,
      );
      const file = resolve(root, "." + (path === "/" ? "/index.html" : path));
      if (!file.startsWith(root.endsWith(sep) ? root : root + sep)) {
        res.writeHead(403).end();
        return;
      }
      const data = await readFile(file);
      res.writeHead(200, {
        "Content-Type": types[extname(file)] ?? "application/octet-stream",
        "Cache-Control": "no-store",
      });
      res.end(data);
    } catch {
      res
        .writeHead(404)
        .end(
          "Not found. Stage the WASM bundle before opening the browser harness.",
        );
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const { url } = await serve(
    Number(process.env.COLONIES_BROWSER_PORT ?? 7421),
  );
  console.log(`Strata Colonies: ${url}`);
}
