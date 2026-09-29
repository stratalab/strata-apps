#!/usr/bin/env python3
"""Serve pkg/ for local viewing.

Python's http.server has no mapping for .wasm, so `WebAssembly.instantiateStreaming`
rejects the response and wasm-bindgen falls back to buffering the whole module.
That works, but it warns on every load and hides real failures in the noise.
"""
import functools, http.server, socketserver, sys, pathlib

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 4400
ROOT = pathlib.Path(__file__).parent / "pkg"

class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      ".wasm": "application/wasm", ".js": "text/javascript"}
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()
    def log_message(self, *args):
        pass

socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("127.0.0.1", PORT), functools.partial(Handler, directory=str(ROOT))) as httpd:
    print(f"paint: http://localhost:{PORT}/", flush=True)
    httpd.serve_forever()
