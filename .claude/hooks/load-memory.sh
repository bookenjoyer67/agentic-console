#!/usr/bin/env bash
# Deterministic memory load at session start (v0.2.0).
#
# Why this exists: CLAUDE.md asks the agent to read SCOPE.md and MEMORY_INDEX.md at the start of every
# session, but that is advisory text and nothing executes it. Measured 2026-09-28: a fresh headless
# session given the Module 2.3 Step 4 prompt reported that it had read no memory files, because the
# instruction asks for tool calls the model has to choose to make. This hook makes the reads happen
# before the first prompt is processed, so the content is in context whether or not the model decides
# to look. Non-JSON stdout from a SessionStart hook is added to the context.
#
# v0.2.0 also inlines the ACTIVE ENTRIES the index lists. v0.1.0 injected only the index, and a fresh
# session could then name the entries but could not summarize a decision or quote a standard, because
# the index carries one line per entry. Making the index the load manifest keeps the two in step by
# construction: an entry becomes part of the startup context exactly when it is registered.
#
# Kept fast on purpose: every file is size-checked and skipped above MAX_BYTES, so startup cannot be
# slowed down by a large reference document.
set -uo pipefail

root="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
index="$root/.memory/project/MEMORY_INDEX.md"
MAX_BYTES=20000

echo "=== INJECTED PROJECT MEMORY (SessionStart hook v0.2.0) ==="
echo
echo "--- .memory/SCOPE.md ---"
if [ -r "$root/.memory/SCOPE.md" ]; then
  cat "$root/.memory/SCOPE.md"
else
  echo "(missing: .memory/SCOPE.md not found — do not trust any memory layer until it exists)"
fi
echo
echo "--- .memory/project/MEMORY_INDEX.md ---"
if [ ! -r "$index" ]; then
  echo "(missing: .memory/project/MEMORY_INDEX.md not found)"
  echo "--- end of injected memory ---"
  exit 0
fi
cat "$index"

# Inline the active entries named in the index. Paths are taken from the backticked path at the start
# of each "Active entries" bullet and resolved relative to .memory/project/.
grep -A 200 '^## Active entries' "$index" | while IFS= read -r line; do
  case "$line" in
    '## Archived entries'*) break ;;
  esac
  path="$(printf '%s' "$line" | sed -n 's/^- `\([^`]*\)`.*/\1/p')"
  [ -n "$path" ] || continue
  case "$path" in
    /*) file="$path" ;;
    *) file="$root/.memory/project/$path" ;;
  esac
  echo
  echo "--- $path (active entry) ---"
  if [ ! -r "$file" ]; then
    echo "(listed in the index but not readable: $file)"
    continue
  fi
  bytes="$(wc -c <"$file" | tr -d ' ')"
  if [ "$bytes" -gt "$MAX_BYTES" ]; then
    echo "(not inlined: $bytes bytes exceeds the ${MAX_BYTES}-byte startup budget — read it on demand)"
    continue
  fi
  cat "$file"
done

echo
echo "--- end of injected memory ---"
echo "If SCOPE.md names a different project than the one you are working in, halt and report the"
echo "mismatch before doing anything else. Entries whose review date has passed must be flagged to the"
echo "human before you act on them. Entries listed but not inlined have to be read on demand, and none"
echo "of this replaces reading the files you are about to change."
