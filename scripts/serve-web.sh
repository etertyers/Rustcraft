#!/usr/bin/env bash
# Serves target/web/ (M71) and prints the URLs. Any static server works (no
# special headers are needed); this one is Python's.
#   scripts/serve-web.sh [--https] [port]       (default port 8080)
# Browsers only give a page the storage (OPFS) and the GPU (WebGPU) the game
# needs on https:// or http://localhost. Another computer on the Wi-Fi can't use
# http://<this Mac's address>:8080, so use --https: it makes a self-signed
# certificate (target/web-cert/) and serves https://<address>:<port>/; the
# browser warns once about the certificate; accept it ("Advanced", "Proceed").
set -euo pipefail
cd "$(dirname "$0")/.."
HTTPS=0; PORT=8080
for a in "$@"; do case $a in --https) HTTPS=1;; *) PORT=$a;; esac; done
[ -d target/web ] || { echo "run scripts/build-web.sh first" >&2; exit 1; }
IP=$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || hostname -I 2>/dev/null | cut -d' ' -f1 || true)
if [ "$HTTPS" = 1 ]; then
  mkdir -p target/web-cert
  [ -f target/web-cert/cert.pem ] || openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
    -keyout target/web-cert/key.pem -out target/web-cert/cert.pem -subj "/CN=rustcraft.local" \
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1${IP:+,IP:$IP}" 2>/dev/null
  SCHEME=https
else
  SCHEME=http
fi
echo "Rustcraft (web):"
echo "  this Mac:        $SCHEME://localhost:$PORT/"
if [ -n "${IP:-}" ]; then
  if [ "$HTTPS" = 1 ]; then echo "  other computers: https://$IP:$PORT/   (same Wi-Fi; accept the certificate warning once)"
  else echo "  other computers: needs --https (plain http://$IP:$PORT/ has no storage or GPU access)"; fi
fi
exec python3 -I - "$PORT" "$HTTPS" <<'PY'
import http.server, os, ssl, sys
os.chdir("target/web")
class H(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      ".wasm": "application/wasm", ".js": "text/javascript", ".mjs": "text/javascript"}
    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()
server = http.server.ThreadingHTTPServer(("0.0.0.0", int(sys.argv[1])), H)
if sys.argv[2] == "1":
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain("../web-cert/cert.pem", "../web-cert/key.pem")
    server.socket = ctx.wrap_socket(server.socket, server_side=True)
server.serve_forever()
PY
