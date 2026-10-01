# Architecture

The console is a window, not a controller. It reads what a repository's agentic run left behind and draws
it. It holds no state of its own about the run, and it never guesses what a file would say if it could read
it.

## The one rule

**Every reading carries its source and its age, or it is not shown.**

Three rules follow from that, and the tests hold them:

- A reading records the moment of the *read*, never the moment of the collect that served it. A probe that
  succeeded four seconds ago is four seconds old even when it arrives from a snapshot taken just now.
- A failed read stays failed. The console does not substitute a default, a zero, or the last good value. A
  missing file reads as a missing file, with the error beside it.
- Nothing is invented. Where the console cannot read something, it says so on the panel that wanted it.

The probes read a repository, a container, a process table, a journal and a session directory. All of them
are read-only. `--dry-run-actions` prints every command an action would run and stops.

## Threading

One worker thread owns every probe. The UI thread renders the most recent snapshot it was handed and never
waits on a read.

    UI thread                          probe worker ("agentic-console-probes")
    ─────────                          ───────────────────────────────────────
    draw last snapshot  ─────────┐
    keystroke ──────────────────┼──►   channel: request a snapshot
                                │      run the probes whose TTL has expired
    receive snapshot    ◄───────┘      channel: hand back the snapshot

This is the difference between a console that feels alive and one that stalls. Measured on the reference
repository:

| what | before | after |
| --- | --- | --- |
| one synchronous collect, median | 1.907 s | 0.084 s steady state |
| probing done on the UI thread | all of it | none |
| keystroke to frame | 0.95 – 1.48 s | 0.002 – 0.008 s |

The remaining 0.002 s is the floor set by `tmux capture-pane` polling, not by the console.

## The cache

Each probe is a `Cached<T>` with its own TTL in `src/cache.rs`. The table lives in the seam table under
`console_probe_cadence`, so an operator tunes freshness without a rebuild:

| key | meaning |
| --- | --- |
| `refresh_seconds` | how often a snapshot is taken |
| `stale_after_seconds` | the age at which the panel labels its reading **STALE** |
| `default_ttl_seconds` | the TTL of any probe the `probes` table does not name |
| `probes.gate_list` | the expensive one: it asks the running gate server for its own gate list |

`gate_list` is asked far less often than the file stats, because it spawns `python3` inside the container and
imports the gate server's own module — on the order of a second and a half. `r` forces every probe
regardless of the table.

## Modules

| file | what it is |
| --- | --- |
| `src/main.rs` | flags, config resolution, then the app |
| `src/app.rs` | the event loop, the mode stack, the refresh path |
| `src/state.rs` | the observed state: every reading, every cached probe, every derived card |
| `src/collector.rs` | the probe worker thread and its two channels |
| `src/cache.rs` | `Cached<T>`: a value, the moment it was read, its TTL, and its error |
| `src/probe.rs` | the read-only probes: config seams, processes, ports, sessions, journals |
| `src/checkpoint.rs` | the truth table: what state a run is in, and whether a ruling is offered |
| `src/journal.rs` | the storage journal and the gate journal, parsed |
| `src/pipeline.rs` | the orchestration steps, as the config declares them |
| `src/actions.rs` | the seven actions: the exact command each one runs, and its dry-run form |
| `src/config.rs` | the seam table, as this crate reads it |
| `src/dump.rs` | `--dump`: the whole state as JSON, for a script or a test |
| `src/iso.rs` | the age and staleness arithmetic, in one place |
| `src/timings.rs` | `--probe-timings`: what each probe costs |
| `src/ui/mod.rs` | the frame, the panes, the key reference |
| `src/ui/flow.rs` | the pipeline map, with live lights |
| `src/ui/live.rs` | what is happening now: runs, journal rows, ports |
| `src/ui/inspect.rs` | the machinery: seams, role mounts, grants, gates |
| `src/ui/style.rs` | the width doctrine: truncation, ellipsis, gutters |

## The action layer

An action never runs for you. It builds the exact argv, prints it, and waits for one confirmation key. Two
properties matter:

- The command shown is the command run. The preview is the argv, not a description of the argv.
- An unknown name is refused before anything starts. A gate name outside the allowlist, or a role outside
  `roles.valid`, stops at the preview.

Read [`docs/config.md`](config.md) for the keys an action reads, and [`docs/provenance.md`](provenance.md)
for the rules that decide when a ruling is offered at all.

## The conversation

The one thing the console owns is its transcript, and it owns it in memory. Everything else on every
screen is a reading: a file, a `docker` call, a command's output, each carrying its age. The transcript
is the console's own buffer, and it is labelled as such -- `TRANSCRIPT -- this console's own buffer, not
a reading` -- until a line's identity is found in the run's own record.

Each item carries **two** identities, and they are not the same thing:

- `key` makes the buffer unique. A streaming turn emits the same message more than once as it fills in,
  and a tool's call and its result arrive with the same `tool_use` id; the call is keyed `call:<id>` so
  its result can be a distinct line rather than a collision.
- `uuid` is the run's own identity, and it is what reconciliation uses. The record the run writes for
  itself -- the session `.jsonl` the agent CLI keeps in the container -- carries the same uuid on its
  assistant records. Finding the uuid there is what moves a line from LIVE to CONFIRMED, and it is the
  only thing that does: the console does not mark its own guesses as confirmed.

A transcript line is therefore in one of four states: this console's own words, live (the run's, not yet
found in its record), confirmed (found), or failed. A dropped line -- the ring holds the last
`DEFAULT_RING` (400) items -- is *counted* and announced, because a ring that silently forgets is a
transcript that lies by omission.

Two parser rules are worth naming, both measured rather than assumed
([`docs/spikes/prompt-surface-protocol.md`](spikes/prompt-surface-protocol.md)):

- A second `init` envelope updates the session and model fields and **adds no items**. A parser that
  treats `init` as "a session started here" resets the transcript on every resumed turn.
- A turn stopped by the account's rate limit still reports `"subtype":"success"`. The failure is in
  `is_error` and `terminal_reason`. Anything keying on `subtype` draws a rate-limited turn as green.

Driving a turn has three rules of its own. One writer per session: a second turn on a session already in
flight is refused, naming the process, its elapsed time and its short id. The first turn mints the id with
`--session-id`, and the confirmation shows that exact id, so the id on the screen is the id in the argv;
later turns carry `--resume` and that same id. And the console never claims to have started a turn it did
not start: the argv line, the turn and the session the run reports are written into the transcript only
after the spawn succeeded.

### Reconciliation

A line moves from LIVE to CONFIRMED by one route and no other: the run's own session record is read back
out of the container, and an item whose `uuid` appears there is confirmed. The read is a probe of its own
(`conversation_transcript`), keyed by the session the run itself reported, taken on the worker because
every container read belongs there. The session id is not discoverable on the worker -- only the console
sees the run's `init` envelope -- so it travels with the request, and a turn that has just ended forces a
collect so its last lines are not left LIVE while the interval ticks by.

The reasoning lives in one function, `app::reconcile_read`, shared by the interactive console and the
one-shot `--prompt` path, so the two cannot describe the same read differently. Three ways it could lie,
and what each branch does instead:

- **a read that names another session** is not an answer about this run, so it confirms nothing and the
  refusal names the session it did read;
- **a read that failed** confirms nothing and says what failed. Silence here is the worse failure: every
  line would sit LIVE and the operator would read that as a run that had written nothing;
- **a bounded tail that matched nothing** reports the window it read. `tail -c N` answers with the whole
  file when the file is smaller than N, so the size of what came back is what says whether the read could
  see the whole record. Calling a line absent from a record the read never reached is the one thing this
  must not do, and it is why the model function `Conversation::confirm_from` writes no prose: it cannot
  know which of the two it was.

## Tests

133 tests: 71 frame tests, 8 refresh tests, and 54 unit tests.

    cargo test --release --offline

The frame tests render the console at fixed sizes and compare against the drawn output. They render at the
six sizes the design targets -- 80x24, 86x38, 100x30, 120x40, 160x50 and 230x60 -- so a green suite covers
the narrow terminals, not only the wide ones. An overlay is rendered *over* a conversation that has content
in it, because an overlay drawn over an empty backend proves nothing about what does or does not show
through it.
