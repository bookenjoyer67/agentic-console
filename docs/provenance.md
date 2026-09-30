# Provenance

The console's hardest job is deciding whether a run is waiting for a person. Get it wrong in one direction
and an operator writes a ruling into a conversation that was still working. Get it wrong in the other and a
stopped run sits there for an hour with nobody told.

The rule underneath every decision here: **the checkpoint card names a checkpoint only from evidence that
belongs to that run, and it prints the evidence it used.**

## Two facts, kept apart

The card reads two independent things, and it never fuses them:

| fact | how it is read |
| --- | --- |
| does the evidence **name** a checkpoint | the run's own prompt, or its own transcript, or the journal's shape |
| is a `claude` process **running** | a read-only `docker exec <container> ps` |

A run is headless and one invocation per phase. At a human checkpoint the orchestrator ends its turn and the
CLI process exits. The next phase is a separate invocation. So a process in flight means the run is working,
and the waiting happens after the process is gone.

## The truth table

| evidence names a checkpoint | a process is running | what the card shows |
| --- | --- | --- |
| yes | yes | `RUN IN FLIGHT` — the checkpoint lies ahead of the run, so the ruling is refused |
| yes, one read names it | no | `STOPPED AT <checkpoint> -- A RULING RESUMES IT` — the actionable state |
| yes, two reads name different checkpoints | no | `STOPPED AT <checkpoint> -- THE EVIDENCE DISAGREES: NO RULING OFFERED` — `e` does not open, and `--dump` prints `contested` until the disagreement is read |
| no | yes | `RUN IN FLIGHT` — work in progress; no checkpoint is named |
| no | no | `NO RUN, NO CHECKPOINT` — no ruling is offered, because no session is named to resume |

The ruling is offered in exactly one state: a checkpoint is named by one read, and no process is running.

## Evidence, in the order the card reads it

1. **The prompt of a run in flight** — the `-p` argument of the agent process. Only a live process has one,
   and a live process means the run is working, so this read names the checkpoint the run is *heading for*,
   never one waiting on the operator.
2. **The named session's own transcript inside the container** — `<console.session_dir>/<session-id>.jsonl`,
   read from its tail. This is the run's own words about where it halted, and with the process gone it settles
   the checkpoint on its own.
3. **A transcript in `console.evidence_dir` that is attributable to that same session** — its file name
   carries the session id, or its metadata block names the session. A transcript whose name carries no session
   id is **ignored**: being the newest file in the directory is not attribution. The card prints the directory
   it looked in, how many run transcripts were there, how many carried a session id, and which it ignored.
4. **The storage journal's shape** — the newest record is a plan with no implementer record after it
   (checkpoint 1), or a reviewer's verdict with no close after it (checkpoint 2), and that record is inside
   `console.checkpoint_fresh_minutes` of the reading.

Evidence 4 is an inference, and the card marks it as one. It **corroborates and never overrules**: where it
names a different checkpoint from the run's own words, the card shows the disagreement and offers no ruling
rather than quietly preferring a source.

Every line on the card carries its own source and its own age.

## A ruling resumes one named session

A ruling is sent as:

    docker exec -w <workspace> <container> claude --resume <session-id> -p "<ruling>" <flags>

Two properties make that safe:

- **`--resume <session-id>` names the conversation.** `--continue` resumes whichever session is newest, which
  with two runs in one container is somebody else's. The console resolves the session from its own reads, and
  an action that cannot say which conversation it resumes is not offered at all.
- **The ruling is one argv element.** No shell re-parses it, so a ruling containing quotes, backticks or
  newlines arrives intact. This is why the console builds an argv and not a command string.

The flags come from `console.claude_flags` with `--agent orchestrator` filtered out, because the session
already carries the agent forward.

## Refusals name the hazard

When the card refuses a ruling it says why, and the same state is printed in the status bar, on the
confirmation screen, in `--dry-run-actions`'s `guards` line and in `--dump`. The refusal for a run in flight
names the hazard directly: a second process on the same session jsonl is a second writer on that
conversation.

## Known gap

The RUN panel reads the container's process table, and with two orchestrated runs in one container it reports
the first and omits the second. Attribution on the checkpoint card stays correct — it names one session or
refuses — but the panel does not yet list every in-flight process. This is recorded in
[`docs/roadmap.md`](roadmap.md) as task T1.9, and it came out of an experiment that ran two runs at once.
