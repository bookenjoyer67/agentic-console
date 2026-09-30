# handoff-subagent-to-orchestrator.md

The shape of the result a role returns to the orchestrator when its task is finished. Every delegation
in this workflow is answered in this shape.

**Read-only.** `.memory/knowledge/` is human-maintained; the agent never writes here.

## Why the shape is fixed

The orchestrator evaluates a result against the brief's required output format before the next role
starts (`CLAUDE.md` `## Orchestration` → Evaluation gate). A result that omits **what was produced** or
**the acceptance criterion it claims to satisfy** is returned to the same role once, with the specific
defect named. A second failure of the same kind escalates to the human.

## The six fields every result carries

| field | what it carries |
|---|---|
| **What was done** | the work, in the order it happened, in one short paragraph |
| **What was produced** | every artifact the role made: files written, entries recorded, verdicts reached. Name them; do not describe them |
| **Entry ids** | the `entry_id` of every storage entry the role wrote or updated, or `none` and why |
| **Acceptance criteria claimed** | which of the brief's numbered criteria this result claims to satisfy, by number, and the artifact that shows it |
| **Open questions** | anything the role could not settle from the repository as it stands |
| **Blockers** | a precondition that was missing, and the artifact that supplies it |

Never claim a criterion the artifact does not show. "Satisfies criterion 2" without the artifact that
demonstrates it is the defect the gate exists to catch.

## What each role adds to the six

| role | fields this role's result must also carry | source |
|---|---|---|
| `planner` | the plan text: ordered steps, the file each step writes, the gate each step clears, the acceptance criteria, the risk per step. Plus the checkpoint the run stops at | `planner.md` |
| `implementer` | the file list written; the gates the tester runs, **and** the statement that this role executed none of them | `implementer.md` |
| `tester` | one `PASS` / `FAIL` / `INCONCLUSIVE` verdict per gate with the raw runner output and its counts; any gate left inconclusive and the prerequisite it waits on | `tester.md` |
| `reviewer` | a verdict with per-finding entries: file, line, rule quoted, literal text, and a `blocking` or `advisory` classification | `reviewer.md` |
| `project-manager` | the ticket, requested status, update performed, final status, the note added, the tool result, and any error the tool returned | `project-manager.md` |
| `researcher` | the answer with its key facts and sources, and any point the lookup did not settle | `researcher.md` |

## The empty template

```
## What was done
<one short paragraph>

## What was produced
- <file written | entry recorded | verdict reached>

## Entry ids
- <entry_id>, or: none — <why>

## Acceptance criteria claimed
- criterion <n>: <the artifact that shows it>

## Open questions
- <question the repository does not settle>

## Blockers
- none, or: <the missing precondition and the artifact that supplies it>

## <this role's added fields, from the table above>
```

## The evaluation gate's three checks

The orchestrator performs all three before delegating to the next role:

1. **The format** — the result carries this shape's fields and the ones this role adds.
2. **The entry id** — the id the role claims to have written is real. `calling_role` in the storage and
   gate journals is caller-supplied free text, so an entry id is checked against the journal, not
   against the claim.
3. **The acceptance criterion** — the criterion the role claims is one the brief actually named, and
   the artifact it cites shows what is claimed.

## What makes a result a defect

- It omits what was produced, or omits the acceptance criterion it claims to satisfy.
- It reports a gate as run when the journal has no row for it, or reports a gate as green without its
  exit code.
- It names an artifact it did not write, or an `entry_id` it did not receive back.
- It settles a claim from another role's prose. A green gate is a journal row with an exit code; a
  role's summary of a green gate is a claim about a claim.
- It reports a passing count without the failure count. A predicate that requires a pass count and not
  zero failures reports PASS on a tree that exited non-zero.
