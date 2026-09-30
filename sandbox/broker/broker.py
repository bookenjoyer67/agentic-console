#!/usr/bin/env python3
"""console sandbox broker — holds the provider credentials so the agent container never does.

The agent container runs on an --internal docker network with no route to the
internet. This broker is the only thing it can reach; the broker holds the real
Claude Code credential (OAuth, refreshed in place) and the DeepSeek key, and
injects them upstream. The agent only ever sees a dummy token.

Routes:
    GET  /health                  -> broker status, never any secret material
    */v1/messages*                -> https://api.anthropic.com   (Claude Code)
    */v1/chat/completions, /v1/models -> OPENAI_UPSTREAM          (opencode)

Env:
    ANTHROPIC_MODE       oauth | apikey | off      (default oauth)
    CLAUDE_CRED          path to ~/.claude/.credentials.json (oauth mode)
    ANTHROPIC_API_KEY    used when ANTHROPIC_MODE=apikey
    OPENAI_KEY_FILE      file containing the DeepSeek/OpenAI-compatible key
    OPENAI_UPSTREAM      default https://api.deepseek.com
    PORT                 default 4000

OAuth constants are the ones Claude Code itself uses (read out of the installed
binary): token URL https://platform.claude.com/v1/oauth/token, public client id
9d1c250a-e61b-44d9-88ed-5944d1962f5e, beta header oauth-2025-04-20.
"""

import json
import os
import sys
import time
import http.client
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOKEN_URL = "https://platform.claude.com/v1/oauth/token"
CLIENT_ID = os.environ.get("CLAUDE_OAUTH_CLIENT_ID", "9d1c250a-e61b-44d9-88ed-5944d1962f5e")
OAUTH_BETA = "oauth-2025-04-20"

ANTHROPIC_MODE = os.environ.get("ANTHROPIC_MODE", "oauth").lower()
CLAUDE_CRED = os.environ.get("CLAUDE_CRED", "/secrets/claude/.credentials.json")
ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
OPENAI_KEY_FILE = os.environ.get("OPENAI_KEY_FILE", "/secrets/deepseek.key")
OPENAI_UPSTREAM = os.environ.get("OPENAI_UPSTREAM", "https://api.deepseek.com")
PORT = int(os.environ.get("PORT", "4000"))
BACKUP_DIR = os.environ.get("BACKUP_DIR", "/state/backups")

HOP_BY_HOP = {
    "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
    "te", "trailers", "transfer-encoding", "upgrade", "host", "content-length",
}


def log(*a):
    print(time.strftime("[%H:%M:%S]"), *a, flush=True)


def read_secret(path):
    try:
        with open(path, "r") as fh:
            return fh.read().strip()
    except OSError as exc:
        log("secret read failed:", path, exc.__class__.__name__)
        return ""


class CredentialError(Exception):
    pass


def load_cred():
    try:
        with open(CLAUDE_CRED, "r") as fh:
            return json.load(fh)
    except (OSError, ValueError) as exc:
        raise CredentialError(
            f"cannot read Claude credential at {CLAUDE_CRED} ({exc.__class__.__name__}). "
            "Log in on the host with `claude` first."
        )


def save_cred(cred):
    """Atomic in-place update inside the mounted ~/.claude directory, with a backup."""
    os.makedirs(BACKUP_DIR, exist_ok=True)
    try:
        with open(CLAUDE_CRED, "r") as fh:
            prev = fh.read()
        stamp = time.strftime("%Y%m%d-%H%M%S")
        bfd = os.open(os.path.join(BACKUP_DIR, f"credentials-{stamp}.json"),
                      os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(bfd, "w") as fh:  # 0600: a backup is still a credential
            fh.write(prev)
    except OSError:
        pass
    tmp = CLAUDE_CRED + ".broker.tmp"
    with open(tmp, "w") as fh:
        json.dump(cred, fh)
    os.chmod(tmp, 0o600)
    os.replace(tmp, CLAUDE_CRED)
    log("credential refreshed and written back to the host file")


def refresh(cred):
    oauth = cred.get("claudeAiOauth") or {}
    token = oauth.get("refreshToken")
    if not token:
        raise CredentialError("no refresh token in the credential file")
    body = json.dumps({
        "grant_type": "refresh_token",
        "refresh_token": token,
        "client_id": CLIENT_ID,
        "scope": " ".join(oauth.get("scopes") or []),
    }).encode()
    req = urllib.request.Request(
        TOKEN_URL, data=body, headers={"Content-Type": "application/json",
                                       "User-Agent": "console-sandbox-broker"}
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read())
    access = data.get("access_token")
    if not access:
        raise CredentialError("refresh response had no access_token")
    oauth["accessToken"] = access
    if data.get("refresh_token"):
        oauth["refreshToken"] = data["refresh_token"]
    expires_in = int(data.get("expires_in") or 3600)
    oauth["expiresAt"] = int((time.time() + expires_in) * 1000)
    cred["claudeAiOauth"] = oauth
    save_cred(cred)
    return access


def anthropic_token(force=False):
    """Current OAuth access token, refreshed when it is about to expire."""
    cred = load_cred()
    oauth = cred.get("claudeAiOauth") or {}
    expires_at = int(oauth.get("expiresAt") or 0) / 1000.0
    if force or expires_at - time.time() < 120:
        log("access token expired/near expiry — refreshing")
        return refresh(cred)
    return oauth.get("accessToken") or refresh(cred)


def route_for(path):
    if path.startswith("/v1/messages") or path.startswith("/v1/complete"):
        return "anthropic"
    # Claude Code also asks its own API which models the account is entitled to.
    # Without this the sandbox cannot fetch the model list (no egress), so aliases
    # like `--model opus` resolve to a default the account may not have.
    # Allowlisted narrowly: create_api_key and the other /api/oauth paths stay refused.
    if path.startswith("/api/oauth/claude_cli/roles"):
        return "anthropic"
    if path.startswith("/v1/chat/completions") or path.startswith("/v1/models") \
            or path.startswith("/v1/embeddings"):
        return "openai"
    return None


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "console-sandbox-broker"

    def log_message(self, *a):  # silence the default access log
        pass

    # ---------------------------------------------------------------- plumbing
    def _body(self):
        n = int(self.headers.get("content-length") or 0)
        return self.rfile.read(n) if n else b""

    def _forward(self, target, headers, body):
        if target == "anthropic":
            conn = http.client.HTTPSConnection("api.anthropic.com", timeout=600)
        else:
            host = OPENAI_UPSTREAM.split("://", 1)[-1].rstrip("/")
            conn = http.client.HTTPSConnection(host, timeout=600)
        conn.request(self.command, self.path, body=body, headers=headers)
        return conn, conn.getresponse()

    def _relay(self, resp):
        self.send_response(resp.status)
        length = resp.getheader("content-length")
        for key, value in resp.getheaders():
            if key.lower() in HOP_BY_HOP or key.lower() == "content-length":
                continue
            self.send_header(key, value)
        if length is not None:
            self.send_header("content-length", length)
            self.end_headers()
            remaining = int(length)
            while remaining > 0:
                chunk = resp.read(min(65536, remaining))
                if not chunk:
                    break
                self.wfile.write(chunk)
                remaining -= len(chunk)
        else:
            self.send_header("connection", "close")
            self.end_headers()
            while True:
                chunk = resp.read(65536)
                if not chunk:
                    break
                self.wfile.write(chunk)
        try:
            self.wfile.flush()
        except OSError:
            pass

    def _error(self, status, message):
        payload = json.dumps({"type": "error", "error": {"type": "broker_error", "message": message}}).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    # ------------------------------------------------------------------ routes
    def do_GET(self):
        if self.path == "/health":
            payload = json.dumps({
                "ok": True,
                "anthropic_mode": ANTHROPIC_MODE,
                "openai_upstream": OPENAI_UPSTREAM,
                "openai_key_present": bool(read_secret(OPENAI_KEY_FILE)),
            }).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        self._handle()

    def do_POST(self):
        if self.path == "/refresh":
            # maintenance endpoint: force the OAuth refresh now (used to verify the
            # write-back path works before the token actually expires)
            try:
                anthropic_token(force=True)
            except CredentialError as exc:
                self._error(502, f"refresh failed: {exc}")
                return
            payload = json.dumps({"ok": True, "refreshed": True}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        self._handle()

    def _handle(self):
        route = route_for(self.path)
        if route is None:
            log("REFUSED", self.command, self.path)
            self._error(404, f"broker only forwards provider API paths; {self.path} is not one")
            return

        body = self._body()
        # lowercase every header name: HTTP headers are case-insensitive, and a dict
        # with both "Authorization" and "authorization" sends BOTH — which is how a
        # dummy token ends up at the upstream next to the real one.
        headers = {k.lower(): v for k, v in self.headers.items() if k.lower() not in HOP_BY_HOP}
        headers["host"] = "api.anthropic.com" if route == "anthropic" else OPENAI_UPSTREAM.split("://", 1)[-1].rstrip("/")

        if route == "anthropic":
            if ANTHROPIC_MODE == "off":
                self._error(503, "anthropic is disabled in this broker")
                return
            if ANTHROPIC_MODE == "apikey":
                headers.pop("authorization", None)
                headers["x-api-key"] = ANTHROPIC_API_KEY
            else:
                try:
                    token = anthropic_token()
                except CredentialError as exc:
                    log("CREDENTIAL ERROR:", exc)
                    self._error(502, f"sandbox broker: {exc}")
                    return
                headers.pop("x-api-key", None)
                headers["authorization"] = f"Bearer {token}"
                beta = headers.get("anthropic-beta", "")
                if OAUTH_BETA not in beta:
                    headers["anthropic-beta"] = (beta + "," + OAUTH_BETA).lstrip(",")
            headers.setdefault("anthropic-version", "2023-06-01")
        else:
            key = read_secret(OPENAI_KEY_FILE)
            if not key:
                self._error(503, "no upstream key configured for the openai-compatible route")
                return
            headers.pop("x-api-key", None)
            headers["authorization"] = f"Bearer {key}"

        started = time.time()
        retried = False
        while True:
            try:
                conn, resp = self._forward(route, headers, body)
            except Exception as exc:  # noqa: BLE001 — broker must answer, never crash
                log("UPSTREAM FAIL", route, exc.__class__.__name__, str(exc)[:200])
                self._error(502, f"broker could not reach the upstream: {exc.__class__.__name__}")
                return

            if resp.status == 401 and route == "anthropic" and ANTHROPIC_MODE == "oauth" and not retried:
                log("upstream 401 — forcing a token refresh and retrying once")
                resp.read()
                conn.close()
                try:
                    headers["authorization"] = f"Bearer {anthropic_token(force=True)}"
                except CredentialError as exc:
                    self._error(502, f"sandbox broker: {exc}")
                    return
                retried = True
                continue

            log(f"{resp.status} {self.command} {self.path} ({time.time() - started:.1f}s)")
            if resp.status >= 400:
                # the upstream's own explanation — invaluable when wiring a broker up
                peek = resp.read()
                text = peek.decode("utf-8", "replace")
                if "gzip" in (resp.getheader("content-encoding") or "").lower():
                    try:
                        import gzip
                        text = gzip.decompress(peek).decode("utf-8", "replace")
                    except Exception:  # noqa: BLE001 — logging must never break the relay
                        pass
                log("  upstream said:", text.replace("\n", " ")[:400])
                self._relay_body(resp.status, resp, peek)
                conn.close()
                return
            self._relay(resp)
            conn.close()
            return

    def _relay_body(self, status, resp, first):
        self.send_response(status)
        length = resp.getheader("content-length")
        for key, value in resp.getheaders():
            if key.lower() in HOP_BY_HOP or key.lower() == "content-length":
                continue
            self.send_header(key, value)
        self.send_header("content-length", str(len(first)))
        self.end_headers()
        self.wfile.write(first)
        try:
            self.wfile.flush()
        except OSError:
            pass


if __name__ == "__main__":
    log(f"broker up on :{PORT} — anthropic={ANTHROPIC_MODE} openai_upstream={OPENAI_UPSTREAM}")
    if ANTHROPIC_MODE == "oauth":
        try:
            anthropic_token()
        except CredentialError as exc:
            log("WARNING:", exc)
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
