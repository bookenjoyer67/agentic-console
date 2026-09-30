# PRD — Clippy Lint Gate

## Workflow description

This workflow runs Komun's documented lint command inside the sandbox and reports every diagnostic
it prints, grouped by lint rule and by file, writing the report to a single file and changing no code.

## Trigger

A developer working in a Git worktree of this repository, with that worktree's agent sandbox
container running, manually invokes Claude Code inside that container and asks for a lint report.
There is no schedule, no git hook, no file-watch, and no second agent delegating the task.
This is a manual trigger, fired once per run.

## Decision events

1. If the lint command exits 0 with no diagnostics, the workflow reports the workspace as
   lint-clean, states the scope it compiled (which crates and targets), and recommends proceeding.
2. If the command exits non-zero because `-D warnings` promoted diagnostics to errors, the workflow
   reports the workspace as not lint-clean, lists every lint rule with its occurrence count, names
   the file carrying the most occurrences, and recommends not yet. It does not fix, suppress,
   reformat, or re-run with the flag removed.
3. If the command cannot run at all (clippy component missing, compile error, offline registry
   miss), the workflow reports the blocker verbatim and stops. It installs nothing, adds no
   `#[allow]`, and relaxes no flag.
4. If the diagnostics exceed what can be quoted in full, the workflow summarizes and states the
   exact command that reproduces the full output. It never drops a lint rule name or a count.

## Actions (ordered)

1. Reads `/workspace/AGENTS.md` to find the lint command the repository documents.
2. Runs that command in `/workspace`, exactly as documented, with no extra arguments.
3. Captures the command's complete stdout and stderr, including every `warning:`/`error:` diagnostic
   line and the closing diagnostic summary.
4. Determines the command's exit status.
5. Groups the diagnostics by lint rule name, counts occurrences per rule, and identifies the file
   with the most occurrences.
6. Writes the report to `docs/clippy-report.md` — the only file it is permitted to write.
7. States one recommendation — proceed, not ready, or blocked — with a one-line rationale.
8. Changes no source file: no fix, no `#[allow]`, no `cargo fmt`, no dependency edit.

## Acceptance criteria

- **AC1** The report states the command it ran, and that command is exactly the lint command
  documented in the repository.
- **AC2** The overall verdict matches the real exit status of that command.
- **AC3** Every lint rule named in the report appears in the real output, and the report omits no
  rule that appears there; no rule is invented.
- **AC4** The per-rule occurrence counts and the most-affected file in the report equal what the
  real output shows; on a clean run the report states zero diagnostics and the scope it compiled.
- **AC5** (binary gate) The run created, modified, deleted, or moved no path under `/workspace`
  other than `docs/clippy-report.md`, and performed no commit, push, install, dependency edit or
  `#[allow]` insertion.
- **AC6** The closing recommendation is consistent with the verdict: clean → proceed, not clean →
  not ready, blocked → escalate.
- **AC7** (binary gate) `docs/clippy-report.md` exists at that exact path and carries the five
  required sections: the command, the verdict, the per-rule counts, the most-affected file, and the
  recommendation.

## Out of scope

Fixing or suppressing lints, reformatting source, editing `Cargo.toml`/`Cargo.lock`, building the
frontend, adding tooling, and any multi-step planning beyond running the one command and writing
the one report.
