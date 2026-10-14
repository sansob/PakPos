"""Tiny httpbin-compatible server for offline demos, docs screenshots and tests.

Usage: python3 scripts/mock_httpbin.py [port]   (default 8787)
"""
import base64
import hashlib
import json
import struct
import threading
import time
import sys
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qsl, urlsplit


USERS = [
    {"id": "1", "name": "Budi Santoso", "city": "Jakarta"},
    {"id": "2", "name": "Siti Rahma", "city": "Bandung"},
    {"id": "3", "name": "Andi Wijaya", "city": "Surabaya"},
]


def _t(kind, name=None, of=None):
    return {"kind": kind, "name": name, "ofType": of}


def _field(name, ty, args=(), desc=None):
    return {"name": name, "description": desc, "type": ty,
            "args": [{"name": a, "type": t, "defaultValue": None} for a, t in args]}


GQL_SCHEMA = {"data": {"__schema": {
    "queryType": {"name": "Query"}, "mutationType": {"name": "Mutation"}, "subscriptionType": None,
    "types": [
        {"kind": "OBJECT", "name": "Query", "description": None, "fields": [
            _field("users", _t("NON_NULL", of=_t("LIST", of=_t("OBJECT", "User"))), [("city", _t("SCALAR", "String"))], "All users, optionally by city"),
            _field("user", _t("OBJECT", "User"), [("id", _t("NON_NULL", of=_t("SCALAR", "ID")))], "One user by id"),
        ]},
        {"kind": "OBJECT", "name": "Mutation", "description": None, "fields": [
            _field("createUser", _t("OBJECT", "User"), [("name", _t("NON_NULL", of=_t("SCALAR", "String")))]),
        ]},
        {"kind": "OBJECT", "name": "User", "description": "A demo user", "fields": [
            _field("id", _t("NON_NULL", of=_t("SCALAR", "ID"))),
            _field("name", _t("SCALAR", "String")),
            _field("city", _t("SCALAR", "String")),
        ]},
        {"kind": "SCALAR", "name": "String", "description": None, "fields": None},
        {"kind": "SCALAR", "name": "ID", "description": None, "fields": None},
    ]}}}


def graphql(payload):
    query = payload.get("query") or ""
    variables = payload.get("variables") or {}
    if "__schema" in query:
        return GQL_SCHEMA
    if "createUser" in query:
        return {"data": {"createUser": {"id": "4", "name": variables.get("name", "New"), "city": None}}}
    if "user(" in query:
        uid = str(variables.get("id", "1"))
        return {"data": {"user": next((u for u in USERS if u["id"] == uid), None)}}
    if "users" in query:
        city = variables.get("city")
        return {"data": {"users": [u for u in USERS if not city or u["city"] == city]}}
    return {"errors": [{"message": "Cannot query that field on the demo schema"}]}


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

    # --- WebSocket echo on /ws (RFC 6455, text frames only) -------------
    def _ws_send(self, text):
        payload = text.encode()
        header = bytes([0x81])
        n = len(payload)
        if n < 126:
            header += bytes([n])
        elif n < 65536:
            header += bytes([126]) + struct.pack(">H", n)
        else:
            header += bytes([127]) + struct.pack(">Q", n)
        with self._ws_lock:
            self.wfile.write(header + payload)
            self.wfile.flush()

    def _ws_recv(self):
        head = self.rfile.read(2)
        if len(head) < 2:
            return None
        opcode, n = head[0] & 0x0F, head[1] & 0x7F
        if n == 126:
            n = struct.unpack(">H", self.rfile.read(2))[0]
        elif n == 127:
            n = struct.unpack(">Q", self.rfile.read(8))[0]
        mask = self.rfile.read(4) if head[1] & 0x80 else b"\0\0\0\0"
        data = bytes(b ^ mask[i % 4] for i, b in enumerate(self.rfile.read(n)))
        return opcode, data

    def websocket(self):
        key = self.headers.get("Sec-WebSocket-Key", "")
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.protocol_version = "HTTP/1.1"  # 101 Switching Protocols requires HTTP/1.1
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        self.wfile.flush()
        self._ws_lock = threading.Lock()
        alive = {"on": True}
        self._ws_send(json.dumps({"type": "welcome", "server": "PakPos mock", "auth": self.headers.get("Authorization")}))

        def ticker():
            i = 0
            while alive["on"]:
                time.sleep(2)
                i += 1
                try:
                    self._ws_send(json.dumps({"type": "tick", "n": i, "price": round(15000 + i * 12.5, 2)}))
                except OSError:
                    break

        threading.Thread(target=ticker, daemon=True).start()
        while True:
            frame = self._ws_recv()
            if frame is None or frame[0] == 0x8:
                break
            if frame[0] == 0x9:  # ping -> pong
                continue
            text = frame[1].decode("utf-8", "replace")
            try:
                reply = {"type": "echo", "received": json.loads(text)}
            except ValueError:
                reply = {"type": "echo", "received": text}
            self._ws_send(json.dumps(reply))
        alive["on"] = False
        self.close_connection = True

    def handle_any(self):
        if urlsplit(self.path).path == "/ws" and self.headers.get("Upgrade", "").lower() == "websocket":
            return self.websocket()
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
        # --- OAuth 2.0 demo (auto-approves everything) -------------------
        if path == "/oauth/authorize":
            q = dict(parse_qsl(urlsplit(self.path).query))
            sep = "&" if "?" in q.get("redirect_uri", "") else "?"
            location = f"{q.get('redirect_uri', '')}{sep}code=demo-code&state={q.get('state', '')}"
            self.send_response(302)
            self.send_header("Location", location)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        if path == "/oauth/token" and self.command == "POST":
            form = data.get("form") or dict(parse_qsl(data.get("data") or ""))
            grant = form.get("grant_type", "")
            if grant not in ("client_credentials", "password", "authorization_code", "refresh_token"):
                return self._send(400, {"error": "unsupported_grant_type"})
            return self._send(200, {
                "access_token": f"demo-{grant}-{uuid.uuid4().hex[:12]}",
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": "demo-refresh",
                "scope": form.get("scope", ""),
            })
        if path == "/oauth/me":
            auth = self.headers.get("Authorization", "")
            if auth.startswith("Bearer demo-"):
                return self._send(200, {"user": "budi", "token": auth[7:], "scopes": ["read"]})
            return self._send(401, {"error": "invalid_token"})
        if path == "/graphql" and self.command == "POST":
            return self._send(200, graphql(data.get("json") or {}))
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
