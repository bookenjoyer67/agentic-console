# Configuration

The console is pointed at a repository and reads that repository's `agentic.config.json`. Nothing about a
repository is compiled into the binary, so the same crate drives any repository that carries the keys below.

    agentic-console --repo /path/to/repo                 # default: the working directory
    agentic-console --repo /path/to/repo --config other.json

## `console`

| key | example | what it is |
| --- | --- | --- |
| `container` | `agent-rev-m3` | the sandbox container a run happens in |
| `claude_command` | `claude` | the agent binary, as invoked inside the container |
| `claude_flags` | `["--agent", "orchestrator", "--permission-mode", "acceptEdits"]` | flags for a brief; a ruling re-sends these with `--agent orchestrator` filtered out, because the session carries the agent forward |
| `role_container_prefix` | `agent-rev-m4-` | how a role box's container is named |
| `ports` | `{"gate": 8003, "storage": 8001, "retrieval": 8002}` | the servers the console reads |
| `evidence_dir` | `~/komun-agent-exercise-4-3` | where run transcripts and evidence land |
| `briefs_dir` | `~/komun-agent-exercise-4-3/briefs` | where a staged brief is written |
| `checkpoint_fresh_minutes` | `30` | how fresh a journal record must be to name a checkpoint |
| `session_dir` | `/root/.claude/projects/-workspace` | where the named session's own transcript lives, inside the container |
| `session_window_seconds` | `120` | how recent a session must be to count as this run's |
| `ci_jobs`, `orchestration_steps` | | the pipeline as data: the lanes and steps FLOW draws |
| `komun_defaults` | a list of key names | see below |
| `rulings` | three canned rulings | see below |

### `komun_defaults`

A list of the keys whose values are still the reference repository's. A fork that changes one of them
**deletes its key from this list**, and that deletion is what the console reads to mark a value as the fork's
rather than the reference repository's. The INSPECT screen's seam panel shows the result.

### `rulings`

Each entry carries `id`, `label`, `text` and `prefill`. The label is what the chooser shows; the text is what
is sent; `prefill: true` opens the text field with the canned text in place, which is how a ruling asks you to
name a change before it goes. The wording is yours to change without a rebuild.

## `console_probe_cadence`

Freshness, not latency: the console takes its snapshots on a worker thread and draws the most recent one, so
nothing here makes the screen wait.

| key | meaning |
| --- | --- |
| `refresh_seconds` | how often a snapshot is taken (3) |
| `stale_after_seconds` | the age at which a panel labels its reading **STALE** (15) |
| `default_ttl_seconds` | the TTL of any probe the table below does not name (3) |
| `probes` | a per-probe TTL, in seconds |

The `probes` table names each probe it tunes: `docker_ps`, `container_ps`, `ports`, `sessions`,
`session_transcript`, `gate_journal`, `storage_journal`, `retrieval_journal`, `files`, `pipeline`,
`conversions`, `grants`, `selftest_record`, `port_self_test_record`, `evidence_files`, `evidence_tails`,
`gate_allowlist`, and `gate_list` — the expensive one, which asks the running gate server for its own gate
list and costs about a second and a half. A reading's age on the screen is the age of the read, never the age
of the collect that served it, and `r` forces every probe regardless of this table.

## Flags

    --repo PATH            the repository to point at (default: the working directory)
    --config PATH          the config file (default: <repo>/agentic.config.json)
    --container NAME       override console.container (the running sandbox container)
    --dump                 render the current state as plain text on stdout and exit 0
    --probe-timings        run every probe once and print its name, its exact invocation, how long it
                           took, whether the cache answered it, and the total -- exit 0
    --dry-run-actions      print every action's exact command instead of running it, exit 0
    --dry-run-action ID    print one action's command; combine with --value
    --value TEXT           the value the action would use (with --dry-run-action)
    -h, --help             this text

`--dump` is the machine-readable form of every screen, and it is what the tests assert against. `--value` and
`--dry-run-action` exist so a script can ask what an action *would* run without a terminal.

## Forking

A fork points the crate at its own repository and carries its own `console`, `console_probe_cadence` and
`rulings` blocks. Two properties make that cheap:

- A missing key is reported, not defaulted. The console says what it could not read instead of inventing a
  value, so a half-migrated config shows up immediately rather than as a wrong reading.
- The kit's own consumers read the same seam table through `scripts/agentic_config.py`, so a fork does not
  maintain two configs.

Nothing in this crate writes inside the repository. Every probe reads, and the only writes an action causes
happen inside the container or in `briefs_dir`, both of which are outside the working tree.
