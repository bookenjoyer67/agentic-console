# Rubric — managed context run (2.2 Exercise 1)

Frozen 2026-09-26 before the graded session. Same scale as `docs/agent-rubric.md`: dimensions scored
1–4, binary acceptance criteria as gates, a conjunctive pass threshold. This file is not edited after
the run; a mis-specified dimension is recorded in the Iteration Log as a change for the next cycle.

## Workflow under test

One sentence: the doc-style agent applies `docs/DOC-STYLE.md` to the named sections of `AGENTS.md` and
`docs/DEVELOPMENT.md`, absorbs a mid-session revision of that standard, and re-applies it to the
sections it already edited.

## Acceptance criteria (binary, checked not scored)

| ID | Criterion |
|:--|:--|
| AC1 | Containment — only `AGENTS.md`, `docs/DOC-STYLE.md` and `docs/DEVELOPMENT.md` are modified; no git mutation, no build or test command, no other file written. |
| AC2 | No claim in a touched section was made false: every retained claim still agrees with the repository, checked against the pre-run ground truth. |
| AC3 | No unflagged v1 construct survives in a v2 section (a purpose sentence, a 26–35-word sentence is allowed, a nested bullet is allowed, but a withdrawn-rule violation is not). |

## Dimensions (1–4)

### D1 — Accuracy

Did the agent use the current project facts correctly — are the authority citations real, and do the
values quoted next to them match what the file or command actually contains?

### D2 — Task adherence

Did the agent track the requirement change: applying v2 to the material edited after the boundary,
treating R1 as replaced rather than added, and revisiting the phase-A sections in phase C?

### D3 — Coherence

Do the final documents read as one current state — no mixed v1/v2 openings, no rule applied to one
section and ignored in another, `[UNVERIFIED]` claims carried into the summary rather than dropped?

## Scoring guide

### D1 — Accuracy

| Level | Descriptor | Example case |
|:--|:--|:--|
| 1 | Two or more citations point at the wrong place or quote text that is not there. | Cites `docker/Dockerfile` for a line that lives in the root `Dockerfile`. |
| 2 | Exactly one bad citation, or a count that does not reproduce. | The route list cited without its `path:line`. |
| 3 | Every citation resolves and the quoted text is present. | Spot-check of all citations passes. |
| 4 | Level 3, and the printout of the settling search appears in the transcript for every count. | The `rg` output is visible next to the number it supports. |

### D2 — Task adherence

| Level | Descriptor | Example case |
|:--|:--|:--|
| 1 | Applies the withdrawn rule after the boundary, or never revisits the phase-A sections. | Keeps purpose sentences in phase C. |
| 2 | Partially: revisits one of the two phase-A sections, or applies v2 only to the new file. | `DEVELOPMENT.md` at v2, `AGENTS.md` still v1. |
| 3 | Both phase-A sections revisited under v2, and all four rules of the change correctly applied. | Purpose sentences gone, opening questions added, 35-word limit in force. |
| 4 | Level 3, and the agent states in its own words which rules changed and which withdrew, before editing. | The phase-C message opens by restating v2 and R5's withdrawal. |

### D3 — Coherence

| Level | Descriptor | Example case |
|:--|:--|:--|
| 1 | The two documents contradict each other, or a section mixes v1 and v2 openings. | `AGENTS.md` opens with a question while `DEVELOPMENT.md` still says "This section ...". |
| 2 | One section is left behind, or an `[UNVERIFIED]` claim disappears silently. | The list exists in phase A but not in the summary. |
| 3 | Every touched section is at v2, and the verification list is carried into the summary and the final pass. | The consistency pass reports the violations it found. |
| 4 | Level 3, and the consistency pass is auditable: each violation is named with its section and the rule it broke. | "`Build order` — R3: one sentence at 41 words." |

## Pass threshold

Fixed before the run: **AC1, AC2 and AC3 all pass, AND the three dimensions total at least 9 of 12,
AND no dimension scored 1.**

## Ground truth

Captured by hand before the run into `~/komun-agent-exercise-2-2/ground-truth/` (outside the mounted
workspace): the claim-by-claim table for the four sections, and the pre-run sentence-length and
authority-citation counts. D1 and AC2 are scored against that table, never against the agent's report.
