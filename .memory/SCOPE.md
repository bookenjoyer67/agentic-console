# SCOPE.md — which project owns this memory directory

**Project:** `proj-console`
**Repository:** `agentic-console`
**What it is:** a terminal window onto an agentic quality gate — a Rust ratatui console at the
repository root, plus the gate, storage and retrieval MCP servers under `mcp/`, the drivers under
`scripts/`, the suites under `eval/`, the governed role definitions under `.claude/agents/`, and the
container runtime under `sandbox/`.
**The seam table:** `agentic.config.json` at the repository root. Every consumer reads it and falls
back to an embedded default of its own; a value that disagrees between the two is the half-wiring
`scripts/port-self-test.sh` exists to catch.

## What this memory belongs to

This directory is the memory of the `agentic-console` repository and of nothing else. Its entries
describe this repository's decisions, its current phase and its measured baselines. An entry here
that describes another project's code — a schema, a frontend, a service — is a leftover from the
reference project this fork was ported from, not a rule for this repository.

## The rule that follows from it

Read this file at the start of every session. **If the project named above does not match the
repository you are working in, halt and report the mismatch before reading any other memory file and
before any edit.** Memory mounted from another project looks identical to the right memory: same
directory layout, same file names, same entry numbering, and nothing in the content announces that it
belongs to a different codebase.

## Where the facts live

| layer | path | the agent's access |
|---|---|---|
| project memory | `.memory/project/` | read-write, through `MEMORY_INDEX.md` |
| knowledge | `.memory/knowledge/` | read-only; human-maintained |
| reference corpus | `.memory/reference/` | read-only; queried by keyword, never read whole |
| run evidence | `.memory/*.log`, `.memory/storage.db` | written by the MCP servers; never written by hand |

Secrets — credentials, tokens, API keys, personal data — are never written to any layer. Reference the
environment variable's name instead of its value. `scripts/hooks/pre-commit` blocks a commit that
matches a credential pattern, but the classification rule in `CLAUDE.md` does not depend on that hook
being installed.
