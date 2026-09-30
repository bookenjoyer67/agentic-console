# ADR-001: Convert the prose and citation conformance check from agent to deterministic code

Which agentic step does this record convert, and what does the record show about it?

## Status

What is this decision's current status?

**Accepted.** The script runs in the workflow through the `conformance` gate, the governance artifacts
are in sync, and the integrated end-to-end regression passed on 2026-09-29 across four runs
(`docs/calibration-log.md`). Lesson 4.3, block [44] (VERBATIM):

## Context

Which step is this, and what does the record show about it?

The orchestration checks prose against `docs/DOC-STYLE.md` and re-executes the citations behind each
claim before a change reaches the human checkpoint. Two roles ran that step by hand
(`.claude/agents/reviewer.md:47` `Apply docs/DOC-STYLE.md to every prose file the change touches`).

The record shows the step ran repeatedly with a stable outcome. The documentation-standard run scored
`11 / 12` (`docs/iteration-log.md:628` `| **Total** | **11 / 12**`), and its consistency pass produced
"six numbered violations, each with its file, the rule it broke, the search that settled it and the fix"
(`docs/iteration-log.md:627` `six numbered violations`). The citation half ran through three cycles
that scored 14, 14 and 16 out of 16 (`docs/iteration-log.md:744` `16 / 16`).

The same record shows the step is expensive and defect-prone when an agent runs it. The
documentation-standard run cost 45 minutes of wall clock and $8.37 (`docs/iteration-log.md:639`
`45m13s wall`; `docs/iteration-log.md:645` `$8.37`). Human review of one citation cycle took about two
minutes with a script (`docs/iteration-log.md:757` `≈2.1 min`). Three defect classes recurred across
cycles: a citation to the wrong file (`docs/iteration-log.md:832`
`The citation therefore does not resolve to the text it quotes`), a citation one line away from its
text (`docs/iteration-log.md:774` `M6 — one citation is one line off (D2, minor).`), and a quoted
literal that does not reproduce (`docs/iteration-log.md:675` `-> 37`).

Three of those defects survived an agent that had been told to quote its evidence, so the failure
class is mechanical rather than motivational. The calibration log already names the pattern and the
control it argues for (`docs/calibration-log.md:62` `Unreproducible self-report`;
`docs/calibration-log.md:85` `Require the literal output behind every number`).

The edge case this step's own record names, restated for prose: a quoted literal that sits one line
away from its pointer. The agent treated that as a defect rather than a pass
(`docs/iteration-log.md:774` `M6 — one citation is one line off (D2, minor).`).

## Decision

What replaces the step, and what contract does the replacement hold?

Replace the agentic step with `scripts/validate_doc_conformance_deterministic.py`. The script reads
each prose file named on the command line, applies R1 to R4 as `docs/DOC-STYLE.md` defines their
detection (`docs/DOC-STYLE.md:24` `How a violation is detected`), re-executes every citation that
carries a quoted literal, and writes a JSON report to a named file.

The contract holds four clauses from Lesson 4.3, block [65] (VERBATIM): "Same input", "Same output
format", "No language model" ("no calls to the OpenRouter API, no agent invocations, and no inference
of any kind"), and "Readable on its own".

- Hold the input: the script reads the files named by `--input` (`docs/DOC-STYLE.md:15` `A sentence that only describes the document itself is not a claim`).
- Hold the output format: the report is JSON with one entry per input file, and it carries no
  timestamp, so two runs are byte-identical.
- Hold the no-model clause: the script imports `argparse`, `json`, `os`, `re` and `sys` alone.
- Return three exit statuses: 0 for clean, 1 for violations found, and 2 for a wrong invocation.
- State the limits in every report rather than guessing: R4's verb-initial clause needs a lexicon the
  standard does not define (`docs/DOC-STYLE.md:29` `A rule is an imperative`), command authorities are
  counted and left unverified, and a pointer whose literal sits far from it is counted as skipped.

Lesson 4.3, block [52] (VERBATIM), the rollback line: "**Rollback:** the conversion will be made as a
single commit in the next section so it can be undone with one git revert if the script produces a
wrong result in the running workflow."

## Alternatives considered

Which alternatives were rejected, and why?

- **Keep the agentic step.** Rejected. The step's cost is recorded at 45 minutes and $8.37 per run
  (`docs/iteration-log.md:639` `45m13s wall`), and the same three defect classes recurred across four
  cycles (`docs/iteration-log.md:675` `-> 37`). A rule set that never changes does not need reasoning
  per run.
- **Convert the review verdict instead.** Rejected. The verdict weighs a change against the rubric and
  the acceptance criteria, so it is not specifiable as a rule set
  (`.claude/agents/reviewer.md:47` `v2 is the current rule set`). The prose half of the same role is
  specifiable, and that half is what this ADR converts.
- **Keep an agent and gate it with the script.** Rejected. An advisory script beside an agent leaves
  the agent as the step the workflow runs, so the cost stays
  (`docs/iteration-log.md:639` `45m13s wall`). Lesson 4.3, block [53] (VERBATIM): "Deterministic checks
  can gate immediately because they produce the same result for the same input."
- **Extend `eval/test_policy.py` instead of adding a script.** Rejected. That suite checks the
  governance policy against the enforcement artifacts
  (`docs/governance-policy.md:13` `patterns NM-1 to NM-9`). Prose conformance is a different question
  and belongs in its own file with its own tests.

## Consequences

What does the conversion change, and what does it leave open?

- The step drops to one process launch: 0.41 to 0.90 seconds across eight runs, against 45 minutes of
  agent cycle time (`docs/iteration-log.md:639` `45m13s wall`).
- The step's token cost drops to zero, because the script makes no model call.
- The step becomes diffable: three runs over the same ten files produced one SHA-256 digest
  (`e832693c0c7845f2cdca08cd97062e36dd0d4cc1c79649593ebf1d02f0a5a111`).
- The script found eight citation defects across ten repository files, a ninth in
  `docs/step-classification.md`, and a tenth in this ADR, the last two written in this session.
- Four checks stay outside the script, and it reports each gap in every run: R4's verb-initial clause,
  command authorities, parentheticals that name no artifact, and citations whose literal sits far from
  the pointer.
- Integration is closed for lesson clauses 1 to 4. The orchestrator's definition runs the script
  (`.claude/agents/orchestrator.md:80` `on each prose file the change touches`), the routing map holds
  the step's MCP access as empty (`docs/routing-and-tool-grant-map.json:71` `"mcp_access": []`), and the
  policy's reviewer entry carries the pointer (`docs/governance-policy.md:229` `its prose and citation
  half is converted`).
- Clause 1 is half-satisfied, and the gap is named. The canonical instruction file is `CLAUDE.md`
  (`## Orchestration`), and the write to it was refused by this session's guardrail, so the orchestrator
  definition carries the instruction instead. One line stays pending: `CLAUDE.md:105`
  `reviews the diff against` is the text the script invocation replaces.
- Clause 4 is deviated from, deliberately. The definition and its skill files stay in place, annotated
  instead of deleted. Reason: `komun-docs-stylist` is a graded Module 1/2 deliverable whose record the
  earlier module cites, and the definition is tracked at commit `311b7b7`, so a later deletion is
  recoverable rather than destructive.
- The checker's false positive is recorded rather than hidden. A pointer whose literal wraps to the next
  line takes a neighbouring literal and reports `CIT-LINE-DRIFT` on a correct citation. Two live
  examples are left unrepaired: `AGENTS.md:29` and `AGENTS.md:45` both cite `web/package.json` correctly
  (`web/package.json:12` `"@sveltejs/adapter-static": "^3.0.0",`) and
  (`web/package.json:20` `"svelte": "^5.0.0",`). `eval/test_deterministic_step.py` pins the behaviour, so
  the limit is a recorded fact rather than an argument.
- The changes await one commit, and the human owns that commit. Its paths are
  `CLAUDE.md` (pending the guardrail), `.claude/agents/orchestrator.md`,
  `.claude/agents/komun-docs-stylist.md`, `docs/routing-and-tool-grant-map.md`,
  `docs/routing-and-tool-grant-map.json`, `docs/governance-policy.md`, `docs/step-classification.md`,
  `docs/calibration-log.md`, `docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`,
  `scripts/validate_doc_conformance_deterministic.py` and `eval/test_deterministic_step.py`. One
  `git revert` of that commit undoes the conversion, as Lesson 4.3, block [52] (VERBATIM) requires.
- The CI job stays open, because `.github/workflows/ci.yml` is held by another workstream in this
  session, and it runs no conformance job yet.
- Acceptance is settled. The regression passed on 2026-09-29, and its four runs are recorded in
  `docs/calibration-log.md` with per-run wall clock, gate evidence and verdicts.

## Evidence

Which artifacts settle this decision?

- Classification: `docs/step-classification.md`, the prose and citation conformance check marked a
  strong candidate on all four signals.
- Before-conversion measurement: `docs/iteration-log.md:639` `45m13s wall` and
  `docs/iteration-log.md:645` `$8.37`, scored `11 / 12` (`docs/iteration-log.md:628`
  `| **Total** | **11 / 12**`).
- After-conversion measurement in isolation: eight runs over ten files, one digest
  (`e832693c0c7845f2cdca08cd97062e36dd0d4cc1c79649593ebf1d02f0a5a111`), 0.41 to 0.90 seconds, zero
  tokens, 273 citations checked and 265 resolved at the cited line.
- Unit tests: `eval/test_deterministic_step.py`, 15 tests, `15 passed` under pytest 9.1.1.
- After-conversion measurement of this integration: three runs over the same ten files produced one
  digest (`15f1c690dbfdb0e1c19f78237836ce1669b47de47c176a98dfe56a943cc61af5`), in 0.844s, 0.424s and
  0.422s, with zero tokens and $0.00, 299 citations checked and 289 resolved.
- Before and after in one place: `docs/calibration-log.md:88` `## Before and after the deterministic conversion`,
  which records the agentic run's 45m13s, $8.37 and 11 / 12 against the script's three runs.
- The false positive's live count: two citations, `AGENTS.md:29` and `AGENTS.md:45`, both correct and
  both reported as drift (`web/package.json:12` `"@sveltejs/adapter-static": "^3.0.0",`), and pinned by
  `eval/test_deterministic_step.py`.
- Suites after the integration: `eval/test_policy.py` `75 passed`, and
  `eval/test_deterministic_step.py` `15 passed`.
- Defect classes the script now reproduces on demand: a drifted pointer
  (`docs/iteration-log.md:450` `pieces = chunker(section)`), a literal absent from its file
  (`docs/memory-architecture.md:49` `.env*`), and a bare location
  (`docs/DOC-STYLE.md:40` `A bare location is a v1 form`).
- Integrated end-to-end regression check: PASSED on 2026-09-29. Four runs - D1 and D2 development, H1 and
  H2 holdout - each reached a reviewer verdict, with the `conformance` gate run by the tester in every run.
- Runnable in the workflow: the deterministic step runs inside the gate workflow through the
  `conformance` gate. Its name is `toolchain.commands.conformance`, and the config entry
  (`agentic.config.json:61` `"argv": ["python3", "scripts/run-conformance-gate.py"],`) is its argv.
  The gate's prose file set is the config key `gates.conformance.files`.
  The wrapper fails only on new drift against `HEAD`, so the repository's pre-existing findings never
  make it red (`scripts/run-conformance-gate.py:2` `fail on NEW drift only.`).

