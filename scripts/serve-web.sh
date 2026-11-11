#!/usr/bin/env bash
# Serves target/web/ on the local network (M71) and prints the URLs. Any
# static server works (no special headers are needed); this one is Python's.
#   scripts/serve-web.sh [port]       (default 8080)
set -euo pipefail
cd "$(dirname "$0")/.."
PORT=${1:-8080}
[ -d target/web ] || { echo "run scripts/build-web.sh first" >&2; exit 1; }
IP=$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || hostname -I 2>/dev/null | cut -d' ' -f1 || true)
echo "Rustcraft (web):"
echo "  this Mac:        http://localhost:$PORT/"
[ -n "${IP:-}" ] && echo "  other computers: http://$IP:$PORT/   (same Wi-Fi; WebGPU needs https or localhost, see docs/web.md)"
exec python3 -I - "$PORT" <<'PY'
import http.server, os, sys, functools
os.chdir("target/web")
class H(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      ".wasm": "application/wasm", ".js": "text/javascript", ".mjs": "text/javascript"}
    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()
http.server.ThreadingHTTPServer(("0.0.0.0", int(sys.argv[1])), H).serve_forever()
PY
