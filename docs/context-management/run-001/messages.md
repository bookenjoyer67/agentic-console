# Boundary messages and operator log — run 001

The graded managed session ran 2026-09-26 in the `agent-rev` sandbox container
(`~/rev/sandbox/run-agent.sh`, image `agent-sandbox:komun`, internal network, credential broker,
`/workspace` = `~/rev`), driven by the operator over a pty:

```
docker exec -it -w /workspace agent-rev claude --model opus \
  --agent komun-docs-stylist --permission-mode acceptEdits
```

Store of record for what was actually sent and returned: the session transcript, copied out of the
container to `~/komun-agent-exercise-2-2/transcript-run-001.jsonl` (session
`aea13263-d840-4dc6-81d5-4f1e413661a9`). This file records the operator-side messages, the timings and
the environment events that the transcript alone does not explain.

## Message sequence

| # | Phase | Message (operator) | Agent turn time |
|:--|:--|:--|:--|
| 1 | A | Task + phase-A scope: standard at `docs/DOC-STYLE.md`, `AGENTS.md` "What this is" only, R1–R5 in effect, restate goal and rules before editing | 3m 04s |
| 2 | A | "Critical rules" section; same rules; other sections byte-identical | 3m 21s (ended in two rule questions, no edit) |
| 3 | A | Operator rulings on Q1 and Q2 (below); apply the whole section in one pass | 2m 51s |
| 4 | boundary | `/summarize-session` — the planned proactive summary, fired before any new rule arrived | 1m 05s |
| 5 | boundary | Summary confirmed as accurate after host-side verification | n/a |
| 6 | B | **Boundary 1 preamble** + the requirement change + "revise `docs/DOC-STYLE.md` to v2, then apply v2 to `Prerequisites`" | 3m 50s |
| 7 | B | "Build order (critical)" under v2 | 2m 31s |
| 8 | C | **Boundary 2 preamble** + revisit the phase-A sections under v2 | 3m 40s |
| 9 | C | Consistency pass over all four sections; list every violation with its rule, then fix; closing report | 8m 10s (includes a 4m 40s API retry stall) |

Wall clock 14:46:43 → 15:31:56 CDT (45m 13s); model time ≈ 28m 32s of which ~4m 40s was client
back-off, so ~23m 52s of actual generation. Operator-side verification after the session: ~4 minutes of
scripted checks (citation re-execution, rule measurement, containment).

## Boundary 1, as sent (phase A -> phase B, the requirement change)

> Phase A is complete: the "What this is" and "Critical rules" sections of AGENTS.md are edited under
> documentation standard v1. Phase B focuses on docs/DEVELOPMENT.md, sections "Prerequisites" and
> "Build order (critical)". Rules still in effect: R2 (authority parenthetical), R3 (sentence limit),
> R4 (imperatives, no hedging). Rules changed, effective immediately: R3's limit is now 35 words, not
> 25. R1 is replaced — a section opens with the question it answers, and existing "This section ..."
> purpose sentences are removed. R2 is strengthened — the parenthetical must name the artifact AND the
> literal text, value or count that settles the claim. R5 is withdrawn — nesting is allowed where it
> shows real hierarchy. The Q1 and Q2 rulings still apply, with the question-opening rule replacing
> R1's sentence. What still matters from phase A: the sections you edited now lead with purpose
> sentences that are no longer wanted; they are revisited in phase C, not now.

## Boundary 2, as sent (phase B -> phase C, the revisit)

> Phase B is complete: docs/DOC-STYLE.md is at v2 and both named sections of docs/DEVELOPMENT.md are
> edited under it. Phase C focuses on the two phase-A sections of AGENTS.md — "What this is" and
> "Critical rules" — revisiting them under the current rules. Rules in effect: v2 only … What still
> matters from phase A: those sections lead with v1 purpose sentences that v2 deletes, and they carry
> v1 bare-location parentheticals that v2's R2 fails. Re-read both sections from the file before
> editing them; do not work from your memory of them.

## The operator rulings (message 3)

The agent stopped before the second section and refused to guess two genuinely under-specified rules.
Both rulings were given in the session and are carried in the phase-A summary:

- **Q1 (R1's reach):** Reading B — every heading opens a section, so the six `###` subsections each get
  their own opening sentence.
- **Q2 (R4's reach):** R4 governs bullets that state a rule; "**Never log** …" counts as an imperative
  and stays as written; fact-shaped bullets keep their shape and gain R2 authority instead. R4 also
  governs rules written in prose.

## Environment events worth recording

- **API back-off.** During the final consistency pass the client reported "Waiting for API response ·
  will retry in 4m 40s · check your network" and then resumed unaided; the broker log shows only
  successful upstream calls either side of the gap, and `agent-rev` stayed `Up` throughout. It is a
  harness limitation, not a context-management event, and it is counted in the cycle-time figure above.
- **Pre-flight probes (not part of the graded run).** Two headless calls before the session: one
  proving `CLAUDE.md` is loaded and its five-step procedure is recited back, one proving
  `komun-docs-stylist` resolves and describes its own scope. They created their own transcripts
  (`0a504a51…`, `edaeff89…`), which are separate from the graded session's.
- **Transport detail.** The pty accepts the message text via `write`; the submit needs a separate
  carriage return. Recorded because it is the kind of thing that silently wastes a turn next time.
