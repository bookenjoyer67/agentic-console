# agentic-console

A terminal window onto a repository's agentic quality gate. It shows the gate's flow, the run that
is happening right now, and the machinery underneath, and every value on every screen is a file
read, a `docker` call or a command's output. Nothing is invented, nothing is cached without its age
being shown, and nothing in the repository is ever written.

It is pointed at a repository and reads that repository's `agentic.config.json`, so it ports with
the kit. It ships inside the kit at `console/`, and with no argument it watches the repository it
lives in:

    cargo build --release && ./open.sh          # watches the repo this crate ships inside
    cargo run --release -- --repo /path/to/repo # or point it anywhere

## This repository

This is a **fork of the agentic quality-gate kit**: the machinery that gates software — the gate, storage and
retrieval servers under `mcp/`, the drivers under `scripts/`, the suites under `eval/`, the role definitions
under `.claude/agents/`, and the seam table in `agentic.config.json` — together with the console that shows it.
The kit's own fork check runs here and decides whether the fork is real, which makes this repository the
demonstration as well as the port: [`docs/fork-proof.md`](docs/fork-proof.md) is the evidence, with the command
and its output.

The reference project lives in its own repository, where the kit gates a Rust product. Read this one to
understand the pipeline: it is a tenth the size and holds nothing but the pipeline and the console. It also
still ships inside the kit at `console/`, so a fork of the kit gets the console without a second clone.

## Settling in

What does a newcomer need before the runtime works?

1. Read [`.env.example`](.env.example). It names every variable the runtime reads, says which component reads
   it, and states which of them are never credentials. Copy it to `.env` for your own machine; `.env` is
   ignored by this repository.
2. Stage your own key material on your host, outside the repository: `sandbox/stage-secrets.sh` writes it into
   the state directory with mode 0600. Nothing sensitive lives in this repository.
3. Build the runtime: `sandbox/run-agent.sh` builds the broker from `sandbox/broker` if the image is missing,
   then starts the agent container on an internal network with a dummy token.

A clone of this repository names the repository it watches, because the console's no-argument default is the
repository it sits inside:

    cargo build --release && ./open.sh /path/to/repo

| document | what it is |
| --- | --- |
| [`docs/fork-proof.md`](docs/fork-proof.md) | the fork evidence: the check, its output, every seam before and after |
| [docs/architecture.md](docs/architecture.md) | the probe worker, the cache, the modules, the measured numbers |
| [docs/provenance.md](docs/provenance.md) | the evidence rules: what may name a checkpoint, when a ruling is offered |
| [docs/screens.md](docs/screens.md) | the three screens and every panel on them |
| [docs/keys.md](docs/keys.md) | every key and every mode |
| [docs/config.md](docs/config.md) | the seam-table keys this crate reads, and the CLI flags |
| [docs/roadmap.md](docs/roadmap.md) | what comes next, and the defects it fixes |

## The three screens

| tab | key | what it shows |
| --- | --- | --- |
| **FLOW** | `1` | The map with live lights. Lane A: a pull request arriving, the CI jobs parsed from the workflow file with their `needs` chain and which of them can block a merge. Lane B: a brief driving an orchestrated run, the eight ordered steps, with the two human checkpoints marked as decisions. Lane C: a repeated agent step being turned into a script. Lane D: a role box being probed. Every box names the artifact behind it on selection. |
| **LIVE** | `2` | What is happening right now: whether a run is in flight (a read-only `docker exec <container> ps`), the last 20 gate-journal rows as a table, the last storage entries, the port readings, the gate server's own `list_gates`, and the **checkpoint card**. |
| **INSPECT** | `3` | The machinery: the config's seams, each marked as still Komun's or changed for this repo; the role x mount matrix; the grant grid (who may do what); the suites with their last result; the conversion candidates with their next review dates; the ADRs. |

## The checkpoint card

The most important element on the screen. The console never decides that a checkpoint is open: it
looks for evidence, in order, and names the evidence it used. **Every source is attributed**: a piece
of evidence may name a checkpoint for this run only when it is that run's own. A transcript of a
different session — even the newest file in the same directory — is another run's artefact and names
nothing on this card.

1. The prompt of a run that is in flight in the container (the `-p` argument of the agent process) —
   this is where a run's own instruction to stop is written. Only a live process has one, and a live
   process means the run is working, so the checkpoint this read names is the one the running run is
   heading for, never one waiting on the operator.
2. The named session's **own transcript inside the container**:
   `<console.session_dir>/<session-id>.jsonl`, read from its tail through the same probe layer as
   every other reading. This is the run's own words about where it halted, and it settles the
   checkpoint on its own: with the process gone — the only state in which a ruling is offered — it
   is the read the card rests on.
3. A transcript in **`console.evidence_dir`** that is *attributable to that same session*: its own
   file name carries the session id (the harness's own copies do — `run3-h1-2-<uuid>.jsonl`) or its
   own metadata block names the session. A transcript there whose name carries no session id, or
   another session's id, is **ignored**: being the newest file in that directory is not attribution.
   The card prints the directory it looked in, how many `run*` transcripts were there, how many
   carried a session id, and which ones it ignored and why.
4. The storage journal's shape: the newest record is the planner's plan with no implementer record
   after it (checkpoint 1), or the reviewer's verdict with no project-manager close after it
   (checkpoint 2), and that record is inside `console.checkpoint_fresh_minutes` of the reading.

Evidence 4 is an inference from the journal and the card says so on its own evidence lines; the
card keeps the evidence lines in the order above, each with its own source and age. It
**corroborates and never overrules**: where it names a different checkpoint from a run's own words
(evidence 1-3), the card says the two disagree, shows the strongest read's own words and offers no
ruling, rather than quietly preferring one of them. Every line carries its own source and its age.
When the card offers a ruling, `e` opens a text field and the ruling is sent as the next
invocation of the one session the card names: `claude --resume <session-id> -p "<ruling>" ...`.

The run is headless and one invocation per phase: at a human checkpoint the orchestrator ends its
turn and the CLI process exits, and the next phase is a **separate** invocation that resumes
that session: `claude --resume <session-id> -p "<ruling>"`. So a process being in flight means the run is working, never waiting; the waiting happens
after the process is gone. The card keeps the two facts of the reading apart: whether the evidence
**names** a checkpoint, and whether a `claude` process is **running** right now.

| evidence names a checkpoint | a `claude` process is running | state the card shows |
| --- | --- | --- |
| yes | yes | `RUN IN FLIGHT` — the run is working, so the checkpoint lies ahead of it; the ruling is refused, because resuming that session would start a second process on it |
| yes, one read names it | no | `STOPPED AT <checkpoint> -- A RULING RESUMES IT` — the run stopped there and its process is gone; the ruling is what resumes the session. This is the actionable state |
| yes, but two reads name different checkpoints | no | `STOPPED AT <checkpoint> -- THE EVIDENCE DISAGREES: NO RULING OFFERED` — the card shows the strongest read's own words and names which read said what; `e` does not open and `--dump` prints `contested` until the disagreement is read |
| no | yes | `RUN IN FLIGHT` — work in progress; no checkpoint is named yet |
| no | no | `NO RUN, NO CHECKPOINT` — no ruling is offered: no session is named to resume. This is also the state when the only transcripts in the evidence directory that name a checkpoint are unattributable, and the card says which directory it looked in and why it ignored them |

The ruling is offered in exactly one state: the row where a checkpoint is named by one read and no
process is running. In the `RUN IN FLIGHT` rows it is refused, and the refusal names the hazard: a second
`claude --resume <session-id>` process on the same session jsonl would be a second writer on it. The
same state and its evidence basis are printed in the status bar, on the confirmation screen, in
`--dry-run-actions`'s `guards` line and in `--dump`.

## Keys

| key | action |
| --- | --- |
| `1` `2` `3`, `Tab`, `Shift-Tab` | switch screens (always — see the note on approval below) |
| `j` / `k`, arrows, `PageUp` / `PageDown` | move the selection; scroll INSPECT |
| `g` | back to the first box |
| `a` | the action menu |
| `Enter` | **approve**: with a ruling offered, opens the confirmation for the default canned ruling |
| `e` | the **ruling chooser**: `1`-`9` or `Enter` picks a canned ruling, `j`/`k` moves, `c` free text, `Esc` closes |
| `t` | start a brief |
| `r` | re-read every reading now, forcing every probe (the console also re-reads on its own, on `console_probe_cadence.refresh_seconds`; a probe is not re-run inside its own TTL) |
| `L` | show/hide the action log |
| `?` | the key reference |
| `q`, `Ctrl-C` | quit |
| menu: `j`/`k` move, `Enter` choose, `Esc` close | |
| input: type, `Backspace`, `Enter` to review the command, `Esc` to cancel | |
| confirmation: `y` or `Enter` runs it, `n` or `Esc` cancels | |

### Approving a checkpoint

Two paths, and both end at the same confirmation screen showing the exact command before anything runs:

* **`Enter`** — the default canned ruling ("approve as written"). One keystroke to the point of decision, then `y`.
* **`e`** — the chooser: `1` approve as written, `2` approve with a rework first (the input opens prefilled), `3` halt (prefilled), `c` free text (empty).

The wording of each canned ruling lives in `agentic.config.json` under `rulings`, each with its label, the
exact text sent, and whether the input opens prefilled — so a fork writes its own approval language without
touching code.

**Deliberate:** `1`/`2`/`3` keep switching screens even while a ruling is offered, and the canned rulings live
inside the chooser instead. Binding them directly to the card would mean a keystroke meant for navigation could
end a run. If you want the numbers direct on the card, it is a small change to the key handling — it was left
out on purpose, not overlooked.

## Actions, and why they are safe

Every action first puts its **exact argv, its working directory, its environment and anything it
would write** on the screen and waits for a confirmation key. There is no path from a keypress to a
command that skips the confirmation screen.

| action | what it runs |
| --- | --- |
| port self-test | `bash scripts/port-self-test.sh`, in the repository root |
| policy + step suites | the config's own `toolchain.commands.policy.argv` |
| gate server selftest | the gate server's own selftest, inside the container |
| one gate by name | the gate server's `run_gate` tool, through the server, for an allowlisted name only |
| role box | `bash scripts/run-agent.sh <role> bash -lc "<one shot>"`; an unknown role is refused before anything starts |
| brief | stages the brief in the configured briefs directory (never inside the repository), then `docker exec -w <workspace> <container> claude -p "<brief>" <console.claude_flags>` |
| ruling | `docker exec -w <workspace> <container> claude --resume <session-id> -p "<ruling>" <flags>` — the flags come from `console.claude_flags` with `--agent orchestrator` filtered out, because the session carries the agent forward |

Refusals are part of the design, not an error path: a second brief while a run is in flight, an
unknown role, a gate name the config does not carry, an empty brief, and a ruling with nothing to
resume are all refused before a command is built. No action offers `rm`, a container restart, a
config write or a git command.

Every `docker exec` the console builds is assembled by one helper, in docker's own grammar —
`docker exec [OPTIONS] CONTAINER COMMAND [ARG...]`: every option goes **before** the container name.
An option written after it is read by docker as the executable, which fails with
`exec: "-w": executable file not found in $PATH`.

A started action is tracked: the child is re-read on every tick and on every refresh, so an action
that has exited stops being reported as running, and how it ended — `exited with code N`, or
`was killed by signal N (SIGTERM)` — goes into the action log and the status bar.

## Refresh: a worker thread, a cadence, and a cache

The probes are the slow part — asking the running gate server for its own `list_gates` spawns python3
inside the container and imports fastmcp, which is on the order of a second. So they do not run in the
event loop at all. A worker thread takes each snapshot and sends it down a channel; the loop draws the
last snapshot it was handed and never waits for a probe. On a collect that takes two seconds, keys are
still handled and frames are still drawn throughout, and the header says

    STALE  A READ IS IN FLIGHT

which is the honest thing to say about the readings on the screen: they are the last *completed* read,
and the age beside them is the age of that read. Every reading keeps its own source and its own age
(the footer and each screen print them), and a failed read stays a failed read — it is never replaced
by an older success.

Each probe's answer is cached in memory with its own time to live, so a probe is not re-executed more
often than its cadence allows; a cached answer keeps the timestamp of when it was **read**, not when it
was served from cache. The cadence lives in `console_probe_cadence` in the repository's
`agentic.config.json`: `refresh_seconds` (how often a snapshot is taken), `stale_after_seconds` (the
age at which the snapshot on screen is labelled STALE), `default_ttl_seconds` (the TTL of any probe the
`probes` table does not name) and `probes`, a per-probe TTL table keyed by the probe names the console
uses. The expensive one is asked far less often than the file stats and the process table:

    "gate_allowlist": 30     the server's own list_gates, ~1-2s: asked every 30 seconds
    "docker_ps": 3           the cheap local reads
    "files": 3
    "pipeline": 15           the recorded results, which change when a gate runs

`r` asks for a collect that no TTL may serve: every probe runs again. What each probe costs, and what
the cache saves, is measured rather than asserted:

    agentic-console --repo PATH --probe-timings

which runs every probe once (cold), again immediately (warm) and once more a refresh interval later
(steady state, which is what the console actually pays per refresh), printing each probe's name, its
exact invocation, its duration and whether it was answered from memory, then the totals.

## Non-interactive modes

    agentic-console --repo PATH --dump                   the whole reading as plain text, exit 0
    agentic-console --repo PATH --probe-timings          what each probe costs, measured
    agentic-console --repo PATH --dry-run-actions        every action's exact command, run nothing
    agentic-console --repo PATH --dry-run-action ruling --value "approved"

`--dump` names the file, docker call or command behind every value, with the age of each reading, so
the data layer can be checked in a pipe. `--dry-run-actions` builds each command with the same
function the confirmation screen calls, against the state the console actually reads.

## Configuration

Everything comes from `<repo>/agentic.config.json`. Two blocks matter most here:

* the existing `container`, `toolchain.commands`, `containers`, `roles`, `artifacts`,
  `classification` and `port.komun_defaults` blocks, which the screens read as they are;
* the `console` block the console itself uses: `container`, `claude_command`, `claude_flags`,
  `role_container_prefix`, `ports` (gate/storage/retrieval), `session_dir` (where the CLI writes
  session transcripts inside the container: `/root/.claude/projects/<project>`), `evidence_dir` (the
  directory the harness drops its own `run*` transcript copies into; a transcript there is
  attributed to a session only when its own file name carries that session's id), `briefs_dir`,
  `checkpoint_fresh_minutes`, `ci_jobs` (the fallback job list when the workflow file cannot be
  parsed), `orchestration_steps` (the ordered sequence the FLOW map draws) and `komun_defaults`.

`console.komun_defaults` lists the keys whose values are still Komun's. The INSPECT screen reads it
to flag each seam as *still a Komun default* or *changed for this repo*; a fork that changes a value
deletes its key from that list. `scripts/agentic_config.py`'s embedded `DEFAULT` carries the same
block, and the two are compared by hand:

    python3 -c "import sys, json; sys.path.insert(0,'scripts'); import agentic_config as a; \
      print(a.DEFAULT == json.load(open('agentic.config.json')))"

`--container NAME` overrides `console.container` for one session.

## Building and testing

    cargo clippy --release --all-targets -- -D warnings    # zero warnings, no exceptions
    cargo test                                             # frame assertions, fixture repository

The tests render each screen into ratatui's `TestBackend` at a fixed size and assert on the frame:
the eight step labels, the five job names, the checkpoint card, the gate names, the seam classes,
the mount matrix, the grant grid, the conversion candidates and the ADRs. Four of them cover
evidence attribution: a foreign transcript in the evidence directory may not name this card's
checkpoint, an unattributable one is ignored with the reason and the directory shown, an
attributable copy still names its checkpoint, and a journal that disagrees with the run's own
transcript is reported as a disagreement instead of being resolved silently. The fixture is a synthetic
repository under `target/tmp`, so the tests depend on no particular repository and never write to
one. Its CI file lists its jobs in a scrambled order on purpose: the FLOW map must follow the file.

`tests/refresh.rs` covers the refresh architecture, and every test in it fails against a console that
probed on the UI thread: a probe called twice inside its TTL is executed once; a cached answer carries
the time it was read, not the time it was served; a probe whose argument changed is re-read before its
TTL; a failed read replaces an older success rather than being hidden by it; `r` forces every probe;
each probe takes its TTL from `console_probe_cadence`; and the event loop keeps drawing and handling
keys while a deliberately slow collect is in flight, which is driven through the same `ui::step` the
real loop calls, with the slowness injected rather than waited for.
