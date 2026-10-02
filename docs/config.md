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
| `conversation.stream_flags` | `["--output-format", "stream-json", "--verbose", "--include-partial-messages"]` | the flags every turn carries after the prompt; absent, or empty, keeps these four — a turn with no streaming flags reads the run's own words as one block when the turn ends instead of as events while it is written |
| `console_probe_cadence.probes.conversation_transcript` | `3` | how often the console's **own** session's record is re-read. It is forced when a turn ends, so this cadence governs the passive case: a long turn's lines move from LIVE to CONFIRMED while it is still running |
| `ci_jobs`, `orchestration_steps` | | the pipeline as data: the lanes and steps FLOW draws |
| `rulings` | three canned rulings | see below |

### How the console decides a value is still a reference's

There is no list to maintain. The console compares each `console.*` value against `port.ancestors` — the chain
of generations this repository descends from — and marks the value as still a reference's default when it
matches one, naming the generation it reaches back to. A value that matches several is reported against the
oldest, which is the sharper statement: unchanged since that generation. Changing a value is all a fork has to
do; the seam panel on INSPECT shows the result.

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
`conversions`, `scorecards`, `grants`, `selftest_record`, `port_self_test_record`, `evidence_files`, `evidence_tails`,
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

## `models`

Which model does each consumer use, and where is that decided?

| key | the value this repository ships | who reads it |
| --- | --- | --- |
| `models.agent` | `""` | the agent CLI; the console sends no `--model` flag, so the CLI resolves its own default |
| `models.sandbox_cli` | `deepseek-v4-flash` | the opencode hint the sandbox launcher prints |
| `models.retrieval` | `BAAI/bge-small-en-v1.5` | the retrieval server's `RETRIEVAL_EMBEDDING_MODEL` |

The block is a top-level key in `agentic.config.json` (`agentic.config.json` `"retrieval":
"BAAI/bge-small-en-v1.5"`), and each consumer reads it rather than holding its own copy. The retrieval
launcher resolves the key through `scripts/agentic_config.py`, with the environment variable still
winning and the literal fallback intact (`scripts/start-mcp-servers.sh:75` `$(cfg models.retrieval
'BAAI/bge-small-en-v1.5')`). The sandbox launcher prints the configured model in its opencode hint
(`sandbox/run-agent.sh:147` `opencode run -m sandbox/$(cfg models.sandbox_cli`).

`scripts/new-project.py --propose` writes all three into the proposal manifest, and `--apply` plants
them under `models` in the fork's config. The wizard takes `--agent-model`, `--sandbox-model` and
`--retrieval-model` (`scripts/new-project.py` `--sandbox-model SANDBOX_MODEL`). Accepting those
defaults records today's effective values and changes no behaviour.

The block is deliberately not a `port.seam`: its values equal the ancestor's, and a seam whose value
equals its ancestor's fails the fork check (`agentic.config.json` `"seams"`). A fork that edits one
line here changes that model in every consumer.

## `runtime_commands`

The exact commands the RUNTIME panel prints for a prerequisite that is missing, and the one place
they are written. The panel is read-only: it prints these and never runs one, and a hardcoded copy
in the console is how a fork shows its users another project's paths.

| key | the value this repository ships | the panel row it fixes |
| --- | --- | --- |
| `runtime_commands.engine_launcher` | `bash sandbox/run-agent.sh` | the broker and agent containers |
| `runtime_commands.base_image_build` | `docker build -t {image} .` | the base image |
| `runtime_commands.tools_image_build` | `docker build -f sandbox/Dockerfile.m3 -t {image} .` | the tools image |
| `runtime_commands.gate_start` | `docker exec {container} python3 /workspace/mcp/gate/server.py --port {port} --host 0.0.0.0` | the gate MCP server |
| `runtime_commands.storage_retrieval_start` | `docker exec {container} bash /workspace/scripts/start-mcp-servers.sh` | the storage and retrieval MCP servers |

`{image}`, `{container}` and `{port}` are substituted at print time from the config's own keys —
`containers.base_image` / `containers.tools_image`, `console.container` and `console.ports` — so the
image, container and port names each stay in exactly one place. Each row refuses in the console's own
words when a value it reads is empty or absent (`src/state.rs:2581` for the launcher), rather than
printing a blank line or a guessed command.

`runtime_commands.engine_launcher` is **not** `artifacts.launcher`. `artifacts.launcher`
(`scripts/run-agent.sh`) is the per-role wrapper that drives one role box; `engine_launcher`
(`sandbox/run-agent.sh`) is the kit's own engine, which creates the broker, the network and the agent
container the box needs. They are different tools, and neither key may be re-pointed at the other.
The gate's start command and the storage/retrieval starter are likewise different commands from
different files and are kept as two keys.

`scripts/new-project.py --propose` writes all five into the proposal manifest, and `--apply` plants
them under `runtime_commands` in the fork's config, idempotently. The wizard takes `--engine-launcher`,
`--base-image-build`, `--tools-image-build`, `--gate-start` and `--storage-retrieval-start`
(`scripts/new-project.py` `--engine-launcher ENGINE_LAUNCHER`). Accepting those defaults records
today's effective commands and changes no behaviour.

The block is deliberately not a `port.seam`, for the same reason as `models`: the fork check sweeps a
seam for an ancestor's literal, and these are printed wiring the wizard proposes and a fork edits
freely, not identity a fork must differ on.
