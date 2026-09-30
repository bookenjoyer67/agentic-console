#!/usr/bin/env bash
# Stage the credentials the BROKER needs, on the host, outside the repo.
# Nothing here is ever mounted into the agent container.
#
#   ~/.config/console-sandbox/
#     deepseek.key          copied out of opencode's auth.json (mode 600)
#     backups/              broker keeps a copy of the credential file before each refresh
#
# The Claude Code credential is NOT copied: the broker mounts ~/.claude directly
# read-write so that a refresh in the sandbox and a refresh on the host stay one
# lineage (OAuth refresh tokens rotate — two copies would invalidate each other).
set -euo pipefail

STATE="${HOME}/.config/console-sandbox"
AUTH_JSON="${HOME}/.local/share/opencode/auth.json"
CLAUDE_CRED="${HOME}/.claude/.credentials.json"

mkdir -p "$STATE/backups"
chmod 700 "$STATE" "$STATE/backups"

python3 - "$AUTH_JSON" "$STATE/deepseek.key" <<'PY'
import json, os, stat, sys
src, dst = sys.argv[1], sys.argv[2]
try:
    data = json.load(open(src))
except OSError:
    print("  opencode auth.json not found — deepseek route will report no key")
    sys.exit(0)
entry = data.get("deepseek") or {}
key = entry.get("key")
if not key:
    print("  no deepseek entry in opencode auth.json — deepseek route will report no key")
    sys.exit(0)
fd = os.open(dst, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as fh:
    fh.write(key + "\n")
print(f"  deepseek.key staged ({len(key)} chars, mode 600)")
PY

if [ -f "$CLAUDE_CRED" ]; then
  echo "  claude credential present at $CLAUDE_CRED (broker mounts ~/.claude directly)"
else
  echo "  WARNING: no $CLAUDE_CRED — run \`claude\` on the host once to log in" >&2
fi

echo "staged in $STATE:"
ls -l "$STATE" | sed 's/^/  /'
