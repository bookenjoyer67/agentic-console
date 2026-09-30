# Policy Reconciliation

## What does this file record?

Which conflicts existed between the routing map and the role definitions, and which side of each one was fixed?

The routing map is the decision of record for the gate (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`). Four conflicts stood between the map and the seven role definitions, and a fifth stood between the governance policy's container-permission prose and the launcher that enforces it. Each conflict was resolved by fixing one side per defect, and the table names that side and quotes the literal now in place.

| Conflict | Artifacts that disagreed | Side fixed | Literal value now in place | Where the fix landed |
| :--- | :--- | :--- | :--- | :--- |
| `C1` — the map's tester row granted an inert stub (`docs/iteration-log.md:305-306` `the course's deliberately inert stub`) | `docs/routing-and-tool-grant-map.md:18` `mcp__coursetools__test_runner` against `.claude/agents/tester.md:12-14` `- mcp__gate__run_gate` | The map; `.claude/agents/tester.md:12-14` `- mcp__gate__run_gate` is unchanged | `docs/routing-and-tool-grant-map.md:18` grants `mcp__gate__run_gate`, `mcp__gate__list_gates` and `mcp__gate__read_audit_log`, and denies `mcp__coursetools__test_runner` | `docs/routing-and-tool-grant-map.md:18`, `:70` and `:85`; the JSON grant at `docs/routing-and-tool-grant-map.json:35` `"mcp__gate__run_gate"` already agreed |
| `C2` — the reviewer held no gate read tool and could not confirm a recorded run | `docs/routing-and-tool-grant-map.json:46` `"mcp__gate__list_gates"` against `.claude/agents/reviewer.md:15` `- mcp__retrieval__retrieve` | The definition, which gains the two grants; `docs/routing-and-tool-grant-map.json:46` `"mcp__gate__list_gates"` is unchanged | `.claude/agents/reviewer.md:16-17` `- mcp__gate__list_gates` and `- mcp__gate__read_audit_log`; `.claude/agents/reviewer.md:25` `- mcp__gate__run_gate` stays denied | `.claude/agents/reviewer.md:16-17`, `:25` and `:89-91`; `docs/routing-and-tool-grant-map.md:19` completed to match its own JSON grant list |
| `C3` — the map gave the orchestrator a second path into the repository | `docs/routing-and-tool-grant-map.md:15` `mcp__coursetools__file_read` against `.claude/agents/orchestrator.md:11` `disallowedTools: Bash, mcp__coursetools__file_read` | The map; `.claude/agents/orchestrator.md:11` `disallowedTools: Bash, mcp__coursetools__file_read` is unchanged | `docs/routing-and-tool-grant-map.md:15` denies `mcp__coursetools__file_read` and `mcp__coursetools__file_write`; `docs/routing-and-tool-grant-map.json:11` `"orchestrator": []` | `docs/routing-and-tool-grant-map.md:15`; `docs/routing-and-tool-grant-map.json:11` |
| `C4` — the planner and tester autonomy values contradicted the map (`docs/routing-and-tool-grant-map.md:16` `Medium — it orders the work`) | `docs/routing-and-tool-grant-map.md:16` `Medium` against `.claude/agents/planner.md:23` `autonomy: medium` (was `low`); `docs/routing-and-tool-grant-map.md:18` `Low` against `.claude/agents/tester.md:25` `autonomy: low` (was `medium`) | The definitions, matching `docs/routing-and-tool-grant-map.md:16` `Medium — it orders the work` and `docs/routing-and-tool-grant-map.md:18` `Low — it runs the fixed gates and reports` | `.claude/agents/planner.md:23` `autonomy: medium` and `.claude/agents/tester.md:25` `autonomy: low` | `.claude/agents/planner.md:23` and `:33`; `.claude/agents/tester.md:25` and `:36` |
| `C5` — every role's entry opened with one shared read-write mount sentence. Four roles the policy grants a memory entry write had no writable path to `/workspace/.memory`. | The shared sentence, since rewritten, `the container mounts this repository read-write at /workspace` (`docs/governance-policy.md` `**Container permissions**`, all seven lines) against `scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`. The `Policy disagreements` section's `Deny the planner, tester, reviewer and researcher their single memory entry` line, since rewritten, against `docs/routing-and-tool-grant-map.json:17` `"mcp__storage__write_entry"`. | One side per defect: the policy prose for the mount sentence, because the launcher's argument is the enforcement. The launcher's mount set for the missing writable path, because the map is the decision of record for that grant. | `docs/governance-policy.md:110` ``the launcher mounts this repository read-only at `/workspace` for this role``. `scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")` mounts that path for the five roles holding `mcp__storage__write_entry`. | `docs/governance-policy.md` lines 67, 110, 155, 202, 248, 292 and 337; `scripts/run-agent.sh:95`, `:168` and its `role_profile` case; `scripts/README.md` matrix, proofs and `Policy disagreements` section. |

## Verification commands

Which command confirms each fixed literal, and what did it return?

- Match the tester's grant cell against the JSON grant list (`grep -c 'mcp__gate__read_audit_log`; `mcp__storage__read_entry' docs/routing-and-tool-grant-map.md` -> `1`).
- Show the map denying the inert stub (`grep -c 'mcp__coursetools__test_runner` is the deliberately inert stub' docs/routing-and-tool-grant-map.md` -> `1`).
- Show the orchestrator's JSON grant list empty (`grep -n '"orchestrator": \[\]' docs/routing-and-tool-grant-map.json` -> `11:    "orchestrator": [],`).
- Show the orchestrator's map denial reason (`grep -c 'the harness `Read` and `Write` tools cover orchestration documents' docs/routing-and-tool-grant-map.md` -> `1`).
- Show the reviewer's gate grants and its `run_gate` denial (`grep -n 'mcp__gate__list_gates\|mcp__gate__run_gate' .claude/agents/reviewer.md` -> `16:  - mcp__gate__list_gates`, `25:  - mcp__gate__run_gate`).
- Show both fixed autonomy values (`grep -n 'autonomy' .claude/agents/planner.md .claude/agents/tester.md` -> `.claude/agents/planner.md:23:autonomy: medium`, `.claude/agents/tester.md:25:autonomy: low`).
- Show the per-role mount statement in every policy entry (`grep -c 'the launcher mounts this repository' docs/governance-policy.md` -> `7`).
- Show the launcher's mount argument for a read-only workspace (`grep -n 'MOUNTS+=(-v "$REPO:/workspace:ro")' scripts/run-agent.sh` -> `165:  MOUNTS+=(-v "$REPO:/workspace:ro")`).
- Show the memory bind and the five roles carrying it (`grep -n 'ROLE_MEM=rw' scripts/run-agent.sh` -> `95:    planner)`, `96:    implementer)`, `97:    tester)`, `98:    reviewer)`, `100:    researcher)`).
- Show a read-only role refusing a repository write while its entry path accepts one (`./scripts/run-agent.sh reviewer bash -c 'touch /workspace/should-fail.txt'` -> `touch: cannot touch '/workspace/should-fail.txt': Read-only file system`; the same role's probe run -> `reviewer memory entry write+remove OK`).

## Citation maintenance

Which line citations moved because a fixed artifact changed length?

- Insert the reviewer's two grant lines and its `run_gate` denial (`.claude/agents/reviewer.md:16-17`, `:25`), which moves every later line by three.
- Point the policy's reviewer citations at the quoted literal (`docs/governance-policy.md:215` `the reviewer edits nothing it reviews` at `.claude/agents/reviewer.md:92`).
- Collapse the orchestrator's JSON grant list (`docs/routing-and-tool-grant-map.json:11` `"orchestrator": []`), which moves the retrieval ceiling block up by three lines.
- Point the policy's ceiling citations at the moved lines (`docs/governance-policy.md:55` cites `docs/routing-and-tool-grant-map.json:60` `"orchestrator": "none"`).
- Confirm every moved citation (`grep -c 'reviewer.md:92' docs/governance-policy.md` -> `2`; `grep -c 'routing-and-tool-grant-map.json:60' docs/governance-policy.md` -> `1`).
- Point the policy's seven container-permission lines at the launcher's own mount arguments (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`, `:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`), which is the one direction a later launcher edit moves them.
- Confirm the launcher's own citations of the policy lines, which the seven rewrites left in place (`grep -n 'lines 110, 202' scripts/run-agent.sh` -> `13:#   * /workspace is read-only for the five roles whose entry says they write no file there (lines 110, 202,`).
