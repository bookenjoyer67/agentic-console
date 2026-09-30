#!/usr/bin/env bash
# port-self-test.sh -- prove that this agentic gate is actually forkable.
#
#   bash scripts/port-self-test.sh            verify the wiring on this repository
#   bash scripts/port-self-test.sh --fork     as above, and fail on every seam that still
#                                             carries a Komun default
#
# Why this exists: a fork that edits agentic.config.json but leaves a consumer hardcoded gets a
# system that half-works. The red team still passes, the suites still pass, and the gates silently
# test nothing. So this script does not trust the consumers. It points each consumer at a mutated
# copy of the config and requires that consumer's own --print-config output to CHANGE. A consumer
# whose output does not move is not reading the config, whatever its comments claim.
#
# Exit 0 when every check passes, 1 when any check fails, 2 on a usage error.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2
LOADER="$ROOT/scripts/agentic_config.py"
FORK=0
[ "${1:-}" = "--fork" ] && FORK=1
[ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] && { sed -n '2,12p' "$0"; exit 0; }

FAIL=0
pass() { printf '  ok    %s\n' "$1"; }
fail() { printf '  FAIL  %s\n' "$1"; FAIL=1; }

# Consumers: file:flag. Each must expose --print-config returning a JSON object of the config
# values it consumes. Add a row when a new consumer starts reading the config.
CONSUMERS="
mcp/gate/server.py
scripts/classify-change.py
scripts/validate_doc_conformance_deterministic.py
eval/test_policy.py
scripts/run-agent.sh
"
PY="python3"
command -v "$PY" >/dev/null 2>&1 || { echo "error: python3 is required by this self-test" >&2; exit 2; }

echo "== 1. the loader =="
if [ ! -f "$LOADER" ]; then
  fail "scripts/agentic_config.py is missing"
else
  src="$("$PY" "$LOADER" --source 2>/dev/null)"
  [ -n "$src" ] && pass "loader resolves its source: $src" || fail "loader printed no source"
  if "$PY" "$LOADER" --print-config 2>/dev/null | "$PY" -c \
      'import json,sys; d=json.load(sys.stdin); sys.exit(0 if d.get("schema_version") else 1)' ; then
    pass "loader emits valid JSON carrying schema_version"
  else
    fail "loader does not emit valid JSON with schema_version"
  fi
fi

echo "== 2. consumers read the config (mutation proof) =="
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
"$PY" - "$ROOT" "$TMP" <<'PYEOF'
import json, pathlib, sys
root, tmp = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
base = json.loads((root / "agentic.config.json").read_text())
mut = json.loads(json.dumps(base))
mut.setdefault("project", {})["name"] = "SELFTEST-MUTANT"
mut["containers"]["base_image"] = "selftest/base:SELFTEST-MUTANT"
mut["containers"]["tools_image"] = "selftest/tools:SELFTEST-MUTANT"
mut["containers"]["networks"]["internal"] = "selftest-internal"
mut["containers"]["networks"]["broker"] = "selftest-broker-net"
mut["containers"]["broker"]["name"] = "selftest-broker"
mut["containers"]["registry_volume"] = "selftest-registry"
mut["artifacts"]["project_key"] = "proj-SELFTEST-MUTANT"
mut["artifacts"]["style_rules"] = "selftest/DOC-STYLE.md"
mut["artifacts"]["storage_allow_list"] = "selftest/storage-allow-list.json"
mut["artifacts"]["retrieval_allow_list"] = "selftest/retrieval-allow-list.json"
mut["artifacts"]["policy_document"] = "selftest/governance-policy.md"
mut["toolchain"]["commands"]["test"]["argv"] = ["selftest-runner", "test"]
mut["toolchain"]["commands"]["fmt"]["argv"] = ["selftest-runner", "fmt"]
mut["classification"]["governed_globs"] = ["selftest/*"]
(tmp / "mutant.json").write_text(json.dumps(mut, indent=2, sort_keys=True))
PYEOF

if [ ! -f "$TMP/mutant.json" ]; then
  fail "could not build the mutated config"
else
  for c in $CONSUMERS; do
    [ -f "$c" ] || { fail "$c is missing"; continue; }
    case "$c" in
      *.sh) RUNNER="bash" ;;   # a shell consumer is not a python program
      *)    RUNNER="$PY" ;;
    esac
    base_out="$(AGENTIC_CONFIG="$ROOT/agentic.config.json" bash -c "cd '$ROOT' && $RUNNER '$c' --print-config" 2>/dev/null)"
    mut_out="$(AGENTIC_CONFIG="$TMP/mutant.json" bash -c "cd '$ROOT' && $RUNNER '$c' --print-config" 2>/dev/null)"
    if [ -z "$base_out" ]; then
      fail "$c does not implement --print-config"
    elif [ "$base_out" = "$mut_out" ]; then
      fail "$c ignores the config: its --print-config output did not change when the values did"
    else
      pass "$c follows the config"
    fi
  done
fi

if [ "$FORK" = "1" ]; then
  echo "== 3. fork mode: seams still carrying a Komun default =="
  "$PY" - "$ROOT" <<'PYEOF'
import json, pathlib, re, sys
root = pathlib.Path(sys.argv[1])
cfg = json.loads((root / "agentic.config.json").read_text())
defaults = cfg["port"]["komun_defaults"]

def dig(d, dotted):
    for part in dotted.split("."):
        if not isinstance(d, dict) or part not in d:
            return None
        d = d[part]
    return d

stale = []
for key, default in defaults.items():
    current = dig(cfg, key)
    if current == default:
        stale.append((key, default, "config value still equals the recorded default"))

consumers = [
    "mcp/gate/server.py", "scripts/run-agent.sh", "scripts/classify-change.py",
    "scripts/validate_doc_conformance_deterministic.py", "eval/test_policy.py",
    "scripts/agentic_config.py", ".github/workflows/ci.yml",
]
for rel in consumers:
    p = root / rel
    if not p.exists():
        continue
    text = p.read_text(errors="replace")
    for key, default in defaults.items():
        if not isinstance(default, str) or len(default) < 4:
            continue
        for m in re.finditer(re.escape(default), text):
            line_no = text[:m.start()].count("\n") + 1
            stale.append((key, default, f"{rel}:{line_no} still contains the literal"))

if not stale:
    print("  ok    no Komun default survives in the config or in a consumer")
    sys.exit(0)
print(f"  FAIL  {len(stale)} seam(s) still carry a Komun default:")
for key, default, where in stale:
    print(f"        {key} = {default!r} -- {where}")
print()
print("        Edit agentic.config.json for the value, then edit the fallback in the file named")
print("        above so the two agree. A fallback equal to the old default is the half-wired case")
print("        this check exists to catch.")
sys.exit(1)
PYEOF
  [ $? -ne 0 ] && FAIL=1
fi

echo
if [ "$FAIL" = "0" ]; then
  echo "port-self-test: PASS"
  exit 0
fi
echo "port-self-test: FAIL -- see the lines above"
echo "A consumer that ignores the config is the failure this kit exists to prevent: the suites would"
echo "still pass while the gates test the wrong thing. Do not silence this check."
exit 1
