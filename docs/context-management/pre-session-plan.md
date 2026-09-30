# Pre-session plan — manage context in a long-running agent session

Exercise: Canvas 269 (Agentic Engineer), assignment 24204, "2.2 Exercise 1: Manage Context in a
Long-Running Agent Session". Written and committed before the graded session starts.

## The task (real project work)

A documentation standard exists at `docs/DOC-STYLE.md` (v1, frozen). The agent applies it to two
documentation files, section by section, and then re-applies a revised standard to the sections it
already edited. The output is real: the standard is enforced on `AGENTS.md` (the cold-start guide) and
on `docs/DEVELOPMENT.md`, every claim in the touched sections ends up with an authority a reader can
re-check, and claims that cannot be traced by reading are collected into an explicit verification list
instead of being asserted.

Not in scope: any file other than `AGENTS.md`, `docs/DOC-STYLE.md` and `docs/DEVELOPMENT.md`; any code,
config, migration or test; any git command; any build or test command.

## The agent

`.claude/agents/komun-docs-stylist.md`, committed before the first run. Project-scoped, tools
`Read, Grep, Glob, Edit, Write` — no `Bash`, so containment is checkable rather than asserted. It is
context-dependent by construction: each section it edits depends on the rule set currently in effect
and on the state of sections it edited earlier in the same session.

## Phases

| Phase | Focus | Rules in effect | Files |
|---|---|---|---|
| A | Apply the standard to the two opening sections | v1 (R1–R5) | `AGENTS.md` ("What this is", "Critical rules") |
| — | Context boundary + proactive summary before any new information is introduced | v1 still in effect | summary artifact |
| B | Apply the revised standard to new material | v2 (R3 raised, R1 replaced by R2', R2 strengthened, R5 withdrawn) | `docs/DOC-STYLE.md` (revised to v2), `docs/DEVELOPMENT.md` ("Prerequisites", "Build order") |
| C | Revisit the phase-A sections under the current rules, then a consistency pass over all four sections | v2 | `AGENTS.md` sections from phase A, both files |

## The requirement change (phase A -> phase B)

Realistic for this project: a first pass through the guide shows that a 25-word limit fights the
authority requirement, that "This section ..." openings add a line of noise to a cold-start guide, and
that the authority parenthetical needs the literal value next to it to be worth anything. Effective
immediately:

1. R3 — sentence limit raised from 25 to 35 words.
2. R1 — **replaced**: a section now opens with the question it answers (`What is this?`), and the
   existing purpose sentences are removed. R1 is explicitly no longer in effect.
3. R2 — strengthened: the authority parenthetical must name the artifact **and** the literal text,
   value or count that settles the claim.
4. R5 — **withdrawn**: nesting is allowed where it shows real hierarchy.

## The artifact revisited later

`AGENTS.md`'s "What this is" and "Critical rules" sections, edited in phase A under v1, are revisited in
phase C under v2. This is the coherence test: the final documents must not mix v1 openings with v2
rules, and no phase-A rule may survive the phase-C pass.

## Evidence for evaluation

1. `git diff` of the two documents, per phase, plus the committed versions.
2. A host-side rule check over the edited sections: sentence lengths (25-word and 35-word counts),
   presence of a literal purpose sentence in any v2 section, authority parentheticals present per claim,
   `[UNVERIFIED]` markers, nesting.
3. A claim-by-claim ground-truth table captured by hand **before** the run, held outside the mounted
   workspace, so accuracy is scored against the repository and not against the agent's own report.
4. The Claude Code transcript JSONL from the container: which messages carried a boundary preamble,
   whether the summary was produced at the planned boundary before the new rules arrived, which files
   the agent read, and whether phase-A sections were re-read before phase C.
5. The session summary artifact saved from the session.

## Rubric

`docs/context-management/rubric.md`, frozen before the run: Accuracy, Task Adherence and Coherence, each
1–4 on the project's existing scale, with binary acceptance criteria and a conjunctive pass threshold.

## Known risks

- Both documents are long; the run is bounded to four sections plus the consistency pass so
  "locally reasonable but off-task" drift is visible rather than hidden by volume.
- The agent may paraphrase the authority it greps instead of quoting the matched text — the failure
  mode the 2.1 cycle already proved, which is why v2 strengthens R2.
- The workspace contains the earlier iteration log and the 2.1 artifacts; they are not an answer key for
  a style task, so they stay in place, and the transcript check reports whether they were read.
