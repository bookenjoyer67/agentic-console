# Calibration Log

Version: v1.0.0
Last updated: 2026-09-28

## What this log records

Which events does this log turn into governance?

Record every near-miss beside the iteration-log line that evidences it, so a Module 4 denial traces to an observed event (`docs/iteration-log.md:3` `Every run of a workflow in this repo gets an entry below`).
Draw the patterns below from the log's top five entries, Run 001 to Run 005, and the Module 3 work (`docs/iteration-log.md:267` `Run 005 (workflow 6`).
Name each pattern, so the governance policy can cite the denial it argues for (`docs/calibration-log.md` this section's `NM-1` to `NM-10`).

## Near-miss patterns for Module 4 governance

Which near-misses does Module 4 calibrate against, and what does each one risk?

Ten patterns follow, each named and each cited to the line that evidences it (`docs/iteration-log.md:1` `Iteration Log`).

### NM-1 — Inert grant

Which near-miss let a granted tool satisfy no gate?

Risk: a role granted an inert tool reports blocked gates rather than failed ones, so the run holds no gate evidence (`docs/iteration-log.md:305-306` `the course's deliberately inert stub`).

### NM-2 — Starved role

Which near-miss left a planning role unable to read its own inputs?

Risk: a planning role denied its reads plans from a ten-line brief and cannot ground the plan it returns (`docs/iteration-log.md:334-335` `its plan rested entirely on the ten-line quotation`).

### NM-3 — Union permissions

Which near-miss showed confinement resting on a single file?

Risk: a permission union lets a role call another role's tool, so confinement rests on each definition's denied list (`docs/iteration-log.md:362` `allows the union of every role's tools`).

### NM-4 — Self-approved provenance

Which near-miss put an approval in the approver's own voice?

Risk: a checkpoint record written by the role it judges borrows the appearance of an independent approval (`docs/iteration-log.md:375-376` `the same role whose verdict it approves`).

### NM-5 — Credential inside memory reach

Which near-miss brought a credential within reach of a committable file?

Risk: a write path open to any classification can persist a credential into files meant to be committed (`docs/iteration-log.md:494` `sk-ant-…`).

### NM-6 — Borrowed memory mount

Which near-miss let another project's rules answer for this one?

Risk: memory mounted from another project looks identical to the right memory, so a role acts on foreign rules (`docs/iteration-log.md:489-490` `SCOPE.md declares project-b; the workspace is Komun`).

### NM-7 — Unevidenced closure

Which near-miss closed a change with gates never executed?

Risk: a run can close a change with two gates unrun and record the gap as a known limitation (`docs/iteration-log.md:400` `not run: crates/wasm/pkg/ is absent`).

### NM-8 — Unreproducible self-report

Which near-miss showed a report's own numbers as no evidence?

Risk: a role's self-reported count can drift while the artifact stays correct (`docs/iteration-log.md:625` `the closing report's self-reported edit count does not reproduce`).

### NM-9 — Permissions that do not bind the process

Which near-miss showed a permission bit stopping nothing?

Risk: a filesystem permission is not a guardrail when the writing process runs as root (`docs/iteration-log.md:552` `root ignores those bits`).

### NM-10 — Wrapped-pointer false positive

Which near-miss made the checker report drift on a correct citation?

Risk: a pointer whose quoted literal wraps to the next line pairs with a neighbouring literal, so a correct citation reads as drift (`eval/test_deterministic_step.py:222` `def test_limitation_wrapped_literal_pairs_with_a_neighbour`).

## Governance controls these near-misses argue for

Which control does each near-miss buy?

- Bind gate execution to the tester alone, because `NM-1` showed an inert grant blocking every gate (`docs/routing-and-tool-grant-map.md:70` `keep the gate server the one path that executes a command`).
- Grant reads to the roles whose work needs them, because `NM-2` showed a starved planner (`docs/routing-and-tool-grant-map.md:16` `mcp__coursetools__codebase_search`).
- Enumerate denials in every definition, because `NM-3` showed confinement inferred from a union (`docs/iteration-log.md:367` `enumerate the denied MCP tools in orchestrator.md`).
- Give every checkpoint record one author, because `NM-4` showed a self-approved checkpoint (`docs/iteration-log.md:386` `assign checkpoint records to the project-manager`).
- Cap every write at `internal`, because `NM-5` showed a credential inside memory reach (`docs/memory-architecture.md:151` `Must never appear in any memory file.`).
- Scope every memory mount to this project, because `NM-6` showed borrowed memory answering for this repository (`docs/memory-architecture.md:138` `Identifies which project owns the mounted memory directory`).
- Require one verdict per gate and no `Done` status without evidence, because `NM-7` showed an unevidenced closure (`.claude/agents/project-manager.md:53` `Every gate passed`).
- Require the literal output behind every number, because `NM-8` showed a self-report that does not reproduce (`docs/iteration-log.md:684` `no claim may rest on an earlier session's record`).
- Enforce the read-only memory layers with a hard stop, because `NM-9` showed permission bits a root process ignores (`docs/memory-architecture.md:196` `did not stop a root write`).

## Before and after the deterministic conversion

Which step does this entry measure, and what did each side of the conversion cost?

The step is the prose and citation conformance check (`docs/step-classification.md:43` `Step: prose and citation conformance check — CONVERTED`). The decision of record is `docs/adr/ADR-001-doc-conformance-deterministic-conversion.md`, and both sides below are measured runs rather than projections.

### Before conversion — the agentic run of 2026-09-26

What did one agentic pass cost, and what did it score?

- Cycle time: 45m13s of wall clock for nine operator messages (`docs/iteration-log.md:639` `45m13s wall`).
- Token cost: 2,400 input and 164,438 output, plus 230,407 cache-write and 5,606,538 cache-read (`docs/iteration-log.md:645-646` `input 2,400 / output 164,438 / cache`).
- Model spend: $8.37 at the CLI's own accounting, over 80 distinct API requests (`docs/iteration-log.md:645` `$8.37`).
- Rubric score: 11 of 12, PASS (`docs/iteration-log.md:628` `| **Total** | **11 / 12**`).
- Host-side review beside the run: ≈2.1 minutes in the longest cycle, scripted rather than spot-checked (`docs/iteration-log.md:757` `≈2.1 min`).

### After conversion — three runs of the script on 2026-09-29

What does the script cost, and does it repeat byte for byte?

- Command: `python3 scripts/validate_doc_conformance_deterministic.py --input <file> --output run<N>.json`, over the ten files named at the end of this section.
- Cycle time: 0.844s, 0.424s and 0.422s across the three runs, against 45m13s for the agentic pass (`docs/iteration-log.md:639` `45m13s wall`).
- Token cost: zero and no model call, because the script imports five standard modules alone (`scripts/validate_doc_conformance_deterministic.py:63` `import argparse`).
- Spend: $0.00, and the report carries no timestamp, so two runs over one input are byte-identical.
- Repeatability, diff-verified: one SHA-256 digest over all three reports, `15f1c690dbfdb0e1c19f78237836ce1669b47de47c176a98dfe56a943cc61af5`; `diff -q run1.json run2.json` and `diff -q run1.json run3.json` each print nothing and exit 0.
- Coverage of each run: 299 citations checked, 289 resolved at the cited line, 10 citation findings, and 72 pointers skipped for want of a literal beside them.
- Clean inputs exist in the same run: `docs/step-classification.md` and `docs/orchestration-diagram.md` return `0 violation(s)` each.
- **Known limitation, with its count.** Two of the ten findings are false positives from the
  wrapped-literal pairing mode, and neither citation was rewritten. `AGENTS.md:29` and `AGENTS.md:45`
  both cite `web/package.json`, whose intended literals sit one line below their pointers
  (`web/package.json:12` `"@sveltejs/adapter-static": "^3.0.0",`) and
  (`web/package.json:20` `"svelte": "^5.0.0",`).
- **The other eight findings are the same pairing mode.** They sit in `docs/policy-reconciliation.md`,
  where pointers are chained on one line, and two were hand-checked as correct citations
  (`docs/policy-reconciliation.md:37` `the reviewer edits nothing it reviews`) and
  (`.claude/agents/reviewer.md:16-17` `- mcp__gate__list_gates`).
- **The rule findings are pre-existing.** Every file this conversion edited holds the same rule-finding
  count after the edit as before it: `docs/step-classification.md` reports `0 violation(s)`,
  `docs/routing-and-tool-grant-map.md` reports `8 violation(s)`, and `docs/governance-policy.md`
  reports `1 violation(s)`. No rule finding here was introduced by the integration.

The ten inputs, in the order the command names them: `AGENTS.md`, `CLAUDE.md`, `docs/DOC-STYLE.md`, `docs/governance-policy.md`, `docs/routing-and-tool-grant-map.md`, `docs/policy-reconciliation.md`, `docs/step-classification.md`, `docs/iteration-log.md`, `docs/memory-architecture.md`, `docs/orchestration-diagram.md`. The three reports live outside the repository, so the digest above is the record.

## Which four runs closed the module 4.3 end-to-end regression?

What did each run cost, and what did its gate report?

| Run | Task id | Wall clock | Invocations, in seconds |
| --- | --- | --- | --- |
| D1 | `run-2026-09-29-calibration-nm` | `2323` s | `run2-d1-1` 634, `run2-d1-2` 900, `run3-d1-3` 474, `run3-d1-4` 315 |
| D2 | `run-2026-09-29-coursetools-path` | `1592` s | `run3-d2-1` 495, `run3-d2-2` 968, `run3-d2-3` 129 |
| H1 | `RUN-2026-09-29-schema-r1` | `1698` s | `run3-h1-1` 570, `run3-h1-2` 958, `run3-h1-3` 170 |
| H2 | `run-2026-09-29-untested-invariant` | `1438` s | `run3-h2-1` 531, `run3-h2-2` 762, `run3-h2-3` 145 |

- Read every wall-clock figure from the `wall_clock_s` field of the harness metadata block in the file that names it.
- Record one invocation that hit the old cap: `run2-d1-2.txt` exited 124 at the 900 s cap, before the harness moved to 1800 s with an in-container terminator.
- Record no cost figure. The broker reports no usage for these invocations, so cost is unmeasurable here and no estimate is offered.

### Did the converted step run in place of the retired subagent?

Which gate carried the step, and which role called it?

- Carry the step as the `conformance` gate, whose argv is the repository's own config entry (`agentic.config.json:61` `"argv": ["python3", "scripts/run-conformance-gate.py"],`).
- Call that gate from the `tester` alone, the one role the map grants `mcp__gate__run_gate`, with the converted step's own MCP access empty (`docs/routing-and-tool-grant-map.json:71` `"mcp_access": []`).
- Read four journal rows in `.memory/gate-audit.log` as `"gate": "conformance"`, `"exit_code": 0` and `"calling_role": "tester"`, at `17:48:56`, `18:35:29`, `21:07:44` and `21:35:22` on 2026-09-29.
- Spawn no `komun-docs-stylist` in any of the four runs. The four session transcripts list their subagent spawns in order, and that name is absent from every one.
- Draw that name's retirement from the map's converted-steps section, which routes the step to the gate instead of a subagent (`docs/routing-and-tool-grant-map.md:99` `- Run that step inside the workflow through the `conformance` gate`).

### What did the suites report after each run?

Which gates were green, and which ones stayed red or absent?

- **D1.** `policy` exit 0 (`90 passed`), `conformance` exit 0 (totals 155 to 148), `clippy` exit 0 with its guard satisfied, `test` exit 101 (156 passed, 2 failed), `fmt` exit 1.
- **D2.** `policy` exit 0 (`90 passed in 1.93s`) and `conformance` exit 0; `clippy` barred by design, because its cache-hit guard writes `crates/server/src/main.rs`.
- **H1.** `test` exit 0 (158 passed), `clippy` exit 0 with the guard applied and satisfied, `policy` exit 0, `conformance` exit 0, `fmt` exit 1.
- **H2.** `test` exit 0 (`komun-core` 21 passed, `komun-server` 138 passed), `clippy` exit 0 with the guard applied and satisfied, `policy` exit 0, `conformance` exit 0, `fmt` exit 1.
- Keep `fmt` red in all four runs, unattributed to either change and pre-existing at `HEAD`.
- Settle H1's rule through the gate's per-file row, which moved from base `{CIT 6, R1 8}` to current `{CIT 0, R1 0}` with `new_findings: []`.
- Leave two criteria unsettleable in-workflow, because no gate and no role holds a shell or a diff: H1's per-suite pytest split, and the changed-file scope of both new runs.
- Measure those open questions through the harness, which is not the run. The citation counts re-measured at 82 checked and 82 resolved (`python3 scripts/validate_doc_conformance_deterministic.py --input mcp/gate/SCHEMA.md` -> `0 violation(s), 82 citation(s) checked, 82 resolved at the cited line`).
- Measure the `fmt` question the same way. H2's `fmt` body named `crates/core/src/tests.rs` in four hunks byte-identical to `HEAD` (`rustfmt --check` on both -> `4` hunks, same line numbers, none touching the new test).
- Change exactly one content file per new run: `mcp/gate/SCHEMA.md` for H1 and `crates/core/src/tests.rs` for H2, measured on the host with `git status`.
