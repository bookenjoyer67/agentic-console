# Roadmap

What is being built next, and the defects each piece fixes. The measured starting point: the design target is
80 to 100 columns, and the frame tests rendered at 230x60 and 200x50, so no test ever exercised the narrow
case an operator actually has.

## Phase 1 — legibility, no new features

| task | defect it fixes |
| --- | --- |
| T1.1 | a shape function with a size-aware minimum, so a panel is never drawn smaller than it can be read |
| T1.2 | a row budget per panel, and loud dropping — content that will not fit is announced, never silently cut |
| T1.3 | the FLOW lane stacks instead of compressing when the terminal is narrow |
| T1.4 | the LIVE panels stack the same way |
| T1.5 | INSPECT stops vanishing below its current minimum width |
| T1.6 | a header that fits, including the reading's age |
| T1.7 | consistent gutters, ellipsis over mid-word cuts, and path wrapping that keeps the tail |
| T1.8 | the size matrix: frame tests at 80x24, 86x38, 100x30, 120x40, 160x50 and 230x60 |
| T1.9 | the RUN panel lists every process in flight, not only the first |

T1.9 came out of an experiment that ran two orchestrated runs in one container. Attribution on the checkpoint
card stayed correct — it named one session or refused — while the RUN panel showed one process and never
mentioned the other. The card keeps only `prompt_chars`, so the prompt's own words are the new datum this task
needs.

Phase 1 keeps the existing suite unedited. Every frame test asserts a rendering, and an assertion edited to fit
a change is no longer a regression test.

## Phase 2 — the composer, one-shot

A prompt surface for the orchestrator. Compose one message, see the exact argv it becomes, confirm it, send it
once. The console already has the pieces — the argv builder, the confirmation, the action log — so this phase
is a text field with a provenance rule, not a new subsystem.

It starts with the orchestrator only. Other roles are reachable through the role-box action, and the composer
follows the same rule the checkpoint card follows: it names who it is talking to, and it refuses when it
cannot.

## Phase 3 — the conversation pane

A pane that shows the run's own transcript, read through the same probe layer as every other reading, with
`--resume` to continue it. This is where a console stops being a dashboard and becomes the place the work is
driven from. It depends on Phase 2's composer, because a transcript you cannot answer is a log.

## Phase 4 — templates and polish

Prompt templates in the seam table, so a fork's common instructions are data rather than typing. Then the
smaller things: overlay placement at narrow widths, the action log's own scrolling, and the key reference
growing a section per screen.

## Decisions already taken

| question | decision |
| --- | --- |
| the primary target | 80 to 100 columns, widening gracefully above 160 |
| the prompt surface | both, phased: one-shot first, then the transcript with resume |
| who it addresses first | the orchestrator only |
| the layout model | one primary panel plus overlays |
| templates | config-driven |

## How a change lands here

The console's own gate is three commands, and the same three run in CI:

    cargo fmt --check
    cargo clippy --release --all-targets -- -D warnings
    cargo test --release --offline

The 52 tests are the floor, not the target: a change that alters a rendering adds a test at the width it
altered, which is exactly the discipline the narrow-width defects were missing.
