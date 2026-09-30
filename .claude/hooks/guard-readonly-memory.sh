#!/usr/bin/env bash
# Hard stop for the read-only memory layers.
#
# Why this exists: .memory/knowledge/ and .memory/reference/ are set to 555/444 so the agent cannot
# write to them, but the agent runs as root inside the sandbox container and root ignores file
# permissions. Measured 2026-09-28: `touch .memory/knowledge/root-test.txt` returned 0 as root and 1 as
# uid 1000. A guardrail that does not bind the process that actually writes is not a guardrail, so this
# hook enforces the rule inside Claude Code's own permission layer, where the uid is irrelevant.
#
# Deny any Write/Edit/MultiEdit whose target is under a read-only memory layer. Everything else is left
# to the normal permission flow (exit 0 with no output = no decision).
set -uo pipefail

input="$(cat)"

# Extract the target path without depending on jq (the sandbox image has no jq or python3). Both
# parameter names are read, because the two tool families disagree on it: the harness's
# Write/Edit/MultiEdit tools pass `file_path`, and `mcp__coursetools__file_write` passes `path`. A
# guard that reads only one name is a guard the other family walks past on a name difference alone.
# Pure parameter expansion, so no quote nesting and no external tool beyond grep.
path=""
for key in file_path path; do
  raw="$(printf '%s' "$input" \
    | grep -o "\"$key\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" \
    | head -1)"
  [ -n "$raw" ] || continue
  path="${raw#*:}"                                   # from the colon onward
  path="${path#"${path%%[![:space:]]*}"}"            # drop leading whitespace
  path="${path#\"}"                                  # drop the opening quote
  path="${path%\"}"                                  # drop the closing quote
  break
done

case "$path" in
  */.memory/knowledge/* | */.memory/reference/*)
    printf '%s\n' "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"deny\",\"permissionDecisionReason\":\"Blocked: this path is inside a read-only memory layer. .memory/knowledge/ and .memory/reference/ are human-maintained; the agent never writes to them. Record the change as a decision entry in .memory/project/decisions/ and register it in .memory/project/MEMORY_INDEX.md instead, or ask the human to make the edit.\"}}"
    exit 0
    ;;
esac

exit 0
