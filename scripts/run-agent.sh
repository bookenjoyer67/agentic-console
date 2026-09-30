#!/usr/bin/env bash
# scripts/run-agent.sh — launch one agent container for one named role:
#   ./scripts/run-agent.sh <role> <command>
#   ./scripts/run-agent.sh --matrix          # the per-role matrix, one row per role
#   ./scripts/run-agent.sh --print-config    # the config keys this script consumes, as one JSON object
# The engine is sandbox/run-agent.sh (internal network, credential broker, dummy token, no secret mounts).
#
# This wrapper adds the agent-sandbox:console-m1 image, the console-internal + console-net pair and the per-role
# mount set. The mount is the enforcement, not a permission bit: the container runs as root, and a root
# process ignores a read-only file mode (`docs/calibration-log.md:86` `Enforce the read-only memory layers
# with a hard stop, because NM-9 showed permission bits a root process ignores`). The per-role rationale,
# the policy line each mode cites and every portability seam are in PORTING.md.
#
# Idempotent: a running container whose mounts already match the role's profile is reused; any other
# container of that name is removed and recreated. Re-running never fails on a taken name. The container
# holds a dummy token and no key material; credentials come from the broker over console-net
# (`sandbox/run-agent.sh:86` `agent: internal network only, dummy token, no secret mounts`).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${REPO:-$SCRIPT_DIR/..}" && pwd)"
# cfg <key> <current-value>: one value resolved by scripts/agentic_config.py, or <current-value> when
# that loader or agentic.config.json is absent, so the launcher is unchanged without the kit. Every
# binding below passes this repository's current value; an environment variable wins over both.
cfg() { local v=""; [ -f "$SCRIPT_DIR/agentic_config.py" ] && v="$(python3 "$SCRIPT_DIR/agentic_config.py" --get "$1" --default "$2" 2>/dev/null || true)"; printf '%s' "${v:-$2}"; }
IMAGE="${IMAGE:-$(cfg containers.tools_image 'agent-sandbox:console-m1')}"
BROKER="${BROKER:-$(cfg containers.broker.name 'console-broker')}"
BROKER_PORT="${BROKER_PORT:-$(cfg containers.broker.port '4000')}"
TARGET_VOL="${TARGET_VOL:-rev-cargo-target}"
REGISTRY_VOL="${REGISTRY_VOL:-$(cfg containers.registry_volume 'console-cargo-registry')}"
OPENCODE_JSON="$SCRIPT_DIR/../sandbox/opencode-sandbox.json"
TMP="${TMPDIR:-/tmp}"
VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"
VALID_CMDS="bash sh claude opencode"
NET_INTERNAL="${NET_INTERNAL:-$(cfg containers.networks.internal 'console-internal')}"
NET_BROKER="${NET_BROKER:-$(cfg containers.networks.broker 'console-net')}"
WORKSPACE="$(cfg containers.workspace '/workspace')"
MEMORY_DIR="$(cfg containers.memory_dir '/workspace/.memory')"

# --print-config: the config keys this script consumes, each resolved, as one JSON object. The role
# matrix is the effective one, so a fork can prove what the launcher actually applied.
print_config() {
  local role sep=""
  printf '{\n  "containers.tools_image": "%s",\n  "containers.broker.name": "%s",\n' "$IMAGE" "$BROKER"
  printf '  "containers.broker.port": "%s",\n  "containers.registry_volume": "%s",\n' "$BROKER_PORT" "$REGISTRY_VOL"
  printf '  "containers.networks.internal": "%s",\n  "containers.networks.broker": "%s",\n' "$NET_INTERNAL" "$NET_BROKER"
  printf '  "containers.workspace": "%s",\n  "containers.memory_dir": "%s",\n' "$WORKSPACE" "$MEMORY_DIR"
  printf '  "roles.mounts": {'
  for role in $VALID_ROLES; do
    role_profile "$role" || continue
    printf '%s\n    "%s": {"workspace": "%s", "memory": "%s", "build_cache": "%s"}' \
      "$sep" "$role" "$ROLE_WS" "$ROLE_MEM" "$ROLE_TARGET"
    sep=","
  done
  printf '\n  }\n}\n'
}

usage() {
  printf 'usage: ./scripts/run-agent.sh <role> <command>\n\n'
  printf '  <role>     %s\n' "$VALID_ROLES"
  printf '  <command>  %s\n\n' "$VALID_CMDS"
  printf 'examples:\n'
  printf '  ./scripts/run-agent.sh reviewer bash\n'
  printf '  ./scripts/run-agent.sh implementer bash -c "touch /workspace/ok-to-write.txt"\n'
  printf '  ./scripts/run-agent.sh --matrix\n'
}

MATRIX_ROWS="$(cat <<'ROWS'
| `orchestrator` | `/workspace` read-write | `/workspace/.memory` read-only | console-internal + console-net (broker only) | Policy grants workspace writes and no memory write (`docs/governance-policy.md:67` `holds no memory write grant`). |
| `planner` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | console-internal + console-net (broker only) | Policy grants one plan entry and denies a workspace write (`docs/governance-policy.md:110` `writes one plan entry into`). |
| `implementer` | `/workspace` read-write | `/workspace/.memory` read-write | console-internal + console-net (broker only) | Policy grants workspace writes and memory entry writes (`docs/governance-policy.md:157` `writes and revises its own entries in`). |
| `tester` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | console-internal + console-net (broker only) | Policy grants one result entry and denies a workspace write (`docs/governance-policy.md:205` `writes one test-result entry into`). |
| `reviewer` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | console-internal + console-net (broker only) | Policy grants one review entry and denies a workspace write (`docs/governance-policy.md:251` `writes one review entry into`). |
| `project-manager` | `/workspace` read-only | not mounted (visible read-only through the workspace bind) | console-internal + console-net (broker only) | Policy grants memory reads and no write (`docs/governance-policy.md:295` `reads stored entries from`). |
| `researcher` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | console-internal + console-net (broker only) | Policy grants one research entry and denies a repository read (`docs/governance-policy.md:340` `writes one `public` research entry into`). |
ROWS
)"

print_matrix() {
  printf '| Role | Workspace mount | Memory mount | Network | Reason |\n'
  printf '|---|---|---|---|---|\n'
  printf '%s\n' "$MATRIX_ROWS"
}

# Per-role profile: workspace mode, memory-layer mode, build-cache mode.
#   none = no mount at all; the path stays visible read-only through the workspace bind.
# The case block is this repository's embedded matrix and the fallback when the loader or the config is
# absent; the loop after it then overrides each dimension from roles.mounts.<role>. ROLE_MEM=rw is derived
# from the grant map: exactly the roles the map gives mcp__storage__write_entry hold a writable memory
# path (`docs/routing-and-tool-grant-map.json:17` `"mcp__storage__write_entry"`), which is planner,
# implementer, tester, reviewer and researcher.
role_profile() {
  case "$1" in
    orchestrator)    ROLE_WS=rw; ROLE_MEM=ro;   ROLE_TARGET=ro ;;
    planner)         ROLE_WS=ro; ROLE_MEM=rw;   ROLE_TARGET=ro ;;
    implementer)     ROLE_WS=rw; ROLE_MEM=rw;   ROLE_TARGET=ro ;;
    tester)          ROLE_WS=ro; ROLE_MEM=rw;   ROLE_TARGET=rw ;;
    reviewer)        ROLE_WS=ro; ROLE_MEM=rw;   ROLE_TARGET=ro ;;
    project-manager) ROLE_WS=ro; ROLE_MEM=none; ROLE_TARGET=ro ;;
    researcher)      ROLE_WS=ro; ROLE_MEM=rw;   ROLE_TARGET=ro ;;
    *) return 1 ;;
  esac
  local dim var mode; for dim in workspace:ROLE_WS memory:ROLE_MEM build_cache:ROLE_TARGET; do
    var="${dim#*:}"; mode="$(cfg "roles.mounts.$1.${dim%%:*}" "${!var}")"; [ -z "$mode" ] || printf -v "$var" '%s' "$mode"
  done
}

if [ $# -eq 0 ]; then
  usage
  exit 2
fi

case "$1" in
  -h|--help) usage; exit 0 ;;
  --matrix)  print_matrix; exit 0 ;;
  --print-config) print_config; exit 0 ;;
esac

ROLE="$1"
if ! role_profile "$ROLE"; then
  printf 'error: unknown role "%s"\n\n' "$ROLE" >&2
  printf 'valid roles: %s\n\n' "$VALID_ROLES" >&2
  usage >&2
  exit 2
fi

CMDLINE="${2:-}"
if [ -z "$CMDLINE" ]; then
  printf 'error: no command given for role "%s"\n\n' "$ROLE" >&2
  printf 'valid commands: %s\n\n' "$VALID_CMDS" >&2
  usage >&2
  exit 2
fi
case "$CMDLINE" in
  bash|sh|claude|opencode) ;;
  *)
    printf 'error: unknown command "%s" for role "%s"\n\n' "$CMDLINE" "$ROLE" >&2
    printf 'valid commands: %s\n\n' "$VALID_CMDS" >&2
    usage >&2
    exit 2
    ;;
esac

NAME="${NAME:-agent-rev-m4-$ROLE}"
EXTRA=("${@:3}")

command -v docker >/dev/null || { printf 'docker is not on PATH\n' >&2; exit 1; }
docker info >/dev/null 2>&1 || { printf 'docker daemon is not running (sudo systemctl start docker)\n' >&2; exit 1; }
[ -d "$REPO" ] || { printf 'no such workspace: %s\n' "$REPO" >&2; exit 1; }
[ -d "$REPO/.memory" ] || { printf 'no memory layer at %s/.memory\n' "$REPO" >&2; exit 1; }
# 1. Both networks, as sandbox/run-agent-m3.sh:31-32. console-internal carries the MCP loopback traffic and forbids egress; console-net exists so the container can reach the credential broker.
docker network inspect "$NET_INTERNAL" >/dev/null 2>&1 || docker network create --internal "$NET_INTERNAL" >/dev/null
if ! docker network inspect "$NET_BROKER" >/dev/null 2>&1; then
  printf 'missing network %s (sandbox/run-agent.sh creates it, with the broker)\n' "$NET_BROKER" >&2
  exit 1
fi
if ! docker inspect "$BROKER" >/dev/null 2>&1 || [ "$(docker inspect -f '{{.State.Running}}' "$BROKER")" != true ]; then
  printf 'broker %s is not running: start it with sandbox/run-agent.sh\n' "$BROKER" >&2
  exit 1
fi
declare -a MOUNTS=()
if [ "$ROLE_WS" = rw ]; then
  MOUNTS+=(-v "$REPO:/workspace")
else
  MOUNTS+=(-v "$REPO:/workspace:ro")
fi
case "$ROLE_MEM" in
  rw)   MOUNTS+=(-v "$REPO/.memory:/workspace/.memory") ;;
  ro)   MOUNTS+=(-v "$REPO/.memory:/workspace/.memory:ro") ;;
  none) : ;;
esac
if [ "$ROLE_TARGET" = rw ]; then
  MOUNTS+=(-v "$TARGET_VOL:$WORKSPACE/target")
  MOUNTS+=(-v "$REGISTRY_VOL:/usr/local/cargo/registry")
else
  MOUNTS+=(-v "$TARGET_VOL:$WORKSPACE/target:ro")
  MOUNTS+=(-v "$REGISTRY_VOL:/usr/local/cargo/registry:ro")
fi
if [ -f "$OPENCODE_JSON" ]; then
  MOUNTS+=(-v "$OPENCODE_JSON:/root/.config/opencode/opencode.json:ro")
fi

# The read-only overlays: the grant authority and the audit journals. A read-write workspace, or a
# read-write memory bind, would otherwise let a role rewrite the files its own grants are read from, or
# erase the journal line that recorded its refusal. Each path is a nested read-only bind over the parent
# bind, so the read-only mount wins for that path alone.
declare -a OVERLAY_FILES=(
  "mcp/storage/allow-list.json"
  "mcp/retrieval/allow-list.json"
  "mcp/roles.allowlist.json"
  "docs/routing-and-tool-grant-map.json"
  ".memory/storage-audit.log"
  ".memory/retrieval-audit.log"
  ".memory/gate-audit.log"
)
declare -a OVERLAY_MOUNTED=()
for overlay in "${OVERLAY_FILES[@]}"; do
  if [ -f "$REPO/$overlay" ]; then
    MOUNTS+=(-v "$REPO/$overlay:$WORKSPACE/$overlay:ro")
    OVERLAY_MOUNTED+=("$overlay")
  fi
done

# 3. Expected mode of the two mounts this launcher is judged on, in docker inspect terms.
WANT_WS_RW=true
[ "$ROLE_WS" = ro ] && WANT_WS_RW=false
WANT_MEM_PRESENT=yes
WANT_MEM_RW=true
case "$ROLE_MEM" in
  ro)   WANT_MEM_RW=false ;;
  none) WANT_MEM_PRESENT=no ;;
esac

# 4. Reuse or recreate. A running container whose two mounts already match is reused, so a second launch
#    of the same role is a no-op instead of a name collision.
ws_rw=""; mem_present=""; mem_rw=""; overlays_ro=yes
if docker inspect "$NAME" >/dev/null 2>&1; then
  running_now="$(docker inspect -f '{{.State.Running}}' "$NAME")"
  ws_rw="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$WORKSPACE\"}}{{.RW}}{{end}}{{end}}" "$NAME")"
  mem_present="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$MEMORY_DIR\"}}yes{{end}}{{end}}" "$NAME")"
  mem_rw="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$MEMORY_DIR\"}}{{.RW}}{{end}}{{end}}" "$NAME")"
  [ -n "$mem_present" ] || mem_present=no
  for overlay in "${OVERLAY_MOUNTED[@]}"; do
    mnt_rw="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$WORKSPACE/$overlay\"}}{{.RW}}{{end}}{{end}}" "$NAME")"
    [ "$mnt_rw" = false ] || overlays_ro=no
  done
  if [ "$running_now" = true ] && [ "$ws_rw" = "$WANT_WS_RW" ] && [ "$mem_present" = "$WANT_MEM_PRESENT" ] && \
     [ "$overlays_ro" = yes ] && \
     { [ "$WANT_MEM_PRESENT" = no ] || [ "$mem_rw" = "$WANT_MEM_RW" ]; }; then
    printf 'container %s is already running with the %s profile — reusing it\n' "$NAME" "$ROLE"
  else
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    RECREATE=yes
  fi
else
  RECREATE=yes
fi

if [ "${RECREATE:-no}" = yes ]; then
  docker run -dit --name "$NAME" \
    --network "$NET_INTERNAL" \
    -e ANTHROPIC_BASE_URL="http://${BROKER}:${BROKER_PORT}" \
    -e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \
    -e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
    -e AGENT_ROLE="$ROLE" \
    "${MOUNTS[@]}" \
    "$IMAGE" >/dev/null
  docker network connect "$NET_BROKER" "$NAME"
  # Trust + model-entitlement cache, the same file sandbox/run-agent-m3.sh:50-54 copies, so a headless
  # run does not stall on onboarding. It carries no credentials.
  if docker inspect agent-rev-m3 >/dev/null 2>&1; then
    if docker exec -u 0 agent-rev-m3 cat /root/.claude.json > "$TMP/m4-claude-$ROLE.json" 2>/dev/null; then
      # Seed through an exec, not docker cp: the daemon's archive path refuses this container once a
      # read-only file overlay sits inside a read-only workspace, and aborts the launch under set -e
      # (measured: `Error response from daemon: openat workspace/docs/routing-and-tool-grant-map.json:
      # read-only file system`). The exec writes to the container's own layer and always succeeds.
      docker exec -i "$NAME" sh -c 'cat > /root/.claude.json' < "$TMP/m4-claude-$ROLE.json"
    fi
    rm -f "$TMP/m4-claude-$ROLE.json"
  fi
fi

# 5. The effective modes, read back from the container this script just ensured — not from intent.
eff_ws="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$WORKSPACE\"}}{{if .RW}}read-write{{else}}read-only{{end}}{{end}}{{end}}" "$NAME")"
eff_mem="$(docker inspect -f "{{range .Mounts}}{{if eq .Destination \"$MEMORY_DIR\"}}{{if .RW}}mounted read-write{{else}}mounted read-only{{end}}{{end}}{{end}}" "$NAME")"
[ -n "$eff_mem" ] || eff_mem="not mounted (visible read-only through the workspace bind)"

printf 'role      : %s\n' "$ROLE"
printf 'image     : %s\n' "$IMAGE"
printf 'container : %s\n' "$NAME"
printf 'networks  : %s (no egress) + %s (broker %s:%s only)\n' "$NET_INTERNAL" "$NET_BROKER" "$BROKER" "$BROKER_PORT"
printf 'workspace : %s -> %s (%s)\n' "$REPO" "$WORKSPACE" "$eff_ws"
printf 'memory    : %s (%s)\n' "$MEMORY_DIR" "$eff_mem"
printf 'cache     : %s -> %s/target (%s)\n' "$TARGET_VOL" "$WORKSPACE" "$ROLE_TARGET"
printf 'command   : docker exec -w %s %s %s\n' "$WORKSPACE" "$NAME" "$CMDLINE"

# 6. The command. A TTY is used only when this shell has one, so the same invocation works from a script.
if [ "$CMDLINE" = bash ] || [ "$CMDLINE" = sh ]; then
  if [ -t 0 ] && [ "${#EXTRA[@]}" -eq 0 ]; then
    exec docker exec -it -w "$WORKSPACE" "$NAME" "$CMDLINE"
  fi
fi
if [ "${#EXTRA[@]}" -gt 0 ]; then
  exec docker exec -w "$WORKSPACE" "$NAME" "$CMDLINE" "${EXTRA[@]}"
fi
exec docker exec -w "$WORKSPACE" "$NAME" "$CMDLINE"
