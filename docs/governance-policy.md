# Agent Governance Policy

Version: v1.0.0
Last updated: 2026-09-28
Reviewed by: pending — no human has signed this v1.0.0 draft, and the Module 4.1 checkpoint owns the approval.

## Policy basis

What does this policy derive from, and where does each authority sit?

- Derive every grant from the routing-and-tool-grant map (`docs/routing-and-tool-grant-map.md:7` `This map is the design decision of record for the gate.`).
- Read the machine-readable grant list as the same map's operative form (`docs/routing-and-tool-grant-map.json:10` `"grants"`).
- Derive every denial from the near-miss patterns observed in calibration (`docs/calibration-log.md` section `Near-miss patterns for Module 4 governance`, patterns `NM-1` to `NM-9`).
- Apply least-privilege defaults to all seven roles (`docs/routing-and-tool-grant-map.md:73` `Grant mcp__storage__delete_entry to no role`).
- Use this repo's paths rather than the lesson's (`sandbox/README-m3.md:54` `the memory directory stays inside the repo at /workspace/.memory`).
- Treat `/memory` as absent in this container (`docker exec agent-rev-m3 ls -d /memory` -> `No such file or directory`).
- Bound every ceiling by the memory classification levels (`docs/memory-architecture.md:149` `Do not store in agent memory.`).

## Least-privilege default

How does a role acquire access, and what does it hold before any grant?

Start every role at no access: no MCP operation, no skill, no retrieval ceiling, no autonomy.
Record every grant beside the artifact that states it, and give every denial a one-line reason.
Adopt the narrower authority where a role definition and the map disagree (`docs/routing-and-tool-grant-map.md:7` `Update an agent definition to match this map whenever the two disagree.`).

## Role 1 — `orchestrator`

What does the sequencing role hold, and what does it decide?

### MCP server and operation access

Which MCP server operations may the orchestrator call?

- Hold no MCP operation at all, and keep the harness tools `Task`, `Read`, `Write` and `Edit` (`.claude/agents/orchestrator.md:10` `tools: Task, Read, Write, Edit`).
- Deny `mcp__coursetools__file_read` and `mcp__coursetools__file_write` (`.claude/agents/orchestrator.md:11` `disallowedTools: Bash, mcp__coursetools__file_read`).
- Deny every storage operation (`docs/routing-and-tool-grant-map.md:41` `Deny the Orchestrator every storage operation`) — this denial answers `NM-3`, where the orchestrator called a denied storage operation mid-run.
- Deny `mcp__retrieval__retrieve` (`docs/routing-and-tool-grant-map.md:57` `it queries no corpus`).
- Deny `Bash` and `mcp__coursetools__test_runner` (`.claude/agents/orchestrator.md:38` `Denied; gate commands belong to the tester`).
- Deny `mcp__gate__run_gate`, `mcp__gate__list_gates` and `mcp__gate__read_audit_log` (`docs/routing-and-tool-grant-map.md:85` `evaluates its own execution`) — this denial answers `NM-4`, where a role graded a record it wrote.
- Resolve the orchestrator file-tool conflict by fixing the map: `docs/routing-and-tool-grant-map.md:15` now denies `mcp__coursetools__file_read` and `mcp__coursetools__file_write`, and `docs/routing-and-tool-grant-map.json:11` reads `"orchestrator": []`.

### Skill activation scope

Which skill may the orchestrator activate, and when?

- Permit `summarize-session` at a phase boundary (`.claude/skills/summarize-session/SKILL.md:6` `phase is ending and new rules or new material are about to arrive`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Carry the summary's unresolved questions into the run summary (`.claude/skills/summarize-session/SKILL.md:38` `Anything unresolved: claims that could not be verified`).

### Data classification ceiling

What classification may this role touch?

- Hold no retrieval ceiling and no storage ceiling (`docs/routing-and-tool-grant-map.json:60` `"orchestrator": "none"`).
- Deny `confidential` and `secret` content to the role (`.claude/agents/orchestrator.md:51` `only the roles that produce a result record it in storage`) — this denial answers `NM-5`, where a credential stood inside memory reach.
- Keep the run summary at `internal` or below (`docs/memory-architecture.md:147` `Safe within the team but not for public repos.`).

### Autonomy level

How far does this role act before a human decides?

- Hold `high` autonomy (`.claude/agents/orchestrator.md:12` `autonomy: high`).
- Decide loop, skip, halt and escalate, and own neither checkpoint (`docs/routing-and-tool-grant-map.md:15` `a human owns both checkpoints`).
- Escalate to the human after a role fails twice (`.claude/agents/orchestrator.md:63` `when a role fails twice`).

**Container permissions** — the launcher mounts this repository read-write at `/workspace` for this role (`scripts/run-agent.sh:163` `MOUNTS+=(-v "$REPO:/workspace")`), and mounts `/workspace/.memory` read-only (`scripts/run-agent.sh:169` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory:ro")`). This role reads and writes orchestration documents under `/workspace`, and it holds no memory write grant (`docs/routing-and-tool-grant-map.json:11` `"orchestrator": []`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Role 2 — `planner`

What does the planning role hold, and what may it not read or write?

### MCP server and operation access

Which MCP server operations may the planner call?

- Grant `mcp__coursetools__file_read` and `mcp__coursetools__codebase_search` (`.claude/agents/planner.md:10-11` `- mcp__coursetools__codebase_search`) — this grant answers `NM-2`, where a planner that could not read its inputs planned from the brief alone.
- Grant `mcp__storage__read_entry`, `mcp__storage__list_entries` and `mcp__storage__write_entry` (`.claude/agents/planner.md:12-14` `- mcp__storage__write_entry`).
- Grant `mcp__retrieval__retrieve` at the `internal` ceiling (`.claude/agents/planner.md:15` `- mcp__retrieval__retrieve`).
- Deny `mcp__coursetools__file_write` (`.claude/agents/planner.md:75` `writing it is the implementer's role`) — a planning role writes no code.
- Deny `mcp__coursetools__shell` and `mcp__coursetools__test_runner` (`.claude/agents/planner.md:76-77` `no command execution while planning`) — no plan step rests on an unrun command.
- Deny `mcp__coursetools__task_tracker` (`.claude/agents/planner.md:78` `ticket state belongs to the project manager`).
- Deny `mcp__coursetools__web_search` (`docs/routing-and-tool-grant-map.md:69` `Hold mcp__coursetools__web_search on the Researcher alone`) — one role keeps open-web text out of every coding context.
- Deny `mcp__storage__update_entry` and `mcp__storage__delete_entry` (`docs/routing-and-tool-grant-map.md:42` `a plan is a new entry rather than a revision or a removal`).

### Skill activation scope

Which skill may the planner activate, and when?

- Permit `summarize-session` at the plan-approval boundary (`.claude/skills/summarize-session/SKILL.md:7` `when the user asks for a session summary`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Name the unread artifact and halt when a rule cannot be settled from the repository (`.claude/agents/planner.md:62` `Halt and return to the orchestrator when the request cannot be planned`).

### Data classification ceiling

What classification may the planner read and write?

- Cap retrieval at `internal` (`.claude/agents/planner.md:74` `the pinned internal ceiling`).
- Write the plan entry at `classification: "internal"` (`.claude/agents/planner.md:59` `classification: "internal"`).
- Deny `confidential` and `secret` content to the role (`.claude/agents/planner.md:73` `classification public or internal only`) — this denial answers `NM-5`.

### Autonomy level

How far does the planner act before a human decides?

- Hold `medium` autonomy (`.claude/agents/planner.md:23` `autonomy: medium`).
- Wait for the human plan-approval checkpoint before any implementer starts (`.claude/agents/planner.md:33` `the plan is a proposal until the human clears the plan-approval checkpoint`).
- Resolve the planner autonomy conflict by fixing the definition: `.claude/agents/planner.md:23` now reads `autonomy: medium`, matching `docs/routing-and-tool-grant-map.md:16` `Medium — it orders the work`.

**Container permissions** — the launcher mounts this repository read-only at `/workspace` for this role (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`), and mounts `/workspace/.memory` read-write so the granted entry write lands (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`). This role reads every file in `/workspace`, writes no file there, and writes one plan entry into `/workspace/.memory` (`.claude/agents/planner.md:75` `writing it is the implementer's role`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Role 3 — `implementer`

What does the writing role hold, and what may it never run or delete?

### MCP server and operation access

Which MCP server operations may the implementer call?

- Grant `mcp__coursetools__file_read`, `mcp__coursetools__file_write` and `mcp__coursetools__codebase_search` (`.claude/agents/implementer.md:10-12` `- mcp__coursetools__file_write`).
- Grant `mcp__storage__read_entry`, `mcp__storage__list_entries`, `mcp__storage__write_entry` and `mcp__storage__update_entry` (`.claude/agents/implementer.md:13-16` `- mcp__storage__update_entry`).
- Grant `mcp__retrieval__retrieve` at the `internal` ceiling (`.claude/agents/implementer.md:17` `- mcp__retrieval__retrieve`).
- Grant `mcp__gate__run_fix` on this role alone (`.claude/agents/implementer.md:18` `- mcp__gate__run_fix`) — it runs a write-mode command by name, and the vocabulary holds one such name, `fmt-fix`.
- Scope `mcp__storage__update_entry` to the entries this role wrote (`docs/routing-and-tool-grant-map.md:72` `the only role that revises a record it wrote`).
- Deny `mcp__coursetools__shell` (`.claude/agents/implementer.md:83` `no build or migration runs from the writing role`) — the writing role executes nothing.
- Deny `mcp__coursetools__test_runner` (`.claude/agents/implementer.md:84` `a self-run gate is not independent evidence`) — this denial answers `NM-1`, where a gate that never ran held no evidence.
- Deny `mcp__coursetools__task_tracker` (`.claude/agents/implementer.md:85` `ticket state belongs to the project manager`).
- Deny `mcp__storage__delete_entry` (`.claude/agents/implementer.md:86` `records are never removed`) — a superseded decision stays readable.
- Deny `mcp__coursetools__web_search` (`docs/routing-and-tool-grant-map.md:69` `Hold mcp__coursetools__web_search on the Researcher alone`) — the writing role holds no network tool.
- Deny `mcp__gate__run_gate` (`.claude/agents/implementer.md:24` `- mcp__gate__run_gate`) — the check surface stays the tester's, so the role that writes a fix never runs the gate that judges it.

### Skill activation scope

Which skill may the implementer activate, and when?

- Permit `summarize-session` at a repair-cycle boundary (`.claude/skills/summarize-session/SKILL.md:6-7` `when the context window is filling`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Return an unnamed file to the orchestrator rather than writing it (`.claude/agents/implementer.md:44-45` `A file the change needs that the plan does not name is a scope change`).

### Data classification ceiling

What classification may the implementer read and write?

- Cap retrieval at `internal` (`.claude/agents/implementer.md:82` `the pinned internal ceiling`).
- Write entries at `public` or `internal` (`.claude/agents/implementer.md:63` `The storage server accepts public and internal writes only`).
- Deny `confidential` and `secret` content to the role (`.claude/agents/implementer.md:80` `the server rejects confidential and secret`) — this denial answers `NM-5`.
- Deny any credential or message plaintext in a written file (`docs/memory-architecture.md:151` `Must never appear in any memory file.`).

### Autonomy level

How far does the implementer act before a human decides?

- Hold `medium` autonomy (`.claude/agents/implementer.md:23` `autonomy: medium`).
- Act on the approved plan without a further human checkpoint (`.claude/agents/implementer.md:33` `acts on an approved plan without a further human checkpoint`).
- Return each change for independent test instead of clearing it (`.claude/agents/implementer.md:69` `state plainly that the implementer ran none of them`).

**Container permissions** — the launcher mounts this repository read-write at `/workspace` for this role (`scripts/run-agent.sh:163` `MOUNTS+=(-v "$REPO:/workspace")`), and mounts `/workspace/.memory` read-write for its entry writes (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`). This role writes the files the plan names under `/workspace`, and it writes and revises its own entries in `/workspace/.memory` (`.claude/agents/implementer.md:86` `records are never removed`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Role 4 — `tester`

What does the verifying role hold, and what may it never repair?

### MCP server and operation access

Which MCP server operations may the tester call?

- Grant `mcp__coursetools__file_read` (`.claude/agents/tester.md:11` `- mcp__coursetools__file_read`).
- Grant `mcp__gate__run_gate`, `mcp__gate__list_gates` and `mcp__gate__read_audit_log` (`.claude/agents/tester.md:12-14` `- mcp__gate__run_gate`) — this grant answers `NM-1`, where an inert tool blocked every gate.
- Grant `mcp__storage__read_entry`, `mcp__storage__list_entries` and `mcp__storage__write_entry` (`.claude/agents/tester.md:15-17` `- mcp__storage__write_entry`).
- Resolve the tester-runner conflict by fixing the map: `docs/routing-and-tool-grant-map.md:18` now grants `mcp__gate__run_gate` and denies `mcp__coursetools__test_runner`. The stub is `the course's deliberately inert stub` (`docs/iteration-log.md:305-306`), and this grant answers `NM-1`.
- Deny every command string and extra argument through the gate server (`mcp/gate/server.py:123` `it accepts no command string, no extra arguments`) — a refused call runs nothing. The vocabulary holds eight commands: the seven check-mode names `test`, `clippy`, `fmt`, `policy`, `conformance`, `webcheck` and `webtest`, and one write-mode name, `fmt-fix`.
- Deny `mcp__coursetools__file_write` (`docs/routing-and-tool-grant-map.md:18` `a verifying role repairs nothing it finds`).
- Deny `mcp__coursetools__shell` and `mcp__coursetools__task_tracker` (`docs/routing-and-tool-grant-map.md:18` `a result is a new record`).
- Deny `mcp__retrieval__retrieve` (`docs/routing-and-tool-grant-map.md:60` `it works from the supplied acceptance criteria`).
- Deny `mcp__storage__update_entry` and `mcp__storage__delete_entry` (`.claude/agents/tester.md:88` `a gate result is a new record`).
- Deny `mcp__coursetools__web_search` (`docs/routing-and-tool-grant-map.md:69` `Hold mcp__coursetools__web_search on the Researcher alone`) — the tester reads no open-web text.
- Deny `mcp__gate__run_fix` (`.claude/agents/tester.md:25` `- mcp__gate__run_fix`) — the verifying role repairs nothing it finds, so the write-mode command stays with the implementer.

### Skill activation scope

Which skill may the tester activate, and when?

- Permit `summarize-session` after a gate run (`.claude/skills/summarize-session/SKILL.md:6-7` `when the context window is filling`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Name the cache-hit prerequisite as a blocker rather than touching a source file (`.claude/agents/tester.md:53` `a silent second run is a cache hit, not a clean lint`).

### Data classification ceiling

What classification may the tester record?

- Hold no retrieval ceiling (`docs/routing-and-tool-grant-map.json:63` `"tester": "none"`).
- Write the result entry at `classification: "internal"` (`.claude/agents/tester.md:61` `classification: "internal"`).
- Deny `confidential` and `secret` content to the role (`.claude/agents/tester.md:61-62` `accepts public and internal writes only`) — this denial answers `NM-5`.
- Print a failure's output in full, with no filtering (`.claude/agents/tester.md:58` `Print each failure's output in full, with no filtering`).

### Autonomy level

How far does the tester act before a human decides?

- Hold `low` autonomy (`.claude/agents/tester.md:25` `autonomy: low`).
- Decide when a gate has passed against the recorded baseline (`.claude/agents/tester.md:36` `decides when a gate has passed against the baseline`).
- Report a gate as INCONCLUSIVE when its prerequisite is unmet (`.claude/agents/tester.md:50-51` `report the gate as inconclusive when the prerequisite is unmet`).
- Resolve the tester autonomy conflict by fixing the definition: `.claude/agents/tester.md:25` now reads `autonomy: low`, matching `docs/routing-and-tool-grant-map.md:18` `Low — it runs the fixed gates and reports`.

**Container permissions** — the launcher mounts this repository read-only at `/workspace` for this role (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`), and mounts `/workspace/.memory` read-write so the granted entry write lands (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`). This role reads `/workspace` and runs the gates against it, writes no file there, and writes one test-result entry into `/workspace/.memory` (`.claude/agents/tester.md:63` `Never create, edit, move or delete a repository file`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Role 5 — `reviewer`

What does the reviewing role hold, and what may it never fix?

### MCP server and operation access

Which MCP server operations may the reviewer call?

- Grant `mcp__coursetools__file_read` and `mcp__coursetools__codebase_search` (`.claude/agents/reviewer.md:10-11` `- mcp__coursetools__codebase_search`).
- Grant `mcp__storage__read_entry`, `mcp__storage__list_entries` and `mcp__storage__write_entry` (`.claude/agents/reviewer.md:12-14` `- mcp__storage__write_entry`).
- Grant `mcp__retrieval__retrieve` at the `internal` ceiling (`.claude/agents/reviewer.md:15` `- mcp__retrieval__retrieve`) — the verdict half keeps this read, and the converted conformance step holds no MCP grant (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`).
- Deny `mcp__coursetools__file_write` (`.claude/agents/reviewer.md:92` `the reviewer edits nothing it reviews`) — this denial answers `NM-4`, where an approval shared the author of the verdict it approved.
- Deny `mcp__coursetools__shell` and `mcp__coursetools__test_runner` (`.claude/agents/reviewer.md:93-94` `a re-run by the reviewer would overwrite the recorded evidence`) — this denial answers `NM-7`, where an unrun gate was closed unevidenced.
- Resolve the reviewer gate-tool conflict by fixing the definition: `.claude/agents/reviewer.md:16-17` now grants `mcp__gate__list_gates` and `mcp__gate__read_audit_log`, which `docs/routing-and-tool-grant-map.json:46` `"mcp__gate__list_gates"` already granted, and `.claude/agents/reviewer.md:25` keeps `mcp__gate__run_gate` denied.
- Deny `mcp__coursetools__task_tracker` (`.claude/agents/reviewer.md:95` `ticket state belongs to the project manager`).
- Deny `mcp__storage__update_entry` and `mcp__storage__delete_entry` (`.claude/agents/reviewer.md:96-97` `a review is a new record`).
- Deny `mcp__coursetools__web_search` (`docs/routing-and-tool-grant-map.md:69` `Hold mcp__coursetools__web_search on the Researcher alone`) — the review rests on this repository's own standards.

### Skill activation scope

Which skill may the reviewer activate, and when?

- Permit `summarize-session` at the review boundary (`.claude/skills/summarize-session/SKILL.md:6-7` `when the context window is filling`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Hand no part of the review to another agent (`.claude/agents/reviewer.md:40` `hands no part of the review to another agent`).
- Apply the documentation standard from this role rather than delegating it (`.claude/agents/reviewer.md:47` `v2 is the current rule set`); its prose and citation half is converted and runs as `scripts/validate_doc_conformance_deterministic.py` (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`).

### Data classification ceiling

What classification may the reviewer read?

- Cap retrieval at `internal` (`.claude/agents/reviewer.md:88` `the pinned internal ceiling`).
- Write the review entry at `classification: "internal"` (`.claude/agents/reviewer.md:72` `classification: "internal"`).
- Deny `confidential` and `secret` content to the role (`docs/routing-and-tool-grant-map.md:89` `the ceiling stays at internal`) — this denial answers `NM-5`.
- Flag a claim the review cannot settle instead of asserting it (`.claude/agents/reviewer.md:55-56` `Do not delete the claim, and do not invent authority for it.`).

### Autonomy level

How far does the reviewer act before a human decides?

- Hold `low` autonomy (`.claude/agents/reviewer.md:26` `autonomy: low`).
- Report findings, and act on none of them (`.claude/agents/reviewer.md:36-37` `it never merges, fixes or re-runs anything itself`).
- Leave the release decision at the human checkpoint (`.claude/agents/reviewer.md:76-77` `name the human release-approval checkpoint as the decision's owner`).

**Container permissions** — the launcher mounts this repository read-only at `/workspace` for this role (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`), and mounts `/workspace/.memory` read-write so the granted entry write lands (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`). This role reads `/workspace`, writes no file there, and writes one review entry into `/workspace/.memory` (`.claude/agents/reviewer.md:92` `the reviewer edits nothing it reviews`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Role 6 — `project-manager`

What does the bracketing role hold, and what may it never read or write?

### MCP server and operation access

Which MCP server operations may the project-manager call?

- Grant `mcp__coursetools__task_tracker` on this role alone (`.claude/agents/project-manager.md:71` `Owned exclusively by this role`).
- Grant `mcp__storage__read_entry` and `mcp__storage__list_entries` (`.claude/agents/project-manager.md:12-13` `- mcp__storage__list_entries`).
- Deny `mcp__storage__write_entry` (`.claude/agents/project-manager.md:80` `the run's results are already recorded by the roles that produced them`) — this denial answers `NM-4`, where a checkpoint record borrowed the reviewer's voice.
- Deny `mcp__storage__update_entry` and `mcp__storage__delete_entry` (`.claude/agents/project-manager.md:81-82` `this role revises no record`).
- Deny `mcp__coursetools__file_read` (`.claude/agents/project-manager.md:74` `the ticket update rests on recorded results`).
- Deny `mcp__coursetools__file_write` and `mcp__coursetools__codebase_search` (`.claude/agents/project-manager.md:75-76` `the terminal role changes no artifact`).
- Deny `mcp__coursetools__shell` and `mcp__coursetools__test_runner` (`.claude/agents/project-manager.md:77-78` `would overwrite the evidence the status rests on`) — this denial answers `NM-7`.
- Deny `mcp__retrieval__retrieve` (`.claude/agents/project-manager.md:79` `performs no corpus lookup`).
- Route the checkpoint record through this role (`.claude/agents/project-manager.md:71` `Owned exclusively by this role`; `docs/iteration-log.md:386` `assign checkpoint records to the project-manager`) — this rule answers `NM-4`.

### Skill activation scope

Which skill may the project-manager activate, and when?

- Permit `summarize-session` at the run-closing boundary (`.claude/skills/summarize-session/SKILL.md:7` `when the user asks for a session summary`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Return an open question instead of guessing a ticket status (`.claude/agents/project-manager.md:61-63` `Guessing a status is not a ticket update.`).

### Data classification ceiling

What classification may the project-manager touch?

- Hold no retrieval ceiling and no write ceiling (`docs/routing-and-tool-grant-map.json:65` `"project-manager": "none"`).
- Deny `confidential` and `secret` content to the role (`.claude/agents/project-manager.md:80` `already recorded by the roles that produced them`) — this denial answers `NM-5`.
- Read ticket state and stored entries only (`.claude/agents/project-manager.md:33-34` `ticket state is the whole of its authority`).

### Autonomy level

How far does the project-manager act before a human decides?

- Hold `low` autonomy (`.claude/agents/project-manager.md:24` `autonomy: low`).
- Record the released status rather than deciding it (`.claude/agents/project-manager.md:37` `records the released status rather than deciding it`).
- Set no `Done` status without gate evidence (`.claude/agents/project-manager.md:53` `Every gate passed`) — this rule answers `NM-7`.

**Container permissions** — the launcher mounts this repository read-only at `/workspace` for this role (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`), and mounts no `/workspace/.memory` of its own (`scripts/run-agent.sh:99` `project-manager) ROLE_WS=ro; ROLE_MEM=none; ROLE_TARGET=ro ;;`). This role reads no file in `/workspace`, reads stored entries from `/workspace/.memory`, and writes neither (`.claude/agents/project-manager.md:75-76` `the terminal role changes no artifact`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Role 7 — `researcher`

What does the network role hold, and what may it never read?

### MCP server and operation access

Which MCP server operations may the researcher call?

- Grant `mcp__coursetools__web_search` on this role alone (`.claude/agents/researcher.md:61` `The workflow's only network lookup`).
- Grant `mcp__storage__write_entry` (`.claude/agents/researcher.md:62` `Writes the research entry at classification`).
- Deny `mcp__coursetools__file_read` (`.claude/agents/researcher.md:63` `the role holds no repository read`) — a repository claim never comes from the open web.
- Deny `mcp__coursetools__file_write` and `mcp__coursetools__codebase_search` (`.claude/agents/researcher.md:64-65` `a repository answer comes from the planner's or the reviewer's own search`).
- Deny `mcp__coursetools__shell` (`.claude/agents/researcher.md:66` `no lookup reaches the network by another path`).
- Deny `mcp__coursetools__test_runner` and `mcp__coursetools__task_tracker` (`.claude/agents/researcher.md:67-68` `gate execution is the tester's role`).
- Deny `mcp__storage__read_entry` and `mcp__storage__list_entries` (`.claude/agents/researcher.md:69-70` `the researcher searches no project memory`).
- Deny `mcp__storage__update_entry` and `mcp__storage__delete_entry` (`.claude/agents/researcher.md:71-72` `a second lookup is a second entry`).
- Deny `mcp__retrieval__retrieve` (`.claude/agents/researcher.md:73` `the researcher answers external questions only`).

### Skill activation scope

Which skill may the researcher activate, and when?

- Permit `summarize-session` inside this role's own context (`.claude/skills/summarize-session/SKILL.md:7` `when the user asks for a session summary`).
- Deny every other skill, because the repository ships exactly one skill file (`find .claude/skills -name SKILL.md` -> `1`).
- Return an unresolved point as an open question, never as a finding (`.claude/agents/researcher.md:46-47` `An unresolved point is returned as an open question, never as a finding.`).

### Data classification ceiling

What classification may the researcher write?

- Hold no retrieval ceiling (`docs/routing-and-tool-grant-map.json:66` `"researcher": "none"`).
- Write the research entry at `classification: "public"` (`.claude/agents/researcher.md:49` `classification: "public"`).
- Deny `confidential` and `secret` content to the role (`.claude/agents/researcher.md:50` `accepts public and internal writes only`) — this denial answers `NM-5`.
- Attribute every returned source in the entry (`.claude/agents/researcher.md:51-52` `every source the search returned`).

### Autonomy level

How far does the researcher act before a human decides?

- Hold `low` autonomy (`.claude/agents/researcher.md:25` `autonomy: low`).
- Answer the brief's question and decide nothing about the change (`.claude/agents/researcher.md:35` `the researcher returns the answer rather than acting on it`).
- Answer one question per call and return (`.claude/agents/researcher.md:61` `one question per call`).

**Container permissions** — the launcher mounts this repository read-only at `/workspace` for this role (`scripts/run-agent.sh:165` `MOUNTS+=(-v "$REPO:/workspace:ro")`), and mounts `/workspace/.memory` read-write so the granted entry write lands (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`). This role reads no file in `/workspace`, writes one `public` research entry into `/workspace/.memory`, and reaches the network only through its search tool (`.claude/agents/researcher.md:61` `The workflow's only network lookup`). The read-only `.memory/knowledge` and `.memory/reference` layers refuse this role's writes (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).
