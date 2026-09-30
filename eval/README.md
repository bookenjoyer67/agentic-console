# Policy test suite

## Which rules does `eval/test_policy.py` enforce?

- Compare the seven policy entries with the artifacts that enforce them in both directions (`docs/governance-policy.md` `Hold no retrieval ceiling and no storage ceiling`).
- Read the policy, the routing map, both allow-lists, the seven definitions and the launcher (`scripts/run-agent.sh` `role_profile()`).

| Artifact | What the suite compares against the policy |
|---|---|
| `docs/governance-policy.md` | MCP grants and denials, skill scope, ceiling, autonomy, container mounts |
| `docs/routing-and-tool-grant-map.md`, `.json` | The grants, denials and ceilings the policy derives from |
| `mcp/storage/allow-list.json`, `mcp/retrieval/allow-list.json` | The per-role operations each server enforces |
| `.claude/agents/*.md` | `tools:`, `disallowedTools:` and `autonomy:` |
| `scripts/run-agent.sh` | The per-role mount profile in `role_profile()` |

## How do I run it?

Run it from the repository root, with `-v` so every check is named (`python3 -m pytest eval/test_policy.py -v` -> `75 passed`).

## What does a failure look like?

- Name the role, the artifact and both values (`ROLE \`planner\` disagrees with an artifact.`).
- Fix the side that is wrong, never both, and rerun (`docs/DOC-STYLE.md` `A rule is an imperative, and never hedges`).

## Which drift was injected to prove the suite bites?

- Withdraw a policy storage grant, drift one `autonomy:`, deny a granted tool, drop a mount, rename a cited near-miss, add a skill file, or delete a granted tool (`test_no_storage_overgrant` -> `FAILED`).
