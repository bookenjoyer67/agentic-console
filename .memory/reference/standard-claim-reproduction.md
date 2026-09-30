---
classification: internal
project: proj-console
doc_type: standard
---

# Standard: how a measured count is settled before it is written down

What makes a count a claim, and what must sit beside it before it is reported?

A claim asserts something about the repository, and every claim needs authority (`docs/DOC-STYLE.md:15` `needs authority (R2). A sentence that only describes the document itself is not a claim.`). A bare location is not enough: naming an artifact without quoting it fails the rule (`docs/DOC-STYLE.md:40` `A bare location is a v1 form and fails R2 in v2`). The acceptable authority forms are three, and the first is a location with the text at it (`docs/DOC-STYLE.md:36` `a location and the text at it`).

## How is a command's own result cited?

A reported count is written beside the command that produced it, so a reader can re-run it and compare (`docs/DOC-STYLE.md:37` `a command and its output`). The repository's guide uses that form for both suites: the console's tests are recorded as `cargo test --release` -> `52 passed, 0 failed` (`AGENTS.md:147` `52 passed, 0 failed`), and the pipeline's suites as a pytest run -> `90 passed` (`AGENTS.md:146` `90 passed`). The conformance gate is recorded as `verdict pass, 12 files checked` (`docs/fork-proof.md:152` `verdict pass, 12 files checked`).

## What happens to a count nobody can reproduce?

Do not delete the claim, and do not invent authority for it (`docs/DOC-STYLE.md:72` `Do not delete the claim, and do not invent authority for it.`). Mark it and list it, naming the artifact that would settle it. A revision is a new version with a dated entry, never an in-place edit (`docs/DOC-STYLE.md:5` `version number plus a dated entry in the history at the bottom`).

## Which instrument measures this fork, and what is never done to a check?

The fork check is one of four instruments that run in this repository, and each prints its own verdict (`docs/fork-proof.md:149` `the fork check`). No check and no test is ever weakened to reach a pass (`AGENTS.md:72` `Never weaken a check or a test to reach a pass.`).
