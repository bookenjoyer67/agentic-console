# Step classification

Which agentic steps run in this repository, and which of them earn deterministic conversion?

Cadence note, quoted from Lesson 4.3, block [193] (VERBATIM): "This document is updated after every
calibration cycle. Steps that cross the stability threshold are promoted to candidate status. A step
that has been a candidate for more than two calibration cycles without meeting all four signals is
reviewed for re-scoping."

## The four signals

What has to hold before a step counts as a candidate?

- Show **stability**: the step ran through at least two recorded calibration cycles with a consistent
  outcome (`docs/iteration-log.md:706` `Run 003 (workflow 3`; `docs/iteration-log.md:744` `16 / 16`).
- Show **repeatability**: the same input produced the same output, checked rather than assumed
  (`docs/iteration-log.md:846` `All 96 citations were checked programmatically`).
- Show **specifiability**: one paragraph lets a developer implement the step, because the standard
  names each rule's detection (`docs/DOC-STYLE.md:24` `How a violation is detected`).
- Show a **high run rate**: the step runs on every review of a prose change
  (`.claude/agents/reviewer.md:47` `Apply docs/DOC-STYLE.md to every prose file the change touches`).

Apply the matrix per step rather than per workflow (Lesson 4.3, block [15], VERBATIM). The table
below applies it to this repository's own recorded work.

## Conversion status

Which steps hold their place as agentic work, and which ones are now deterministic?

| Step | Classification | Converted to | Record |
|---|---|---|---|
| Prose and citation conformance check | agentic until this conversion, now deterministic | `scripts/validate_doc_conformance_deterministic.py` | `docs/adr/ADR-001-doc-conformance-deterministic-conversion.md` |
| Gate execution (`test`, `clippy`, `fmt`, `policy`, `conformance`, `webcheck`, `webtest`) | deterministic | `mcp/gate/server.py` | `docs/iteration-log.md:318` `a fourth MCP server, mcp/gate/server.py` |
| Change classification | deterministic | `scripts/classify-change.py` | `scripts/classify-change.py:12` `Change Classifier` |
| Audit-trail assembly | deterministic | `scripts/build-audit-trail.py` | `scripts/build-audit-trail.py:2` `Assemble the run's audit trail` |
| Plan authoring | agentic | — | `.claude/agents/planner.md:23` `autonomy: medium` |
| Implementation | agentic | — | `.claude/agents/implementer.md:23` `autonomy: medium` |
| Review findings and verdicts | agentic | — | `.claude/agents/reviewer.md:47` `v2 is the current rule set` |
| Ticket bracketing | agentic | — | `.claude/agents/project-manager.md:37` `records the released status rather than deciding it` |
| External research | agentic, on request | — | `.claude/agents/researcher.md:35` `the researcher returns the answer rather than acting on it` |
| Gate sequencing and escalation | agentic, by design | — | `.claude/agents/orchestrator.md:18` `The orchestrator sequences the work` |

## Step: prose and citation conformance check — CONVERTED

Why was this step the strongest candidate in the repository?

- **Stability.** The step ran in one full documentation-standard cycle and in three citation cycles.
  The documentation-standard run scored `11 / 12` (`docs/iteration-log.md:628` `| **Total** | **11 / 12**`).
  The citation cycles scored 14, 14 and 16 out of 16 (`docs/iteration-log.md:744` `16 / 16`).
- **Repeatability.** Phase C of the documentation-standard run re-measured the same four sections and
  reported `0 non-question openers` (`docs/iteration-log.md:626` `0 non-question openers`). The
  contract-auditor cycles re-executed 96 citations and found one defect (`docs/iteration-log.md:774`
  `M6 — one citation is one line off (D2, minor).`).
- **Specifiability.** Each rule carries its own detection clause in the standard
  (`docs/DOC-STYLE.md:24` `How a violation is detected`), and the authority forms name the three
  accepted shapes (`docs/DOC-STYLE.md:40` `A bare location is a v1 form`). One paragraph describes
  the step: check each heading's opener, each claim's parenthetical, each sentence's word count, each
  rule bullet's hedges, then re-execute every `path:line` citation behind a claim.
- **Run rate.** Every review of a prose change runs it (`.claude/agents/reviewer.md:47`
  `Apply docs/DOC-STYLE.md to every prose file the change touches`).
- **Known agent judgment, and the edge cases the deterministic version must preserve.** A claim that
  cannot be traced keeps its place and gets the marker, never invented authority
  (`docs/DOC-STYLE.md:72` `Do not delete the claim, and do not invent authority for it.`). A citation
  whose quoted text is present one line away from its pointer counts as a defect, not a pass
  (`docs/iteration-log.md:774` `M6 — one citation is one line off (D2, minor).`). A citation whose
  quoted text sits in a different file counts as a defect (`docs/iteration-log.md:832`
  `The citation therefore does not resolve to the text it quotes`).
- **Limitation the script reports instead of deciding.** R4's "first word is not a verb" clause needs
  a verb lexicon the standard does not define, so the script checks the four named hedge words and
  states the gap in every report (`docs/DOC-STYLE.md:29` `A rule is an imperative`).
- **Recommendation:** strong candidate, converted. Status: **Converted** under
  `docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`.
- **Preserved edge case:** a line carrying `[UNVERIFIED]` is exempt from the bare-location check
  (`docs/DOC-STYLE.md:72` `Mark the sentence with`).
- **Known false positive, pinned by a test.** Pairing is structural, so the checker takes the literal inside a pointer's parentheses or beside it on one line (`scripts/validate_doc_conformance_deterministic.py:111` `Citation pairing is structural`). A pointer whose literal wraps to the next line therefore takes a neighbouring literal and reports `CIT-LINE-DRIFT` on a citation that is correct.
- **Two live citations carry that false positive.** `AGENTS.md:29` and `AGENTS.md:45` both cite `web/package.json` correctly, and both come back as drift. `eval/test_deterministic_step.py` pins the behaviour on a fixture, so the limit is recorded rather than argued.
- **Integration status.** Clauses 2, 3 and 4 are in place, and clause 1 holds in the orchestrator's definition while its canonical `CLAUDE.md` line is held by a write guardrail (`docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`). Clause 5, the single commit, is the human's.
- **Clause 4 deviates, and the deviation is deliberate.** The retired definition stays in place and is annotated instead of deleted (`.claude/agents/komun-docs-stylist.md:18` `The prose and citation conformance step this definition ran now runs as`). Reason: it is a graded Module 1/2 deliverable that the earlier module's record cites.
- **Next review:** 2026-10-13, after the next calibration cycle.

## Step: gate execution — already deterministic

What moved the gate commands out of a role's hands?

- Bind each gate to a name, with no command string and no extra argument
  (`mcp/gate/server.py:123` `it accepts no command string, no extra arguments`).
- Keep the recorded evidence in one place per gate (`docs/iteration-log.md:395`
  `158 passed, 0 failed, 0 ignored`).
- Note the record that motivated the conversion: an inert grant blocked every gate in the first
  orchestrated run (`docs/iteration-log.md:305-306` `the course's deliberately inert stub`).
- Run the two converted deterministic steps through the same vocabulary, as the `policy` and `conformance` gates (`mcp/gate/gate_vocabulary.py:226` `this repository's eight names: seven check-mode and one write-mode.`).
- Read that vocabulary as eight commands in two modes: the seven check-mode names `test`, `clippy`, `fmt`, `policy`, `conformance`, `webcheck` and `webtest`, and one write-mode name, `fmt-fix` (`agentic.config.json:85` `"fmt-fix": {`).
- Bind `fmt-fix` to `cargo fmt --all`, the one command on this surface that rewrites files (`agentic.config.json:86` `"argv": ["cargo", "fmt", "--all"],`).
- Split the vocabulary on that mode, so the check surface and the write surface resolve against disjoint tables (`mcp/gate/gate_vocabulary.py:248` `FIX_COMMANDS: dict[str, dict[str, Any]] = {`).
- **Next review:** 2026-11-30. The server is deterministic, and its selftest covers the refusal path.

## Step: change classification — already deterministic

Which part of the pipeline never needs a model?

- Decide agentic work from the changed paths alone (`scripts/classify-change.py:12` `Change Classifier`).
- Keep the two job outputs the eval gate keys off (`docs/routing-and-tool-grant-map.md:7`
  `Update an agent definition to match this map`).
- **Next review:** 2026-11-30. The classifier answers a path question, and a path question has one
  answer.

## Step: audit-trail assembly — already deterministic

Why does the trail job hold no model call?

- Combine the other jobs' artifacts into one record (`scripts/build-audit-trail.py:2`
  `Assemble the run's audit trail from the artifacts the workflow downloaded`).
- Run after the other jobs and preserve evidence rather than decide a merge
  (`docs/calibration-log.md:62` `Unreproducible self-report`).
- **Next review:** 2026-11-30. The job copies and summarises recorded artifacts.

## Step: plan authoring — stays agentic

When does a plan need a model rather than a template?

- A plan needs the repository read as a whole and a judgment about scope
  (`.claude/agents/planner.md:23` `autonomy: medium`).
- A planner starved of its reads produced a plan resting on a brief
  (`docs/iteration-log.md:334-335` `its plan rested entirely on the ten-line quotation`).
- **Next review:** 2026-10-27. A plan names files that no rule can predict from the request alone.

## Step: implementation — stays agentic

When does a change need a model rather than a script?

- The writing role holds no command runner and returns each change for independent test
  (`.claude/agents/implementer.md:23` `autonomy: medium`).
- The change under gate in the last run was a two-line lint repair (`docs/iteration-log.md:402`
  `two files, 8 insertions, 2 deletions`).
- **Next review:** 2026-10-27. The work is specifiable per change and not repeatable across changes.

## Step: review findings and verdicts — stays agentic

Why does the review step need judgment rather than a rule set?

- The verdict weighs a change against the rubric and the acceptance criteria
  (`.claude/agents/reviewer.md:47` `v2 is the current rule set`).
- One review class is now deterministic: the prose rules and the citations inside it
  (`.claude/agents/reviewer.md:47` `Apply docs/DOC-STYLE.md to every prose file the change touches`).
- **Next review:** 2026-10-27. The prose half of the review moved to the converted step; the verdict
  half rests on the plan and the gate evidence.

## Step: ticket bracketing — stays agentic

Which step records a decision that a rule cannot make?

- The role brackets a run and records a released status
  (`.claude/agents/project-manager.md:37` `records the released status rather than deciding it`).
- A missing row in the grant map let a role write a checkpoint record it judged
  (`docs/iteration-log.md:375-376` `the same role whose verdict`).
- **Next review:** 2026-12-15. Ticket state is small, and its failure mode is a wrong writer rather
  than a wrong rule.

## Step: external research — stays agentic

When does a question leave the repository?

- The stretch role answers one question from the open web and returns
  (`.claude/agents/researcher.md:35` `the researcher returns the answer rather than acting on it`).
- The role holds the only network tool in the workflow (`docs/routing-and-tool-grant-map.md:69`
  `Hold mcp__coursetools__web_search on the Researcher alone`).
- **Next review:** 2026-12-15. One question per call resists a rule set, and the role decides nothing
  about the change.

## Step: gate sequencing and escalation — stays agentic, by design

Which step must keep a human in the loop?

- The orchestrator decides loop, skip, halt and escalate, and owns no checkpoint
  (`.claude/agents/orchestrator.md:18` `The orchestrator sequences the work`).
- Two records show why judgment stays here: a role's self-reported count drifted
  (`docs/iteration-log.md:625` `the closing report's self-reported edit count does not reproduce`),
  and a role called a tool its own map denies (`docs/iteration-log.md:355` `the orchestrator called`).
- **Next review:** 2026-12-15. Escalation is a judgment about a run, and no rule set settles it.
