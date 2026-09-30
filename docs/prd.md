# PRD — Workspace Test Gate

## Workflow description

This workflow runs Komun's documented workspace test command inside the sandbox and reports
whether the workspace's tests pass, naming every failure exactly as the test runner printed it.

## Trigger

A developer working from the repo root on the host, with the agent sandbox container running,
manually invokes Claude Code inside that container and asks it to run the project's tests.
There is no schedule, no git hook, no file-watch, and no other agent delegating the task.
This is a manual trigger, fired once per run.

## Decision events

1. If the test command exits 0, the workflow reports the workspace as green, gives the per-crate
   pass counts, and recommends proceeding.
2. If the test command exits non-zero, the workflow reports the workspace as not green, names every
   failing test with the assertion text the runner printed, and names the crate to inspect first.
   It does not attempt a repair.
3. If the test command cannot run at all (compile error, missing toolchain, blocked network), the
   workflow reports the blocker verbatim and stops. It does not install anything, does not disable
   or skip tests, and does not work around the blocker.
4. If the runner's output is longer than can be quoted in full, the workflow summarizes and states
   the exact command that reproduces the full output. It never drops the name of a failing test.

## Actions (ordered)

1. Reads `/workspace/AGENTS.md` to find the test command the repo documents.
2. Runs that command in `/workspace`, exactly as documented, with no extra arguments.
3. Captures the command's complete stdout and stderr, including every `test result:` line.
4. Determines the command's exit status.
5. Summarizes: tests passed and failed per crate, and each failing test by full path with its
   assertion message.
6. States one recommendation — proceed, not ready, or blocked — with a one-line rationale.
7. Writes nothing to the filesystem.

## Acceptance criteria

- **AC1** The summary states the command it ran, and that command is exactly the test command
  documented in the repository.
- **AC2** The overall verdict matches the real exit status of that command.
- **AC3** Every failing test named in the summary appears in the real runner output, and the
  summary omits no failing test that appears there.
- **AC4** The per-crate passed/failed counts in the summary equal the counts on the real
  `test result:` lines.
- **AC5** (binary gate) The run created, modified, deleted, or moved no file under `/workspace`,
  and performed no commit, push, install, or deploy.
- **AC6** The closing recommendation is consistent with the verdict: green → proceed,
  not green → not ready, blocked → escalate.

## Out of scope

Fixing failures, editing or skipping tests, installing tooling or crates, building the frontend,
and any multi-step planning beyond running the one command and reporting its result.
