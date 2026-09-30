#!/usr/bin/env bash
# Compare retrieval quality across chunking strategies for the Agentic Engineer 3.2 exercise.
#
#   paragraph (default) vs semantic, over the same corpus, ground-truth set and ceiling rules
#
# The harness needs a live server, so this script starts one retrieval server per mode on the
# same port, waits for it to answer, runs mcp/retrieval/run_ground_truth.py against it, stops
# it, and prints both pass rates side by side at the end.
#
# Run inside the sandbox container (or on a host that has fastmcp, fastembed and sqlite-vec):
#
#   bash scripts/run-retrieval-comparison.sh                 # paragraph, then semantic
#   bash scripts/run-retrieval-comparison.sh semantic        # one mode only
#
# Overridable: PYTHON, PORT, HOST, MEMORY, RETRIEVAL_REFERENCE_DIR, GROUND_TRUTH, TOP_K,
# BOUNDARY_THRESHOLD, LOG_DIR, READY_TIMEOUT.
#
# Exit status: 0 when every requested mode was measured; 1 when a mode could not be measured
# (server never answered, or the harness produced no pass-rate line), plus a per-mode table.

set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

PYTHON="${PYTHON:-python3}"
PORT="${PORT:-8002}"
HOST="${HOST:-127.0.0.1}"

# A server already listening on $PORT makes this script measure THAT server instead of the one it
# starts: the child dies with "address already in use", wait_for_port still succeeds, and both chunking
# modes then report identical numbers from one process. Measured on this repo: paragraph and semantic
# produced byte-identical output that way. Move to a free port instead of reporting a false comparison.
port_in_use() {
  "$PYTHON" -c "
import socket, sys
s = socket.socket(); s.settimeout(0.5)
sys.exit(0 if s.connect_ex(('$1', int('$2'))) == 0 else 1)"
}
if port_in_use "$HOST" "$PORT"; then
  echo "note: $HOST:$PORT is already serving something; a free port is used for this measurement"
  PORT="$("$PYTHON" -c 'import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
  echo "      measuring on port $PORT"
fi
BOUNDARY_THRESHOLD="${BOUNDARY_THRESHOLD:-0.75}"
MEMORY="${MEMORY:-$REPO_ROOT/.memory}"
REFERENCE_DIR="${RETRIEVAL_REFERENCE_DIR:-$MEMORY/reference}"
GROUND_TRUTH="${GROUND_TRUTH:-$REPO_ROOT/docs/retrieval-ground-truth.md}"
TOP_K="${TOP_K:-3}"
READY_TIMEOUT="${READY_TIMEOUT:-120}"
LOG_DIR="${LOG_DIR:-$(mktemp -d)}"

SERVER_PY="$REPO_ROOT/mcp/retrieval/server.py"
HARNESS_PY="$REPO_ROOT/mcp/retrieval/run_ground_truth.py"

MODES=("$@")
if [ "${#MODES[@]}" -eq 0 ]; then
  MODES=(paragraph semantic)
fi

# The per-mode server and harness output is written here; a caller-supplied LOG_DIR must exist.
mkdir -p "$LOG_DIR" || {
  echo "ERROR: cannot create the log directory: $LOG_DIR" >&2
  exit 1
}

echo "retrieval chunking comparison — Agentic Engineer module 3.2"
echo "  repository    $REPO_ROOT"
echo "  python        $($PYTHON --version 2>&1)"
echo "  corpus        $REFERENCE_DIR"
echo "  ground truth  $GROUND_TRUTH"
echo "  endpoint      http://$HOST:$PORT/mcp   top_k=$TOP_K"
echo "  boundary      $BOUNDARY_THRESHOLD (semantic mode)"
echo "  logs          $LOG_DIR"
echo

for path in "$SERVER_PY" "$HARNESS_PY" "$GROUND_TRUTH"; do
  [ -f "$path" ] || {
    echo "ERROR: missing required file: $path" >&2
    exit 1
  }
done

[ -d "$REFERENCE_DIR" ] || {
  echo "ERROR: reference corpus not found: $REFERENCE_DIR" >&2
  echo "       The retrieval server cannot index an empty path." >&2
  exit 1
}

port_is_open() {
  "$PYTHON" - "$HOST" "$PORT" <<'PY'
import socket, sys
sock = socket.socket()
sock.settimeout(1.0)
try:
    sock.connect((sys.argv[1], int(sys.argv[2])))
    print("open")
except OSError:
    print("closed")
finally:
    sock.close()
PY
}

wait_for_port() {
  local deadline=$((SECONDS + READY_TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    [ "$(port_is_open)" = "open" ] && return 0
    kill -0 "$1" 2>/dev/null || return 1
    sleep 1
  done
  return 1
}

wait_for_port_to_close() {
  local deadline=$((SECONDS + 15))
  while [ "$SECONDS" -lt "$deadline" ]; do
    [ "$(port_is_open)" = "closed" ] && return 0
    sleep 1
  done
  return 1
}

MODES_MEASURED=()
FAILED_MODES=()
declare -A PASSED=() TOTAL=() RATE=() INDEX_LINE=() BELOW_FLOOR=()

for mode in "${MODES[@]}"; do
  server_log="$LOG_DIR/$mode-server.log"
  harness_out="$LOG_DIR/$mode-harness.txt"

  "$PYTHON" "$SERVER_PY" \
    --port "$PORT" \
    --host "$HOST" \
    --chunking "$mode" \
    --boundary-threshold "$BOUNDARY_THRESHOLD" \
    --reference-dir "$REFERENCE_DIR" >"$server_log" 2>&1 &
  server_pid=$!

  if ! wait_for_port "$server_pid"; then
    echo "== $mode chunking: server did not answer on $HOST:$PORT =="
    echo "   last lines of $server_log:"
    tail -n 15 "$server_log" | sed 's/^/   | /'
    echo
    kill "$server_pid" 2>/dev/null
    wait "$server_pid" 2>/dev/null
    FAILED_MODES+=("$mode")
    continue
  fi

  echo "== $mode chunking (boundary-threshold $BOUNDARY_THRESHOLD) =="
  grep -m1 '^indexed ' "$server_log" | sed 's/^/   index: /'

  "$PYTHON" "$HARNESS_PY" \
    --server "http://$HOST:$PORT/mcp" \
    --file "$GROUND_TRUTH" \
    --top-k "$TOP_K" 2>&1 | tee "$harness_out"
  harness_status="${PIPESTATUS[0]}"

  kill "$server_pid" 2>/dev/null
  wait "$server_pid" 2>/dev/null
  wait_for_port_to_close || true

  result_line="$(grep -m1 '^HARNESS_RESULT ' "$harness_out" || true)"
  if [ -z "$result_line" ]; then
    echo "   no pass-rate line was produced (harness exit $harness_status)"
    echo
    FAILED_MODES+=("$mode")
    continue
  fi

  # HARNESS_RESULT passed=4 total=5 rate=80.0 floor=80.0
  passed="$(sed -n 's/.* passed=\([0-9]*\).*/\1/p' <<<"$result_line")"
  total="$(sed -n 's/.* total=\([0-9]*\).*/\1/p' <<<"$result_line")"
  rate="$(sed -n 's/.* rate=\([0-9.]*\).*/\1/p' <<<"$result_line")"
  floor="$(sed -n 's/.* floor=\([0-9.]*\).*/\1/p' <<<"$result_line")"

  MODES_MEASURED+=("$mode")
  PASSED["$mode"]="$passed"
  TOTAL["$mode"]="$total"
  RATE["$mode"]="$rate"
  INDEX_LINE["$mode"]="$(grep -m1 '^indexed ' "$server_log" || echo '(no index line)')"
  BELOW_FLOOR["$mode"]="$(awk -v rate="$rate" -v floor="$floor" 'BEGIN { print (rate + 0 < floor + 0) ? "yes" : "no" }')"

  if [ "$harness_status" -ne 0 ]; then
    echo "   harness exited $harness_status for $mode (below the floor, or it could not run)"
  fi
  echo
done

echo "chunking mode comparison"
printf '%-12s %-14s %-10s %s\n' "mode" "passed/total" "pass rate" "verdict"
for mode in "${MODES[@]}"; do
  if [ -n "${RATE[$mode]:-}" ]; then
    verdict="passes the 80% floor"
    [ "${BELOW_FLOOR[$mode]}" = "yes" ] && verdict="BELOW the 80% floor"
    printf '%-12s %-14s %-10s %s\n' \
      "$mode" \
      "${PASSED[$mode]}/${TOTAL[$mode]}" \
      "${RATE[$mode]}%" \
      "$verdict"
  else
    printf '%-12s %-14s %-10s %s\n' "$mode" "n/a" "n/a" "NOT MEASURED"
  fi
done
echo
echo "index size per mode:"
for mode in "${MODES[@]}"; do
  printf '  %-10s %s\n' "$mode" "${INDEX_LINE[$mode]:-n/a}"
done

if [ "${#FAILED_MODES[@]}" -gt 0 ]; then
  echo
  echo "FAILED to measure: ${FAILED_MODES[*]}" >&2
  echo "Read the server logs under $LOG_DIR and fix the cause before comparing modes." >&2
  exit 1
fi

echo
echo "both modes measured ($(IFS=,; echo "${MODES_MEASURED[*]}"))."
echo "Keep paragraph chunking as the default unless semantic wins on this set."
