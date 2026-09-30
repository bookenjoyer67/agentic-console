---
classification: internal
project: proj-console
doc_type: runbook
---

# Runbook: the eight-step orchestration and its two human checkpoints

Which steps does an orchestrated run take, and where must a person approve?

The pipeline is declared as data, not written into code: the lanes and steps the console draws come from the config's own `orchestration_steps` (`docs/config.md:23` `the pipeline as data: the lanes and steps FLOW draws`). The sequence has eight steps, and two of them are human checkpoints (`docs/fork-proof.md:117` `the eight-step sequence with its two human`).

## What are the eight steps, in order?

The steps are declared in order: the project manager opens the ticket, the planner plans, human checkpoint 1 approves the plan, the implementer writes, the tester runs the gates, the reviewer reads the journal and records a verdict, human checkpoint 2 approves the release, and the project manager closes (`agentic.config.json:306` `"HUMAN CHECKPOINT 1 (plan approval)"`). Each step declares its kind beside its label, so a role step names a role and a checkpoint names `human` (`agentic.config.json:314` `"HUMAN CHECKPOINT 2 (release approval)"`).

## Why does this repository keep the reference project's sequence?

This repository keeps the eight-step sequence deliberately, because the pipeline is the same pipeline (`docs/fork-proof.md:118` `which this repository keeps deliberately: the pipeline is the same pipeline.`). The console lists that key among the ones it still carries at the reference project's default (`agentic.config.json:321` `"orchestration_steps"`), and its status line reports eight steps for it (`docs/fork-proof.md:114` `8 steps`).

## Which jobs does the FLOW map draw for a pull request?

The config's job list names five jobs with their `needs` chains (`docs/fork-proof.md:63` `five jobs`). Lane A draws a pull request arriving, with the CI jobs parsed from the workflow file and their `needs` chain, and which of them can block a merge (`docs/screens.md:13` `the CI jobs parsed from the workflow file`).
