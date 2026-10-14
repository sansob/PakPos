"""Tiny httpbin-compatible server for offline demos, docs screenshots and tests.

Usage: python3 scripts/mock_httpbin.py [port]   (default 8787)
"""
import base64
import json
import sys
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qsl, urlsplit


class Handler(BaseHTTPRequestHandler):
    server_version = "mock-httpbin/1.0"

    def log_message(self, *args):  # quiet
        pass

    def _send(self, status, payload, content_type="application/json"):
        body = payload if isinstance(payload, bytes) else json.dumps(payload, indent=2).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _echo(self):
        parts = urlsplit(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length).decode("utf-8", "replace") if length else ""
        ctype = self.headers.get("Content-Type", "")
        data = {
            "args": dict(parse_qsl(parts.query)),
            "headers": {k: v for k, v in self.headers.items()},
            "origin": self.client_address[0],
            "url": f"http://{self.headers.get('Host')}{self.path}",
        }
        if self.command not in ("GET", "HEAD"):
            data["data"] = raw
            data["form"] = dict(parse_qsl(raw)) if "x-www-form-urlencoded" in ctype else {}
            try:
                data["json"] = json.loads(raw) if raw else None
            except ValueError:
                data["json"] = None
        return parts.path, data

    def handle_any(self):
        path, data = self._echo()
        method = self.command.lower()
        if path == f"/{method}" or path == "/anything":
            return self._send(200, data)
        if path.startswith("/status/"):
            code = int(path.rsplit("/", 1)[1])
            return self._send(code, {"status": code})
        if path == "/bearer":
            auth = self.headers.get("Authorization", "")
            if auth.startswith("Bearer "):
                return self._send(200, {"authenticated": True, "token": auth[7:]})
            return self._send(401, {"authenticated": False})
        if path.startswith("/basic-auth/"):
            _, _, user, pw = path.split("/", 3)
            expected = "Basic " + base64.b64encode(f"{user}:{pw}".encode()).decode()
            ok = self.headers.get("Authorization") == expected
            return self._send(200 if ok else 401, {"authenticated": ok, "user": user})
        if path == "/uuid":
            return self._send(200, {"uuid": str(uuid.uuid4())})
        if path == "/json":
            return self._send(200, {"slideshow": {"author": "PakPos", "title": "Demo", "slides": [
                {"title": "Halo", "type": "all"},
                {"title": "Overview", "type": "all", "items": ["Fast", "Native", "Open source"]}]}})
        if path == "/html":
            return self._send(200, b"<html><body><h1>PakPos</h1></body></html>", "text/html")
        return self._send(404, {"error": "not found", "path": path})

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = do_OPTIONS = handle_any


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8787
    print(f"mock httpbin on http://127.0.0.1:{port}")
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
