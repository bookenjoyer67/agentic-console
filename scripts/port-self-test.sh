#!/usr/bin/env bash
# port-self-test.sh -- prove that this agentic gate is actually forkable.
#
#   bash scripts/port-self-test.sh            verify the wiring on this repository
#   bash scripts/port-self-test.sh --fork     as above, and fail on every seam that still
#                                             carries a value an ancestor kit had
#
# Why this exists: a fork that edits agentic.config.json but leaves a consumer hardcoded gets a
# system that half-works. The red team still passes, the suites still pass, and the gates silently
# test nothing. So this script does not trust the consumers. It points each consumer at a mutated
# copy of the config and requires that consumer's own --print-config output to CHANGE. A consumer
# whose output does not move is not reading the config, whatever its comments claim.
#
# --fork adds a third check with three axes, because a fork must be checked against its whole
# lineage and not only its parent:
#   coverage   every path port.seams names must differ from what EVERY ancestor had there
#   ancestry   no ancestor's value may survive as a literal in a consumer. A literal from two
#              generations back is absent from the parent's config, so a parent-only search
#              cannot find it -- which is how one fork's leftovers reach a third generation
#   escaping   a value is searched in both its JSON form and its source-escaped form, so a regex
#              recorded with single backslashes is still found against Python source
#
# Exit 0 when every check passes, 1 when any check fails, 2 on a usage error.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2
LOADER="$ROOT/scripts/agentic_config.py"
FORK=0
[ "${1:-}" = "--fork" ] && FORK=1
[ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] && { sed -n '2,20p' "$0"; exit 0; }

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
  echo "== 3. fork mode: seams and ancestry =="
  "$PY" - "$ROOT" <<'PYEOF'
import json, pathlib, re, sys
root = pathlib.Path(sys.argv[1])
cfg = json.loads((root / "agentic.config.json").read_text())
port = cfg.get("port", {}) or {}
seams = port.get("seams")
ancestors = port.get("ancestors")

if not isinstance(seams, list) or not isinstance(ancestors, list) or not ancestors:
    # An older kit's config carries ONE `*_defaults` map, holding the FIRST ancestor's values. The
    # name is read out of the config below rather than written here: this check sweeps itself, so a
    # reference project's name in its own source would be reported as a leak in its own source.
    # Two things were wrong with it and neither was visible from inside:
    #   * the baseline was a generation too far back, so a fork of a fork passed vacuously;
    #   * it listed only the keys its author remembered, so a value never re-pointed in an earlier
    #     fork was unlisted -- and therefore unfindable here, at any depth.
    print("  FAIL  this config carries no port.seams list and no port.ancestors chain")
    legacy = [k for k in port if k.endswith("_defaults")]
    if legacy:
        print(f"        an older kit's config has port.{legacy[0]} instead: one map, the FIRST")
    print("        ancestor, and only the keys someone remembered to list. Re-run the project's")
    print("        onboarding, or add both blocks by hand.")
    sys.exit(1)

parent = ancestors[-1].get("name", "the parent kit")


def dig(d, dotted):
    for part in dotted.split("."):
        if not isinstance(d, dict) or part not in d:
            return None
        d = d[part]
    return d


# A value's form in a source file is not always its form in the JSON: a regex carries single
# backslashes here and doubled ones in Python source, so a search for the raw string misses it.
# Both forms are searched. chr(92) keeps the escape out of this file's own quoting.
BS = chr(92)


def forms(value):
    return {value, value.replace(BS, BS * 2)}


def leaves(value):
    """The searchable scalars inside a seam value. A seam can hold a dict -- console.ports does
    -- and a scan that only looked at strings would let a port number walk past."""
    if isinstance(value, bool):
        return []
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [leaf for v in value.values() for leaf in leaves(v)]
    if isinstance(value, list):
        return [leaf for v in value for leaf in leaves(v)]
    if isinstance(value, (int, float)):
        return [str(value)]
    return []


# A value quoted ON PURPOSE -- a documented example, a style guide's illustration -- is excused
# only in WRITING, per file, with the reason. An excused match is printed as excused, so an
# exception can never become precedent by accident. Anything not listed here still fails.
excuses = {(q.get("value"), q.get("file")): q.get("why", "no reason given")
           for q in (port.get("quoted") or [])}
excused = []

stale = []

# 1. COVERAGE. Every path port.seams declares must differ from the value EVERY ancestor had for it.
#    The list lives in the config so that completeness is reviewable as part of the seam table,
#    rather than in a tuple inside this script that nobody reads.
kept = port.get("kept") or {}
kept_notes = []
# Values recorded in `review` are REPORTED, never swept and never failed: they are the ones where
# staying at an ancestor's value can be right (the CLI's own name, its flags, the fallback job list).
# Sweeping them is what would match "claude" in every consumer in the tree.
# Report each key ONCE, against the OLDEST generation it still matches: ancestors are oldest first, so
# naming the generation it reaches back to is the sharper and quieter statement than listing the same
# key under every generation whose value it happens to share.
review_seen = {}
for anc in ancestors:
    for path, recorded in (anc.get("review") or {}).items():
        live = dig(cfg, path)
        if live is not None and json.dumps(live, sort_keys=True) == json.dumps(recorded, sort_keys=True):
            review_seen.setdefault(path, anc.get("name"))
review_notes = [
    f"{path} still equals {name}'s value -- unchanged since that generation; reported for review, "
    f"not failed"
    for path, name in sorted(review_seen.items())
]
# every scalar of every seam that this project deliberately kept at an ancestor's value
kept_values = {leaf for path in kept if dig(cfg, path) is not None
               for leaf in leaves(dig(cfg, path))}
for path in seams:
    current = dig(cfg, path)
    for anc in ancestors:
        recorded = (anc.get("seams") or {}).get(path)
        if recorded is None or current is None:
            continue
        if current == recorded:
            if path in kept:
                kept_notes.append(f"{path} equals {anc.get('name')}'s value, kept deliberately: "
                                  f"{kept[path]}")
                continue
            stale.append(f"[coverage] {path} = {recorded!r} -- the config still carries "
                         f"{anc.get('name')}'s value")

# 2. ANCESTRY. No ancestor's value may survive as a literal anywhere it is used.
# The scanned set is DERIVED from the tree's wiring, not hand-listed. The hand list this replaces
# named seven files and missed `scripts/start-mcp-servers.sh`, which starts two of the servers and
# carried the reference project's image name -- an unported value in a wiring file the check could not
# see. A wiring file added tomorrow is scanned without anyone remembering to add it here.
WIRING_DIRS = ("scripts/", "mcp/", "eval/", ".claude/", ".github/workflows/")
WIRING_SUFFIXES = (".py", ".sh", ".json", ".yml", ".yaml")
# The governed role definitions are wiring, not prose: they name the project key, the clippy guard's
# marker and the gate names, which is exactly what a fork must re-point. They are markdown, so the
# suffix list has to know that for this one directory. It did not, which is how a whole port of the six
# role definitions landed with no check reading a byte of any of them -- a gap the porting run itself
# reported from inside, and could not close, because no role holds shell access.
WIRING_MARKDOWN_DIRS = (".claude/agents/",)
WIRING_ROOT_FILES = ("docker-entrypoint.sh", ".mcp.json")

# Declared, named in this check's output, and each one says why. An exclusion nobody can see is how a
# hand list fails: the missing file is invisible in both the config and the output.
SCAN_EXCLUDED = {
    "src/": "the console's Rust carries the REFERENCE project's values in its embedded fallback on "
            "purpose -- a repo with no agentic.config.json gets them and the console says so on screen "
            "-- so sweeping it today would report the fallback as a leak and bury the real ones. "
            "Re-pointing that fallback is its own change, and this exclusion is gated on it.",
    "docs/": "prose and the historical record name the reference project legitimately",
    ".memory/": "the run journals are the historical record",
    "target/": "build output",
    "vendor/": "third-party code",
    "node_modules/": "third-party code",
}


# Files this project keeps as records rather than wiring, with a reason each, declared in
# `port.records`. They belong in the config and not here for a concrete reason: this check sweeps
# scripts/, which includes this file, so writing an ancestor's name into it would be reported as a
# leak in it.
records = port.get("records") or {}


def is_wiring(rel):
    if rel.startswith("__pycache__/") or rel.endswith((".pyc", ".pyo")):
        return False
    if rel in records:
        return False
    if any(rel.startswith(d) for d in SCAN_EXCLUDED):
        return False
    if rel.startswith(WIRING_DIRS) and rel.endswith(WIRING_SUFFIXES):
        return True
    if rel.startswith(WIRING_MARKDOWN_DIRS) and rel.endswith(".md"):
        return True
    return "/" not in rel and (rel.startswith("Dockerfile") or rel in WIRING_ROOT_FILES)


consumers = sorted(
    p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()
    and is_wiring(p.relative_to(root).as_posix())
)
print(f"  info  scanned {len(consumers)} wiring file(s); not scanned, each with its reason here or in "
      f"port.records: {', '.join(sorted(SCAN_EXCLUDED))}"
      + (f" + {', '.join(sorted(records))}" if records else ""))
for rel, why in sorted(records.items()):
    print(f"        record, not wiring: {rel} -- {why}")
for rel in consumers:
    p = root / rel
    if not p.exists():
        continue
    text = p.read_text(errors="replace")
    for anc in ancestors:
        name = anc.get("name", "an ancestor")
        # A seam this project KEPT at an ancestor's value IS this project's value, so its text in a
        # wiring file is not evidence of an unported ancestor -- it is correct usage. Searching it
        # would produce only false positives. Every non-kept seam is still searched in full.
        raw = list((anc.get("seams") or {}).values()) + list(anc.get("literals") or [])
        for value in [leaf for item in raw for leaf in leaves(item)]:
            if len(value) < 4 or value in kept_values:
                continue
            starts = []
            for form in forms(value):
                starts += [m.start() for m in re.finditer(re.escape(form), text)]
            if not starts:
                continue
            why = excuses.get((value, rel))
            if why:
                excused.append(f"{rel}: {value!r} -- {why}")
                continue
            for start in sorted(starts):
                line_no = text[:start].count(chr(10)) + 1
                stale.append(f"[ancestry] {name} has {value!r} -- {rel}:{line_no} still "
                             f"contains the literal")

if review_notes:
    print(f"  note  {len(review_notes)} value(s) where an ancestor's value may be the right one:")
    for line in review_notes:
        print(f"        {line}")

if kept_notes:
    print(f"  note  {len(kept_notes)} seam(s) deliberately kept at an ancestor's value:")
    for line in kept_notes:
        print(f"        {line}")

if excused:
    print(f"  note  {len(excused)} deliberate quotation(s) excused by port.quoted:")
    for line in excused:
        print(f"        {line}")

if not stale:
    lineage = ", ".join(a.get("name", "?") for a in ancestors)
    print(f"  ok    no value from any ancestor survives ({lineage})")
    sys.exit(0)
print(f"  FAIL  {len(stale)} seam(s) still carry an ancestor's value:")
for line in stale:
    print(f"        {line}")
print()
print("        A [coverage] line means the config still holds an ancestor's value: set it in")
print("        agentic.config.json from the manifest, then re-point the matching fallback so the")
print("        two agree. An [ancestry] line means a literal from an EARLIER generation is still")
print("        in a consumer, which a parent-only search could never have found. Fix the file")
print(f"        named. The lineage checked here: {parent} and everything before it.")
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
