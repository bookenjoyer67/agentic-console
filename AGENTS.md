# AGENTS.md — agentic-console

What does this file answer, and who is it for?

Cold-start guide for an agent working in this repository. If code and this file disagree, fix one of
them. This repository is a fork of the agentic quality-gate kit. The reference project the kit was
ported from is named `komun`, and it gates a Rust product in its own repository.

## What this is

What is this repository, and which two parts does it ship?

This repository ships two parts. The first is the pipeline: the gate, storage and retrieval servers
under `mcp/`, the drivers under `scripts/`, the suites under `eval/`, the role definitions under
`.claude/agents/`, the seam table in `agentic.config.json` (`agentic.config.json:13` `"name":
"agentic-console",`), and the container runtime under `sandbox/`. The second is a Rust ratatui
console at the repository root; its manifest names it `agentic-console` (`Cargo.toml:2` `name =
"agentic-console"`).

The console is a terminal window onto a repository's quality gate (`README.md:3` `A terminal window
onto a repository's agentic quality gate.`). It reads files, makes `docker` calls and reads command
output. It is also the surface one conversation with the agent is driven from: CONVERSATION is the
primary screen, and a prompt sent from it runs the agent CLI inside the container, whose own permission
mode decides what that turn may touch. The console's own material stays read-only toward the
repository, and its own description says so
(`Cargo.toml:5` `Read-only toward the repository, by construction.`).

This fork deliberately keeps the whole pipeline, including the seam table. The kit's own instrument
decides whether the fork is real, and `docs/fork-proof.md` carries the check and its output
(`docs/fork-proof.md:7` `bash scripts/port-self-test.sh --fork`). The licence is MIT
(`Cargo.toml:6` `license = "MIT"`).

## Critical rules

Which constraints must an agent respect before changing anything here?

### Never commit a credential
Where do credentials live, and what may enter the repository?

- Keep every real credential outside the repository: the broker stages them under the home directory
  (`sandbox/stage-secrets.sh:14` `STATE="${HOME}/.config/komun-sandbox"`).
- Let the agent container hold a dummy token only (`sandbox/run-agent.sh:91` `-e
  ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token`).
- Name an environment variable in prose rather than pasting its value.
- Keep `/target` gitignored (`.gitignore:1` `/target`).

### The seam table is the single source
Where does a fork change a value, and what proves the change landed?

- Edit the value once in `agentic.config.json`, then edit the matching fallback in the consumer.
- Add a row to the port check's consumer list when a new file starts reading the config
  (`scripts/port-self-test.sh:29` `Add a row when a new consumer starts reading the config.`).
- Run `bash scripts/port-self-test.sh --fork` before calling a fork done
  (`docs/fork-proof.md:7` `bash scripts/port-self-test.sh --fork`).

### Touch a source before trusting a lint
Why does the clippy gate touch a file, and which file does it touch?

- Touch the console's entry point first, because a cached linter prints nothing
  (`agentic.config.json:44` `"touch_file": "src/main.rs",`).
- Keep the guard's marker at this crate's own name (`agentic.config.json:42` `"marker": "Checking
  agentic-console",`).
- Read the guard's own reason (`agentic.config.json:45` `a cached clippy run prints nothing and
  exits 0`).

### Keep every citation true
What must a `path:line` citation carry?

- Quote the literal the cited line carries, because a bare location is a v1 form
  (`docs/DOC-STYLE.md:40` `A bare location is a v1 form and fails R2 in v2`).
- Mark a claim you cannot trace, and never invent authority for it (`docs/DOC-STYLE.md:72` `Do not
  delete the claim, and do not invent authority for it.`).
- Run the conformance gate over edited prose (`agentic.config.json:79` `"argv": ["python3",
  "scripts/run-conformance-gate.py"],`).
- Never weaken a check or a test to reach a pass.

### The launcher's mounts enforce the policy
What enforces a role's permissions, and where is the table of them?

- Vary the container mount set per role (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator
  planner implementer tester reviewer project-manager researcher"`).
- Print the matrix from the launcher, with no Docker daemon needed
  (`scripts/README.md:19` `--matrix)  print_matrix; exit 0 ;;`).
- Mount the memory layer as a nested bind inside the read-only workspace
  (`scripts/README.md:41` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`).

## Code layout

Which paths make up the pipeline and the console, and what must a change watch for?

| Path | What | Watch for |
|---|---|---|
| `mcp/` | Gate, storage and retrieval MCP servers, plus the course-tools server | The gate server runs allowlisted names only, and it takes no command string |
| `scripts/` | Drivers: the role launcher, the change classifier, the conformance gate, the scorecard, the config loader | The loader is stdlib only, and it falls back to its embedded defaults |
| `requirements-scorecard.txt` | The scorecard driver's pinned host dependency | Pinned to a commit on purpose: `azathoth-ai` is not on PyPI, and PyPI's `azathoth` is a different project |
| `eval/` | The policy suite and the deterministic-step suite | Run these under pytest inside the container, never on the host |
| `.claude/agents/` | Seven governed role definitions, plus two retired `komun-` definitions | The routing map is the decision of record when the two disagree |
| `agentic.config.json` | The single seam table every consumer reads | Edit this file and each consumer's fallback together |
| `sandbox/` | The container runtime: the credential broker, the image and the launchers | It is runtime, not pipeline, and it keeps the reference project's container names |
| `src/` | The ratatui console: probes, cache, actions and three screens | Probes run on a worker thread, so the UI never waits for one |
| `tests/` | Frame and refresh assertions for the console | They render into a `TestBackend`, so no terminal is needed |
| `docs/` | The pipeline's own documents, the ADRs and the run logs | The logs are history; add to them rather than rewriting them |
| `.github/workflows/ci.yml` | The console's own gate: format, lint and test | One job, named `gate` |

## Quickstart (local dev)

What builds the console, and how is it run against a repository?

```bash
cargo build --release && ./open.sh            # watch the repository this crate sits in
cargo run --release -- --repo /path/to/repo   # or point it anywhere
```

`./open.sh` runs the console in its own tmux session (`open.sh:30` `BIN="$HERE/target/release/agentic-console"`).

## Key architecture facts

What are the facts a change here has to stay consistent with?

- The console binary is `agentic-console`, and it owns one small dependency block
  (`Cargo.toml:20` `ratatui = "0.29"`).
- Configuration is read, never compiled in. Two blocks matter: the shared pipeline blocks, and the
  `console` block the screens use (`agentic.config.json:288` `"container": "agent-console-m1",`).
- The three screens are FLOW, LIVE and INSPECT, on keys `1`, `2` and `3`.
- Probes are slow, so they run off the event loop (`README.md:175` `A worker thread takes each
  snapshot and sends it down a channel`).
- The console reads the config's seam table and labels each value as this repository's or still the
  reference project's (`agentic.config.json:292` `"ports": {"gate": 8101, "storage": 8102,
  "retrieval": 8103},`).
- The pipeline governs seven roles, and the launcher starts one container per role
  (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer
  project-manager researcher"`).
- The container runtime lives under `sandbox/`: a broker holds the keys, and the agent container is
  given a dummy token (`sandbox/run-agent.sh:91` `-e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token`).

## Tests

What does each suite cover, and what did the last run measure?

```bash
cargo fmt --check                                              # formatting
cargo clippy --release --all-targets --offline -- -D warnings  # lint, warnings are errors
cargo test --release --offline                                 # the console's frame and refresh tests
```

- The console's suite renders each screen into ratatui's `TestBackend` and asserts on the frame; its
  fixture repository is synthetic, so it depends on no particular checkout. It renders at the six sizes
  the design targets, from 80x24 to 230x60, so a green suite means the narrow terminals are covered too.
- The formatting step is CI's first one (`.github/workflows/ci.yml:26` `run: cargo fmt --check`).
- The pipeline's own suites run under pytest inside the container
  (`python3 -m pytest eval/test_policy.py eval/test_deterministic_step.py -q` -> `90 passed`).
- The last measured console run is `cargo test --release` -> `138 passed, 0 failed` (59 unit tests, 71
  frame tests, 8 refresh tests). Re-run it after touching the sources: a count read from a run that printed
  no `Compiling` line is the *previous* binary's count, and appending to a test file with a heredoc can
  leave an mtime older than the binary that was built before it.

## Security model (short)

What does the pipeline protect, and where does the console sit?

Threat model: an agent that reads a secret, a container that reaches the internet, and a console
that writes where it should not. The agent container runs on an internal docker network with no
route off the host, and it holds no real credential (`sandbox/run-agent.sh:8` `The agent container
receives NO credential mounts and NO real key material`). The broker is the only thing it can reach,
and the broker holds the provider credentials (`sandbox/broker/broker.py:2` `holds the provider
credentials so the agent container never does`). The broker refuses every path that is not a
provider API path (`sandbox/broker/broker.py:256` `broker only forwards provider API paths`).
Credentials stay outside the repository (`sandbox/stage-secrets.sh:14` `STATE="${HOME}/.config/komun-sandbox"`).

The console's own material is read-only toward the repository
(`Cargo.toml:5` `Read-only toward the repository, by construction.`). It offers no `rm`, no container
restart, no config write and no git command. A turn it starts is a different thing: it runs the agent
CLI in the container, and what that agent writes is what its own permission mode allows, exactly as it
is when a shell starts it.

**Honest limitation:** none of this holds if the host itself is compromised, because the broker runs
as a normal user on that host and mounts the credential files directly.
