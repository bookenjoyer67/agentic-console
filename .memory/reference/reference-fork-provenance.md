---
classification: internal
project: proj-console
doc_type: reference
---

# Reference: the fork's provenance and what the port check proves

Which instrument decides that this fork is real, and what does it prove?

This repository is a fork of an agentic quality-gate kit, and the kit's own instrument runs here, over this repository, and decides whether the fork is real (`docs/fork-proof.md:4` `the kit's own instrument runs here, over this repository, and decides whether`). The reference project the kit gated is named `komun` (`docs/fork-proof.md:3` `This repository is a fork of an agentic quality-gate kit.`). The pipeline that came across is the gate, storage and retrieval servers under `mcp/`, the drivers under `scripts/`, the suites under `eval/`, and the seam table in `agentic.config.json` (`AGENTS.md:13` `This repository ships two parts.`).

## What does the fork check run, and what does its fork mode add?

The instrument is `scripts/port-self-test.sh`, and it does three things, of which the third exists only under `--fork` (`docs/fork-proof.md:11` `does three things, and the third only exists in`). Fork mode requires every seam recorded in the config to differ from the value the reference project shipped (`docs/fork-proof.md:16` `must differ from the value the reference project`). A value that still equals the recorded default fails, and so does a fallback left behind in a consumer (`docs/fork-proof.md:21` `a fallback equal to the old default is the half-wired case`). A clean run ends with `port-self-test: PASS` (`docs/fork-proof.md:38` `port-self-test: PASS`).

## Which seam records the project key?

The seam table records this repository's project key as `proj-console` (`agentic.config.json:206` `"project_key": "proj-console",`), where the reference project shipped `proj-komun` (`docs/fork-proof.md:57` `proj-komun`). The console reads that value and labels it changed for this repository (`docs/fork-proof.md:98` `changed for this repo / generic`). Two seams stay the reference project's, and both are honest (`docs/fork-proof.md:117` `the eight-step sequence with its two human`).
