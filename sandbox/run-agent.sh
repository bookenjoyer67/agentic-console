#!/usr/bin/env bash
# Start one agent sandbox for this repository, with a shared credential broker
# and no egress from the agent container.
#
#   agent container  : --network console-net (--internal, no internet at all)
#   broker container : console-net + bridge; holds the real keys, injects them upstream
#
# The agent container receives NO credential mounts and NO real key material —
# only a dummy token and the broker's in-network URL.
#
# One workspace per container. Parallel sessions run this script twice with
# different REPO values (e.g. two git worktrees of the same repo); each container
# sees its own checkout as /workspace and shares nothing but the broker.
#
#   ~/agentic-console/sandbox/run-agent.sh                      # repo         -> agent-agentic-console
#   REPO=$HOME/agentic-console ~/agentic-console/sandbox/run-agent.sh  # repo  -> agent-agentic-console
#   REPO=$HOME/worktree-b $HOME/agentic-console/sandbox/run-agent.sh
#   AGENT_NAME=my-name REPO=... run-agent.sh                    # override the container name
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${REPO:-$HOME/agentic-console}" && pwd)"
STATE="${HOME}/.config/console-sandbox"
NET=console-net
IMAGE="${IMAGE:-agent-sandbox:console-m1}"
BROKER_IMAGE="${BROKER_IMAGE:-console-sandbox-broker:local}"
BROKER_NAME="${BROKER_NAME:-console-broker}"

# container name derived from the workspace, so two worktrees never collide
SLUG="$(basename "$REPO")"
AGENT_NAME="${AGENT_NAME:-agent-$SLUG}"
# cargo build cache: one per workspace. Two agents sharing one target dir would
# serialise on cargo's build lock, which is the opposite of parallel sessions.
TARGET_VOL="${TARGET_VOL:-$SLUG-cargo-target}"

command -v docker >/dev/null || { echo "docker not on PATH"; exit 1; }
docker info >/dev/null 2>&1 || { echo "docker daemon not running: sudo systemctl start docker"; exit 1; }
[ -d "$REPO" ] || { echo "no such workspace: $REPO"; exit 1; }

# 1. internal network: reachable by the agents, but with no route off the host
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create --internal "$NET" >/dev/null
  echo "created internal network $NET"
fi

# 2. broker image, then the broker itself — started once and left alone, so
#    launching a second agent does not interrupt the first one mid-request.
docker image inspect "$BROKER_IMAGE" >/dev/null 2>&1 || {
  echo "building broker image..."
  docker build -t "$BROKER_IMAGE" "$SCRIPT_DIR/broker" >/dev/null
}
if docker inspect "$BROKER_NAME" >/dev/null 2>&1 && \
   [ "$(docker inspect -f '{{.State.Running}}' "$BROKER_NAME")" = "true" ]; then
  echo "broker already up ($BROKER_NAME) — reusing it"
else
  docker rm -f "$BROKER_NAME" >/dev/null 2>&1 || true
  docker run -d --name "$BROKER_NAME" \
    --network "$NET" \
    --user 1000:1000 \
    -v "${HOME}/.claude:/secrets/claude" \
    -v "${STATE}:/state" \
    -e CLAUDE_CRED=/secrets/claude/.credentials.json \
    -e OPENAI_KEY_FILE=/state/deepseek.key \
    -e BACKUP_DIR=/state/backups \
    "$BROKER_IMAGE" >/dev/null
  # second network gives the broker (and only the broker) a route to the internet
  docker network connect bridge "$BROKER_NAME"
  echo "broker up ($BROKER_NAME)"
fi
BROKER_HOST="$BROKER_NAME"

# 3. Is this workspace a git worktree? Then its .git is a *file* pointing at an
#    absolute path inside the main repo. That path has to exist at the same
#    absolute location in the container or every git command inside fails with
#    "not a git repository". Mount the main git dir read-write alongside it.
GIT_MOUNT=()
if [ -f "$REPO/.git" ]; then
  GITDIR="$(sed -n 's/^gitdir: //p' "$REPO/.git" | head -1)"
  MAIN_GIT="${GITDIR%%/worktrees/*}"
  if [ -n "$MAIN_GIT" ] && [ -d "$MAIN_GIT" ]; then
    GIT_MOUNT=(-v "$MAIN_GIT:$MAIN_GIT")
    echo "worktree detected: mounting shared git dir $MAIN_GIT (same path inside)"
  fi
fi

# 4. agent: internal network only, dummy token, no secret mounts
docker rm -f "$AGENT_NAME" >/dev/null 2>&1 || true
docker run -dit --name "$AGENT_NAME" \
  --network "$NET" \
  -e ANTHROPIC_BASE_URL="http://$BROKER_HOST:4000" \
  -e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \
  -e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  -v "$REPO:/workspace" \
  "${GIT_MOUNT[@]}" \
  -v "$TARGET_VOL:/workspace/target" \
  -v console-cargo-registry:/usr/local/cargo/registry \
  -v "$SCRIPT_DIR/opencode-sandbox.json:/root/.config/opencode/opencode.json:ro" \
  "$IMAGE" >/dev/null

# trust + onboarding for the mounted workspace, plus the account's model
# entitlement cache. Without that cache — and with no egress to fetch it — Claude
# Code resolves aliases like `--model opus` to a model the account is not entitled
# to (claude-opus-5-5) and the call fails. Only these fields are copied; no tokens.
python3 - "$HOME/.claude.json" /tmp/sandbox-claude.json <<'PY'
import json, sys
src, dst = sys.argv[1], sys.argv[2]
h = {}
try:
    h = json.load(open(src))
except OSError:
    pass
keep = {
    "hasCompletedOnboarding": True,
    "lastOnboardingVersion": h.get("lastOnboardingVersion"),
    "modelAccessCache": h.get("modelAccessCache"),
    "additionalModelOptionsCache": h.get("additionalModelOptionsCache"),
    "additionalModelOptionsAnsweredAt": h.get("additionalModelOptionsAnsweredAt"),
    "projects": {"/workspace": {"hasTrustDialogAccepted": True}},
}
json.dump(keep, open(dst, "w"), indent=1)
PY
docker cp /tmp/sandbox-claude.json "$AGENT_NAME:/root/.claude.json"
rm -f /tmp/sandbox-claude.json

# 5. Permission profile, pre-granted inside the container instead of approved
#    interactively. An interactive approval makes Claude Code PERSIST the grant
#    into the MEASURED workspace as .claude/settings.local.json — a write into
#    the repo that no prompt can prevent, which is why containment (G1) failed in
#    both baseline runs. This file lives in the container's own /root, never in
#    the bind mount, and it is identical for every container, so the prompt stays
#    the only variable between parallel runs.
cat > /tmp/sandbox-settings.json <<'JSON'
{
  "statusLine": { "type": "command", "command": "bash /root/.claude/statusline.sh", "padding": 0 },
  "permissions": {
    "allow": ["Bash", "Read", "Glob", "Grep", "Write", "Edit", "MultiEdit", "TodoWrite"]
  }
}
JSON
docker cp /tmp/sandbox-settings.json "$AGENT_NAME:/root/.claude/settings.json"
rm -f /tmp/sandbox-settings.json

echo
echo "workspace : $REPO  ->  /workspace"
echo "container : $AGENT_NAME   (network $NET, no internet)"
echo "claude    : docker exec -it $AGENT_NAME claude --model opus"
echo "opencode  : docker exec -it $AGENT_NAME opencode run -m sandbox/deepseek-v4-flash \"...\""
echo "shell     : docker exec -it $AGENT_NAME bash"
echo "health    : docker exec $AGENT_NAME curl -s http://$BROKER_HOST:4000/health"
echo "logs      : docker logs -f $BROKER_NAME"
echo "stop      : docker rm -f $AGENT_NAME"
