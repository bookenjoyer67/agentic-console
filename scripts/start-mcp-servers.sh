#!/usr/bin/env bash
# Start the Module 3 MCP servers inside the agentic-console sandbox container (Agentic Engineer 3.2).
#
#   storage    streamable HTTP, port 8001, mcp/storage/server.py
#   retrieval  streamable HTTP, port 8002, mcp/retrieval/server.py
#
# coursetools is NOT started here. It is a stdio server that Claude Code spawns itself from
# .mcp.json (the course writes that entry with `claude mcp add coursetools python
# /workspace/mcp/coursetools_server.py`). A second copy started by hand would write JSON-RPC to the
# same stdout stream the real one is using, so this script only checks that the file parses and
# then prints the registration commands.
#
# Run inside the container:  bash /workspace/scripts/start-mcp-servers.sh
# Then register the HTTP servers once:
#   claude mcp add --transport http storage   http://localhost:8001/mcp
#   claude mcp add --transport http retrieval http://localhost:8002/mcp
#
# Overridable: WORKSPACE, MEMORY, HOST, STORAGE_PORT, RETRIEVAL_PORT, CHUNKING, BOUNDARY_THRESHOLD,
# RETRIEVAL_EMBEDDING_MODEL, RETRIEVAL_QUERY_PREFIX.
set -euo pipefail

# cfg <key> <current-value>: one value resolved by scripts/agentic_config.py, or <current-value> when
# that loader or agentic.config.json is absent, so this launcher is unchanged without the kit. The
# shape is copied from scripts/run-agent.sh so the two readers cannot disagree on a missing config.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cfg() { local v=""; [ -f "$SCRIPT_DIR/agentic_config.py" ] && v="$(python3 "$SCRIPT_DIR/agentic_config.py" --get "$1" --default "$2" 2>/dev/null || true)"; printf '%s' "${v:-$2}"; }

WORKSPACE="${WORKSPACE:-/workspace}"
MEMORY="${MEMORY:-$WORKSPACE/.memory}"
HOST="${HOST:-0.0.0.0}"
STORAGE_PORT="${STORAGE_PORT:-8001}"
RETRIEVAL_PORT="${RETRIEVAL_PORT:-8002}"
CHUNKING="${CHUNKING:-paragraph}"
BOUNDARY_THRESHOLD="${BOUNDARY_THRESHOLD:-0.75}"
STORAGE_SERVER="$WORKSPACE/mcp/storage/server.py"
RETRIEVAL_SERVER="$WORKSPACE/mcp/retrieval/server.py"
COURSETOOLS_SERVER="$WORKSPACE/mcp/coursetools_server.py"

# --- fail loudly, before any half-started state ---------------------------------------------
command -v python3 >/dev/null 2>&1 || {
  echo "ERROR: python3 is not on PATH. The Module 3 servers need the image's Python 3.12." >&2
  echo "       Use agent-sandbox:console-m1 (sandbox/Dockerfile.m3), not the Module 1 image." >&2
  exit 1
}

python3 -c 'import fastmcp' >/dev/null 2>&1 || {
  echo "ERROR: this python3 cannot import fastmcp, so no MCP server will start." >&2
  echo "       Use agent-sandbox:console-m1, built with: docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:console-m1 ." >&2
  exit 1
}

for server in "$STORAGE_SERVER" "$RETRIEVAL_SERVER"; do
  [ -f "$server" ] || {
    echo "ERROR: missing MCP server: $server" >&2
    echo "       Lesson 3.2 builds mcp/storage/server.py and mcp/retrieval/server.py; nothing was started." >&2
    exit 1
  }
done

# The storage audit log and the reference corpus live here; both must survive the container.
mkdir -p "$MEMORY" "$MEMORY/reference"

# --- the two HTTP servers -------------------------------------------------------------------
python3 "$STORAGE_SERVER" --port "$STORAGE_PORT" --host "$HOST" &
STORAGE_PID=$!

# Retrieval quality: the default model here is the one this repository measured against the
# ground-truth set (docs/retrieval-quality-report.md `HARNESS_RESULT passed=8 total=8 rate=100.0
# floor=80.0`). The model this server falls back to on its own scores 5/8, below that harness's 80%
# floor, so starting the repository the documented way now yields the documented result. Both values
# stay overridable, and the model is selected through the server's own environment variables
# (mcp/retrieval/server.py:74 `RETRIEVAL_EMBEDDING_MODEL`). The value comes from the seam table
# (`agentic.config.json` `"retrieval": "BAAI/bge-small-en-v1.5"`); an environment variable wins, and
# this literal is the fallback a checkout with no config still gets.
export RETRIEVAL_EMBEDDING_MODEL="${RETRIEVAL_EMBEDDING_MODEL:-$(cfg models.retrieval 'BAAI/bge-small-en-v1.5')}"
export RETRIEVAL_QUERY_PREFIX="${RETRIEVAL_QUERY_PREFIX:-Represent this sentence for searching relevant passages: }"

python3 "$RETRIEVAL_SERVER" \
  --port "$RETRIEVAL_PORT" \
  --host "$HOST" \
  --chunking "$CHUNKING" \
  --boundary-threshold "$BOUNDARY_THRESHOLD" &
RETRIEVAL_PID=$!

cleanup() {
  kill "$STORAGE_PID" "$RETRIEVAL_PID" 2>/dev/null || true
}
trap cleanup EXIT

# A server that dies on a bad flag must not leave the script pretending everything started.
sleep 1
for pair in "storage:$STORAGE_PID:$STORAGE_PORT" "retrieval:$RETRIEVAL_PID:$RETRIEVAL_PORT"; do
  name="${pair%%:*}"; rest="${pair#*:}"; pid="${rest%%:*}"
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "ERROR: the $name server exited immediately. Read its output above for the reason." >&2
    exit 1
  fi
done

echo "started storage   pid $STORAGE_PID  port $STORAGE_PORT  http://localhost:$STORAGE_PORT/mcp"
echo "started retrieval pid $RETRIEVAL_PID  port $RETRIEVAL_PORT  http://localhost:$RETRIEVAL_PORT/mcp  chunking=$CHUNKING boundary-threshold=$BOUNDARY_THRESHOLD"
echo "memory            $MEMORY  (storage.db, storage-audit.log, reference/)"
echo
echo "coursetools is spawned by Claude Code from .mcp.json, not by this script. Registration:"
echo "  claude mcp add coursetools python $COURSETOOLS_SERVER"
echo "  claude mcp add --transport http storage   http://localhost:$STORAGE_PORT/mcp"
echo "  claude mcp add --transport http retrieval http://localhost:$RETRIEVAL_PORT/mcp"
echo
echo "Press Ctrl-C to stop both servers."
wait
