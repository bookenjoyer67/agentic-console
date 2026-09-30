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

## Tests

52 tests: 41 frame tests, 8 refresh tests, and 3 unit tests of the row-clipping rule.

    cargo test --release --offline

The frame tests render the console at fixed sizes and compare against the drawn output. They render wide
today — 230x60 and 200x50 — which is why the narrow-width defects in
[`docs/roadmap.md`](roadmap.md) survived a green suite.
