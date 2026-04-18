#!/usr/bin/env python3
"""Mock both the postagent registry API and an OAuth provider on one port.

Usage:
  python3 scripts/mock-oauth.py           # listens on :9999
  POSTAGENT_API_URL=http://localhost:9999 postagent auth mocksite

Endpoints:
  GET  /api/manual?site=mocksite  -> descriptor with one OAuth method
  GET  /authorize                 -> 302 back to CLI loopback with fake code
  POST /token                     -> fake access_token + refresh_token JSON
"""
from http.server import BaseHTTPRequestHandler, HTTPServer
import json, urllib.parse

DESCRIPTOR = {
    "kind": "oauth2",
    "id": "oauth",
    "label": "Mock OAuth",
    "setup_instructions": (
        "### Mock provider\n\n"
        "1. 随便填 Client ID (e.g. `mockcid`)\n"
        "2. 随便填 Client Secret (e.g. `mocksecret`)\n"
        "3. Redirect URI 已注册: `{{redirect_uri}}`\n"
    ),
    "grants": ["authorization_code"],
    "client": {"type": "confidential"},
    "authorize": {"url": "http://localhost:9999/authorize"},
    "token": {
        "url": "http://localhost:9999/token",
        "body_encoding": "form",
        "client_auth": "body",
        "response_map": {
            "access_token": "/access_token",
            "refresh_token": "/refresh_token",
            "expires_in": "/expires_in",
            "token_type": "/token_type",
            "extras": {"bot_id": "/bot_id"},
        },
    },
    "scopes": {"default": ["read"], "separator": " "},
    "refresh": {"behavior": "reusable"},
    "inject": {"in": "header", "name": "Authorization",
               "value_template": "Bearer {access_token}"},
}

MANUAL_RESPONSE = {
    "success": True,
    "data": {
        "name": "mocksite",
        "description": "Local mock for OAuth testing",
        "authentication": None,
        "auth_methods": [DESCRIPTOR],
        "groups": [],
    },
}

TOKEN_RESPONSE = {
    "access_token": "mock_at_" + "A" * 24,
    "refresh_token": "mock_rt_" + "B" * 24,
    "expires_in": 3600,
    "token_type": "Bearer",
    "bot_id": "mock_bot_123",
}


class H(BaseHTTPRequestHandler):
    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(u.query)
        if u.path == "/api/manual":
            self._json(200, MANUAL_RESPONSE)
        elif u.path == "/authorize":
            cb = q["redirect_uri"][0]
            state = q["state"][0]
            loc = f"{cb}?code=MOCK_CODE&state={state}"
            print(f"  authorize -> redirect to {loc}")
            self.send_response(302)
            self.send_header("Location", loc)
            self.end_headers()
        else:
            self.send_response(404); self.end_headers()

    def do_POST(self):
        if self.path == "/token":
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length).decode()
            print(f"  token  <- {body[:120]}")
            self._json(200, TOKEN_RESPONSE)
        else:
            self.send_response(404); self.end_headers()

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    print("mock OAuth provider listening on http://127.0.0.1:9999")
    print("  POSTAGENT_API_URL=http://localhost:9999 postagent auth mocksite")
    HTTPServer(("127.0.0.1", 9999), H).serve_forever()
