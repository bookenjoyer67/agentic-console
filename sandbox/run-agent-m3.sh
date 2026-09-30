#!/usr/bin/env bash
# Module 3 sandbox launcher for Komun.
#
#   ./sandbox/run-agent-m3.sh          # start (or restart) the Module 3 container
#   ./sandbox/run-agent-m3.sh shell    # start, then drop into a shell inside it
#
# Every command here was run and its output checked while building Module 3.1. Differences from the
# Module 1 launcher (sandbox/run-agent.sh):
#
#   * image agent-sandbox:komun-m3 — the Module 1 image plus Python 3.12, the MCP framework and the
#     baked embedding model (sandbox/Dockerfile.m3).
#   * two networks. agent-internal carries the loopback traffic between the agent and the MCP servers
#     and forbids egress; agent-net is attached only so the container can reach the credential broker
#     on rev-broker:4000. Measured inside the container: broker /health -> 200, https://example.com -> 000.
#   * no port publishing. On this host, publishing a port from an internal-only network produces no
#     mapping, and attaching the default bridge to fix that restores egress. Run the MCP Inspector
#     inside the container instead.
#
# Credentials: the same broker pattern as Module 1. The container holds a dummy token, never a real key.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${IMAGE:-agent-sandbox:komun-m3}"
NAME="${NAME:-agent-rev-m3}"
BROKER="${BROKER:-rev-broker}"
BROKER_PORT="${BROKER_PORT:-4000}"

command -v docker >/dev/null || { echo "docker is not installed"; exit 1; }
docker info >/dev/null 2>&1 || { echo "docker daemon is not running (sudo systemctl start docker)"; exit 1; }

docker network inspect agent-internal >/dev/null 2>&1 || { docker network create --internal agent-internal; }
docker network inspect agent-net >/dev/null 2>&1 || { echo "missing network agent-net (the Module 1 launcher creates it)"; exit 1; }

docker rm -f "$NAME" >/dev/null 2>&1 || true

docker run -dit --name "$NAME" \
  --network agent-internal \
  -e ANTHROPIC_BASE_URL="http://${BROKER}:${BROKER_PORT}" \
  -e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \
  -e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  -v "${REPO}:/workspace" \
  -v rev-cargo-target:/workspace/target \
  -v komun-cargo-registry:/usr/local/cargo/registry \
  "$IMAGE" >/dev/null

docker network connect agent-net "$NAME"

# Trust + model cache: the same file the long-lived Module 1 container already holds, so a headless run
# does not stall on onboarding. It carries no credentials.
if docker inspect agent-rev >/dev/null 2>&1; then
  docker exec -u 0 agent-rev cat /root/.claude.json > /tmp/m3-claude.json 2>/dev/null || echo '{}' > /tmp/m3-claude.json
  docker cp /tmp/m3-claude.json "${NAME}:/root/.claude.json" >/dev/null
  rm -f /tmp/m3-claude.json
fi

echo "container : $NAME ($IMAGE)"
echo "networks  : agent-internal (MCP loopback, no egress) + agent-net (broker only)"
echo "workspace : $REPO -> /workspace"
echo "broker    : http://${BROKER}:${BROKER_PORT} (dummy token inside; no key in this container)"
echo
echo "start the MCP servers with:  docker exec -w /workspace $NAME bash /workspace/scripts/start-mcp-servers.sh"
echo "verify containment with:     docker exec $NAME sh -c 'curl -s -o /dev/null -w \"broker=%{http_code}\\n\" http://${BROKER}:${BROKER_PORT}/health; curl -s -o /dev/null -w \"egress=%{http_code}\\n\" https://example.com'"
echo

if [ "${1:-}" = "shell" ]; then
  exec docker exec -it -w /workspace "$NAME" bash
fi
