# Routing and Tool Grant Map

Which roles run the pre-merge quality gate for `proj-console`, and what does each one hold?

Project: `proj-console`

Update an agent definition to match this map whenever the two disagree. This map is the design decision of record for the gate.

## Role table

Which role receives what from the Orchestrator, produces what, and holds which tools?

| Role | Receives from Orchestrator | Produces | Tools granted | Tools denied (reason) | Context isolation reason | Autonomy |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `orchestrator` | The change request, every subagent return, and both human checkpoint decisions | Task briefs to each subagent, and the assembled run summary | Subagent invocation, a harness capability rather than an MCP tool | `mcp__coursetools__file_read`, `mcp__coursetools__file_write`, `mcp__coursetools__codebase_search`, `mcp__coursetools__shell`, `mcp__coursetools__test_runner`, `mcp__coursetools__task_tracker`, `mcp__coursetools__web_search`, and every storage and retrieval operation — the harness `Read` and `Write` tools cover orchestration documents, so both MCP file tools are denied and no role reaches the repository through a second path; no code-touching or execution tool, because its context goes to sequencing | It holds orchestration documents and short returns, so one context window spans the whole run | High — it sequences the roles and decides loop, skip, halt and escalate; a human owns both checkpoints |
| `planner` | The change request, the repository path, and the acceptance criteria | An ordered plan with the file list it touches | `mcp__coursetools__file_read`, `mcp__coursetools__codebase_search`; `mcp__storage__read_entry`, `mcp__storage__list_entries`, `mcp__storage__write_entry`; `mcp__retrieval__retrieve` at ceiling `internal` | `mcp__coursetools__file_write`, `mcp__coursetools__shell`, `mcp__coursetools__test_runner`, `mcp__coursetools__task_tracker`, `mcp__coursetools__web_search`, `mcp__storage__update_entry`, `mcp__storage__delete_entry` — a plan is a new entry, and a planning role edits no code and no existing record | It reads the repository and its own plan entries, and it never sees a test runner, a gate log or a diff | Medium — it orders the work, and a human approves the plan at Checkpoint 1 |
| `implementer` | The approved plan, the file list, and the `AGENTS.md` constraints | Modified files, with change notes | `mcp__coursetools__file_read`, `mcp__coursetools__file_write`, `mcp__coursetools__codebase_search`; `mcp__storage__read_entry`, `mcp__storage__list_entries`, `mcp__storage__write_entry`, `mcp__storage__update_entry`; `mcp__retrieval__retrieve` at ceiling `internal`; `mcp__gate__run_fix`, which runs a write-mode command by name and holds one such name, `fmt-fix` | `mcp__coursetools__shell`, `mcp__coursetools__test_runner`, `mcp__coursetools__task_tracker`, `mcp__coursetools__web_search`, `mcp__storage__delete_entry`, `mcp__gate__run_gate` — the Tester holds the only runner and the only check surface, and record removal is no role's grant | It receives the approved plan instead of the whole request thread, so the diff before it stays scoped to that plan | Medium — it writes code inside the approved plan and file list, and returns each change for independent test |
| `tester` | The modified files and the acceptance criteria | Gate results, pass or fail, each with its command and output | `mcp__coursetools__file_read`, `mcp__gate__run_gate`, `mcp__gate__list_gates`, `mcp__gate__read_audit_log`; `mcp__storage__read_entry`, `mcp__storage__list_entries`, `mcp__storage__write_entry` | `mcp__coursetools__file_write`, `mcp__coursetools__shell`, `mcp__coursetools__test_runner`, `mcp__coursetools__task_tracker`, `mcp__coursetools__web_search`, `mcp__retrieval__retrieve`, `mcp__storage__update_entry`, `mcp__storage__delete_entry`, `mcp__gate__run_fix` — a result is a new record, a verifying role repairs nothing it finds and therefore holds no write-mode command, the gate server is the only execution path, and `mcp__coursetools__test_runner` is the deliberately inert stub (`docs/iteration-log.md:305-306` `the course's deliberately inert stub`) | It works from the supplied acceptance criteria and the gate output, not from the reference corpus | Low — it runs the fixed gates and reports, and it revises no file and no verdict |
| `reviewer` | The modified files and the repository's review standards | A review report, each finding tied to a file and a rule | `mcp__coursetools__file_read`, `mcp__coursetools__codebase_search`; `mcp__storage__read_entry`, `mcp__storage__list_entries`, `mcp__storage__write_entry`; `mcp__retrieval__retrieve` at ceiling `internal`; `mcp__gate__list_gates`, `mcp__gate__read_audit_log` (the prose and citation conformance step this role ran by hand now runs as a script and holds no MCP access; see `## Converted steps` below) | `mcp__gate__run_gate`, `mcp__coursetools__file_write`, `mcp__coursetools__shell`, `mcp__coursetools__test_runner`, `mcp__coursetools__task_tracker`, `mcp__coursetools__web_search`, `mcp__storage__update_entry`, `mcp__storage__delete_entry` — a reviewer that held a writer would grade its own work, and a re-run would overwrite the recorded gate evidence | It reads the diff and the standards, and it writes only its own review entry | Low — it reports findings, the Orchestrator routes them, and the Implementer acts |
| `project-manager` | The assembled run summary | A ticket update confirmation | `mcp__coursetools__task_tracker`; `mcp__storage__read_entry`, `mcp__storage__list_entries` | `mcp__coursetools__file_read`, `mcp__coursetools__file_write`, `mcp__coursetools__codebase_search`, `mcp__coursetools__shell`, `mcp__coursetools__test_runner`, `mcp__coursetools__web_search`, `mcp__retrieval__retrieve`, `mcp__storage__write_entry`, `mcp__storage__update_entry`, `mcp__storage__delete_entry` — it owns ticket state, not project memory or code | It receives a summary rather than a diff, a gate log or a corpus, so ticket state alone fills its context | Low — it updates the ticket and reports, and it owns no project memory |
| `researcher` (stretch, optional) | One external-documentation question, forwarded when another role is blocked | A findings document with citations | `mcp__coursetools__web_search`, `mcp__storage__write_entry` | `mcp__coursetools__file_read`, `mcp__coursetools__file_write`, `mcp__coursetools__codebase_search`, `mcp__coursetools__shell`, `mcp__coursetools__task_tracker`, `mcp__coursetools__test_runner`, `mcp__retrieval__retrieve`, `mcp__storage__read_entry`, `mcp__storage__list_entries`, `mcp__storage__update_entry`, `mcp__storage__delete_entry` — isolating `web_search` here is the reason the role exists, so nothing rides along | It answers one question and returns, so open-web text never enters a coding role's context | Low — it answers on request only, and it decides nothing about the change |

## Storage operation grants

Which storage operations does each role hold, and why is each denial made?

All storage operations live on the `storage` MCP server and are written `mcp__storage__<operation>`.

| Role | `read_entry` | `list_entries` | `write_entry` | `update_entry` | `delete_entry` |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `orchestrator` | denied | denied | denied | denied | denied |
| `planner` | granted | granted | granted | denied | denied |
| `implementer` | granted | granted | granted | granted | denied |
| `tester` | granted | granted | granted | denied | denied |
| `reviewer` | granted | granted | granted | denied | denied |
| `project-manager` | granted | granted | denied | denied | denied |
| `researcher` | denied | denied | granted | denied | denied |

Denial reasons:

- Deny the Orchestrator every storage operation, because it reads and writes orchestration documents instead of project memory.
- Deny `update_entry` and `delete_entry` to the Planner, because a plan is a new entry rather than a revision or a removal.
- Deny `delete_entry` to the Implementer, because it revises its own notes and removes no record.
- Deny `update_entry` and `delete_entry` to the Tester, because it records a result and alters no earlier record.
- Deny `update_entry` and `delete_entry` to the Reviewer, because it stays read-only toward others' work and writes only its own review.
- Deny `write_entry`, `update_entry` and `delete_entry` to the Project Manager, because it owns ticket state rather than persistent project memory.
- Deny `read_entry` and `list_entries` to the Researcher, because it records findings and reads nothing back from project memory.

## Retrieval operation grants

Which retrieval operation does each role hold, and what ceiling caps it?

The retrieval server exposes one operation, `mcp__retrieval__retrieve`. A grant has two parts: access to the operation, and the classification ceiling pinned to the role.

| Role | Retrieval operation granted | Classification ceiling | Retrieval denied (reason) |
| :--- | :--- | :--- | :--- |
| `orchestrator` | none | none | `mcp__retrieval__retrieve` — it sequences roles and reads their returns, and it queries no corpus |
| `planner` | `mcp__retrieval__retrieve` | `internal` | none |
| `implementer` | `mcp__retrieval__retrieve` | `internal` | none |
| `tester` | none | none | `mcp__retrieval__retrieve` — it works from the supplied acceptance criteria and searches no corpus |
| `reviewer` | `mcp__retrieval__retrieve` | `internal` | none |
| `project-manager` | none | none | `mcp__retrieval__retrieve` — it owns the ticket tool and performs no reference lookup |
| `researcher` | none | none | `mcp__retrieval__retrieve` — its channel is the open web, and the internal corpus would only widen its context |

## Cross-role check

Which grants repeat across roles, and does any repeat breach least privilege?

- Hold `mcp__coursetools__web_search` on the Researcher alone, which is the whole reason the stretch role exists.
- Hold `mcp__gate__run_gate` on the Tester alone, deny `mcp__coursetools__test_runner` to every role, and keep the gate server the one path that executes a command. Read that grant as reaching all seven check-mode names, `webcheck` and `webtest` included (`agentic.config.json:93` `"argv": ["npm", "--prefix", "web", "run", "check"],`). Authorisation is held per tool, not per gate name. The server's only call-time test is a membership check (`mcp/gate/server.py:120` `if gate not in GATES:`). It validates no role, so adding a name to the config extends the surface of every holder of that tool.
- Hold `mcp__coursetools__task_tracker` on the Project Manager alone, because ticket state has one owner.
- Hold `mcp__storage__update_entry` on the Implementer alone, which is the only role that revises a record it wrote.
- Grant `mcp__storage__delete_entry` to no role, and keep record removal outside the gate.
- Grant `mcp__retrieval__retrieve` to three roles, the Planner, the Implementer and the Reviewer, which is safe because the operation is read-only, project-scoped and citation-bearing.
- Cap every retrieval grant at `internal`, and hold no role above that ceiling (`docs/memory-architecture.md:149` `**Confidential** — Sensitive business data. Do not store in agent memory.`; `docs/memory-architecture.md:151` `**Secret** — Credentials, tokens, API keys, PII. Must never appear in any memory file.`).
- Hold `mcp__gate__run_fix` on the Implementer alone, and keep the check surface on the Tester, so the role that repairs a file never grades the repair (`.claude/agents/tester.md:25` `- mcp__gate__run_fix`).

## Alternatives considered

Which grant designs were weighed and ruled out?

**1. Grant `mcp__storage__delete_entry` to the Implementer for scratch cleanup.** Ruled out, because cleanup does not justify handing a coding role a destructive capability. Introduce a dedicated maintenance role under explicit review if cleanup ever becomes necessary.

**2. Grant `mcp__retrieval__retrieve` to the Tester.** Ruled out, because the Tester receives the acceptance criteria and the gate commands in its brief, and an independent corpus search adds a capability the role does not need.

**3. Let the Orchestrator hold `mcp__gate__run_gate` and run the gates itself.** Ruled out, because an orchestrator that runs a gate and then grades that same run evaluates its own execution, which removes the independent check the Tester provides.

**4. Grant `mcp__coursetools__web_search` to every role.** Ruled out, because the open web returns unbounded text, and one dedicated role keeps that text out of every coding context.

**5. Raise the Reviewer ceiling to `confidential`.** Ruled out, because no review task in this gate reads confidential data, and the ceiling stays at `internal` (`docs/memory-architecture.md:149` `Do not store in agent memory.`).

## Converted steps

Which recorded step no longer holds an MCP grant, and what runs it now?

- Run the prose and citation conformance check as `scripts/validate_doc_conformance_deterministic.py`, which holds no MCP operation and makes no model call.
- Stop routing that step to a subagent, because the reviewer ran it by hand from the reads its own row grants (`docs/routing-and-tool-grant-map.md:19` `mcp__coursetools__codebase_search`) and the step now holds no tool of its own.
- Read the decision of record in `docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`, which fixes the contract the script holds.
- Keep the marker machine-readable, so a check can read it: `docs/routing-and-tool-grant-map.json:72` `"mcp_access": []`.
- Run that step inside the workflow through the `conformance` gate, which the server runs by name (`agentic.config.json:61` `"argv": ["python3", "scripts/run-conformance-gate.py"],`).

