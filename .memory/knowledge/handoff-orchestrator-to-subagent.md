# handoff-orchestrator-to-subagent.md

The shape of the brief the orchestrator sends when it delegates to a role. Every role definition
receives its brief in this shape and reads it before doing anything else.

**Read-only.** `.memory/knowledge/` is human-maintained; the agent never writes here.

## Why the shape is fixed

The orchestrator's evaluation gate checks a returned result against **the brief's own required output
format** (`CLAUDE.md` `## Orchestration` → Evaluation gate). If a brief leaves that field out, the
result cannot be evaluated and the gate has nothing to check. A brief that names no acceptance criteria
produces a result that claims to satisfy none.

## The six required fields

Every delegation carries all six. Each is a heading in the brief, in this order.

| field | what it carries | why it is required |
|---|---|---|
| **Role context** | the role's name, exactly as `.claude/agents/<name>.md` spells it, and the one job it is being asked to do this time | the role reads its own definition; this says which of its duties is in play |
| **Task brief** | the change request in one or two sentences, and the plan `entry_id` when one exists | the implementer acts on the plan entry, never on a summary of it |
| **Input materials** | the exact artifacts the role reads: file paths, `entry_id` values, the brief's own attachments, the revision the run started from | a role may not widen its own read surface; anything it needs is named here |
| **Acceptance criteria** | the criteria the result will be judged against, in testable form, one per line | the result must claim which of these it satisfies, and the gate checks that claim |
| **Required output format** | the fields the returned result must carry, drawn from `handoff-subagent-to-orchestrator.md` plus any this role adds | the gate compares the result to this field, not to a remembered standard |
| **Bounds** | what is out of scope, what the role must not touch, and the checkpoint the run is at | scope creep arrives as a helpful extra file; this is what makes it refusable |

## Also state, when they apply

- **The project id** — `proj-console` for this repository — for any role that reads or writes storage
  or queries the corpus. A role handed no project id cannot scope its own calls.
- **The checkpoints.** Which of the two human checkpoints this delegation sits behind or ahead of. No
  implementer is delegated before the plan-approval checkpoint is cleared.
- **The engine flags.** Anything that changes how the run is executed (`--resume <session-id>`, the
  permission mode) is harness state, not role instruction; label it as harness-supplied so a role does
  not read it as a ruling.

## The empty template

```
## Role context
Role: <one of the seven, spelled as its definition file spells it>
This task: <one sentence>

## Task brief
<the change request in one or two sentences>
Plan entry: <entry_id, or "none — this role runs before a plan exists">

## Input materials
- <path or entry_id, one per line>
- revision this run started from: <sha>

## Acceptance criteria
1. <testable criterion>
2. <testable criterion>

## Required output format
<the fields the returned result must carry>

## Bounds
- Out of scope: <what this role must not touch>
- Checkpoint: <which human checkpoint this sits behind, or "none">
```

## A worked example, for this repository

```
## Role context
Role: planner
This task: turn the change request below into an ordered plan with the file list it touches.

## Task brief
Make the seam table and the running servers agree on the gate, storage and retrieval ports.

## Input materials
- agentic.config.json
- scripts/start-mcp-servers.sh
- mcp/gate/server.py
- revision this run started from: 0898175

## Acceptance criteria
1. The port the gate server listens on and the port `console.ports` publishes are the same number.
2. One file decides that number; the others read it.
3. `bash scripts/port-self-test.sh --fork` still prints `port-self-test: PASS`.

## Required output format
Plan text: ordered steps, the file each step writes, the gate each step clears, the acceptance
criteria, the risk per step. Plus `entry_id`, open questions, blockers.

## Bounds
- Out of scope: the CI workflow, the reference project's repository.
- Checkpoint: this plan stops at checkpoint 1 for human approval; no implementer starts first.
```

## What makes a brief a defect

- It names no acceptance criteria, or names them as prose that cannot be checked.
- It carries a task the role's `disallowedTools` deny — asking the planner to run a gate, asking the
  implementer to run a test.
- It asks for work outside the current checkpoint's authority.
- It restates repository rules from memory instead of naming the artifact that states them. A brief is
  the least-verified artifact in the loop, and roles have refuted what a brief asserted from memory.
