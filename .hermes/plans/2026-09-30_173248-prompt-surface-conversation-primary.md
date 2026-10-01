# The Prompt Surface — agentic-console becomes the place the pipeline is driven from

> **For Hermes:** implement this with the `subagent-driven-development` skill — one fresh subagent per
> task, spec-compliance review then code-quality review before the next task starts. Load the
> `live-state-consoles` skill before touching any pane: the console's rules there (a window, never a
> store; a reading carries its source and its age; refuse rather than guess) are the ones this plan
> extends, not replaces.
>
> **No task in this plan commits, pushes or deploys anything** until the operator says so, per the
> standing rule for this repository. Each task ends at a green gate and a diff the operator can read.

**Goal:** turn the console from a read-only window into the place the agentic pipeline is prompted
from — a conversation-primary screen (transcript + composer, Claude Code / opencode shape) that sends
a prompt to the orchestrator, streams the run's own words back as they arrive, and keeps the three
existing screens intact as overlays behind it.

**Architecture:** one new primary screen (CONVERSATION) rendering a console-owned transcript model
(`src/conversation.rs`) that is fed by the existing action machinery: a prompt becomes an
`ActionKind::Prompt` command, built by `actions::build`, shown for confirmation, spawned by
`actions::start`, and drained on the existing tick. Each turn is one `docker exec … claude -p
--output-format stream-json --verbose` invocation, resumed by session id on every later turn, against
a session id the console mints itself. FLOW, LIVE and INSPECT keep their renderers and their content
and are drawn as overlays.

**Tech stack:** Rust 2021, ratatui 0.29, crossterm, serde/serde_json (already the whole dependency
list — this plan adds no dependency), `docker exec` as the only transport, Claude Code CLI 2.1.280
inside the sandbox container.

---

## 1. The four decisions already taken

| question | decision | consequence for this plan |
| --- | --- | --- |
| how the console holds a conversation | **decided in this plan** — see §2; recommendation is one-shot-per-turn with a `Sender` seam that makes the persistent stream a swap, not a rewrite | Phase 0 spikes settle the protocol before any UI work; Phase 5 is deferred and evidence-gated |
| what happens to FLOW / LIVE / INSPECT | conversation-primary; the three screens become overlays on the same renderers, all content preserved | Phase 3 keeps every existing frame test by rendering each overlay at full area |
| the confirmation doctrine | **kept, not weakened** — `i` → type → `Enter` → `y`; the composer box *is* the confirmation screen | Phase 3 defines the key model so no keypress reaches a command |
| who the composer addresses | the orchestrator only; other roles stay on the role-box action | `ActionKind::Prompt` builds only orchestrator invocations |

## 2. How the console should hold a conversation

Two architectures are really available, and the choice decides everything downstream.

### Option A — one invocation per turn, resumed by id

Every send is a fresh child:

    docker exec -w /workspace agent-console-m1 claude -p '<prompt>' \
        [--session-id <uuid> | --resume <uuid>] --output-format stream-json --verbose \
        --include-partial-messages --agent orchestrator --permission-mode acceptEdits

The child exits at the end of the turn. The console reads the JSONL stream as it arrives and appends
to its transcript, so the screen still fills token by token.

**For:** every turn is a bounded process, so "in flight" is the console's own child's liveness rather
than a guess at a process table; the session id is minted by the console (`--session-id`), so the
conversation knows which conversation it is from the first keystroke instead of inferring it from the
session directory the way the ruling action has to; the on-disk session `.jsonl` stays the single
source of truth; it reuses `actions::build`, `Guards`, `Command`, `actions::start` and the reader
threads the console already has, so the delta is a parser and a renderer; and it cannot create the
"two writers on one session jsonl" hazard the existing refusal text is built around.

**Against:** no streaming *input*, so no interrupting a turn mid-flight (stopping it means killing the
console's own `docker exec` child) and no queueing a follow-up behind a running one; each turn pays a
process start plus the CLI's own boot.

### Option B — one long-lived bidirectional session

One child per conversation, spoken to over stdin:

    docker exec -i -w /workspace agent-console-m1 claude -p --input-format stream-json \
        --output-format stream-json --include-partial-messages --replay-user-messages [--resume <uuid>]

The console writes one JSON user message per turn and reads events from stdout; the turn ends at a
`result` event; `Ctrl-C` in the composer can kill the current turn and leave the session alive.

**For:** the real Claude Code feel — mid-turn interrupt, follow-ups queued behind a running turn
(the `result` envelope carries `queued_turn_count`), and one process per conversation whose liveness is
exact.

**Against:** the console then holds live state rather than only re-reading artifacts, so the "window,
never a store" doctrine has to be reworded rather than extended; the input envelope is a schema the
console must get right, owned by a CLI that npm-updates itself inside the container; a persistent child
holding a session is precisely the second-writer shape the ruling refusal exists to prevent; and
recovery from a dead child means re-attaching with `--resume`, which is a code path with no natural
test.

### Recommendation: **Option A now, Option B behind a seam**

Five reasons, in the order they matter:

1. **Option A already has the visual feel.** The flag that produces sub-turn streaming is
   `--include-partial-messages`, which works with `-p` and `--output-format stream-json` — verified on
   2.1.280 in the sandbox. Assistant events arrive as the turn progresses, so text appears progressively
   in both options. What Option B adds is streaming *input*: interrupt and queued follow-ups. Real, but
   it is a feature, not the difference between a chat window and a log.
2. **The evidence the repo already owns.** The whole action layer exists to make one invocation per
   operator decision, with the exact argv shown and confirmed, and the guard suite's central refusal is
   about two writers on one session. Option A keeps both true; Option B makes the guard ambiguous.
3. **Incremental diff.** Option A is a parser plus a renderer plus one `ActionKind`. Option B is a
   protocol client plus a lifecycle plus a recovery path.
4. **Risk concentration.** Option B depends on an input schema that is version-owned by a
   self-updating CLI. This repository's rule everywhere else is that an unverified assumption is
   printed as unverified; a protocol the console cannot check is a poor foundation for its main screen.
5. **The seam is cheap to place now.** `Sender` (below) is ~40 lines and is what makes Phase 5 a
   substitution rather than the second rewrite of the same screen.

    // src/session.rs — the seam Phase 5 swaps behind.
    pub trait Sender {
        /// Start one turn and keep its stream. The child is the console's own from here.
        fn send(&self, command: &actions::Command) -> Result<Started, String>;
        /// Everything that has arrived since the last poll. Never blocks.
        fn poll(&mut self) -> Vec<StreamLine>;
        /// Whether a turn is running right now, from the child itself.
        fn in_flight(&mut self) -> bool;
        /// The pid the turn is running as, for the pane's own line.
        fn pid(&self) -> Option<u32>;
    }

`OneShotSender` is the Phase 2 implementation (it wraps `actions::start`). `StreamSender` is Phase 5.

Phase 0's third spike was written to make the deferred decision on evidence rather than on that
paragraph, and it has now **run**: the CLI accepted the stdin envelope shape, stayed alive between two
envelopes, and produced two complete turns in one process (see `docs/spikes/prompt-surface-protocol.md`
§3). So `--input-format stream-json` is real, and Phase 5 is schedulable rather than hypothetical. The
recommendation above still stands, on the reasons that survive that news: the doctrine, the two-writer
hazard, the incremental diff, and a dependency on a self-updating CLI's input schema. What changed is
that Phase 5 is now a scheduled decision rather than a contingency.

## 3. Facts this plan rests on (measured, not assumed)

Run against the sandbox container, CLI `claude 2.1.280`:

    $ docker exec agent-console-m1 claude --version
    2.1.280 (Claude Code)

`--help` confirms the flags this plan uses:

| flag | what it gives |
| --- | --- |
| `--output-format stream-json` | newline-delimited JSON events instead of prose |
| `--input-format stream-json` | realtime streaming input (Phase 5 only) |
| `--include-partial-messages` | partial chunks, with `--print` + `--output-format stream-json` |
| `--replay-user-messages` | stdin messages echoed back on stdout for acknowledgment |
| `--session-id <uuid>` | the console mints the conversation's id rather than discovering it |
| `--resume <session-id>` / `--fork-session` | continue that one conversation, or continue it under a new id |
| `--forward-subagent-text` | subagent text/thinking forwarded with `parent_tool_use_id` set |

A real one-shot invocation (`-p "Reply with the single word: pong" --output-format stream-json
--verbose --include-partial-messages`) returned six envelopes, in this order:

    system  (subtype hook_started  — hook_id, hook_name, hook_event, uuid, session_id)
    system  (subtype hook_response — output, stdout, stderr, exit_code, outcome)
    system  (subtype init          — cwd, session_id, tools, mcp_servers, model, permissionMode,
                                      slash_commands, apiKeySource, claude_code_version,
                                      output_style, agents, skills, plugins, capabilities)
    system  (status                — status, session_id)
    assistant (message, parent_tool_use_id, session_id, uuid, timestamp, error, request_id)
    result    (duration_api_ms, stop_reason, session_id, total_cost_usd, usage, modelUsage,
               permission_denials, terminal_reason, is_error, num_turns, result, duration_ms)

Every envelope carries a `uuid` and a `session_id`. The session `.jsonl` inside the container
(`/root/.claude/projects/-workspace/<session-id>.jsonl`) carries `uuid`, `parentUuid`, `sessionId`,
`message`, `type` (`assistant` / `user` / `attachment` / `last-prompt` / `cost-state`), `version` and
`timestamp`.

**That shared `uuid` is the linchpin of the design.** It gives the console three things at once: a
dedupe key for partial messages that arrive repeatedly for the same item, a reconciliation key against
the session `.jsonl` (so a streamed line can be *confirmed* rather than believed), and a stable
identity for a transcript item across a resume.

### What Phase 0 measured, after this plan was first written

Phase 0 ran on 2026-09-30 and its output is recorded in `docs/spikes/prompt-surface-protocol.md`. Four
measurements change what is written below, so read them before the phases:

| measured | consequence |
| --- | --- |
| `--session-id` **is** honored: all six envelopes carried the minted id and the transcript landed at `<minted-id>.jsonl` | the first turn can mint the conversation's id; no task needs the read-the-id-out-of-`init` fallback, though Phase 2's code keeps it as the second door |
| `--resume` **replays nothing**: 0 of 6 uuids were shared between turn one's capture and turn two's | Task 4.2 is not a dedupe rule but the opposite: a resumed turn contributes only new items, and the history lives in the session record |
| the persistent stream **works**: two stdin envelopes, one process, two complete turns | Phase 5 is schedulable rather than hypothetical — the recommendation above still starts with Option A, but on the doctrine and diff-size reasons alone, not on doubt about the protocol |
| a second turn emits **its own `init` envelope** | pinned as a parser rule: `init` after the first updates the session, model and CLI version and touches no items, or a persistent stream would reset its own transcript every turn |
| **only `assistant` and `user` records carry a corroborating uuid**; `system/*` and `result` envelopes have none | `ItemState` gains `Own`: a console line from a `system` envelope is this console's own line, not a reading that failed to corroborate |

One further measurement is a refusal to expect rather than a rule: `--session-id` was refused outright
with `Error: Session ID <id> is already in use.` when the id already had a session record on disk. A
first turn that has to be retried therefore mints a fresh id, and the console shows the CLI's own
sentence rather than one of its own.

**Every turn in Phase 0 failed at the API with HTTP 429, so four things are still unmeasured**: a
successful `result` envelope's fields, `tool_use` and `tool_result` block shapes, the
`--include-partial-messages` chunks the streaming cursor depends on, and a non-zero `queued_turn_count`.
Task 0.5 below makes re-measuring them a gate, and Phase 1's parser must not be called done until the
tool-block shapes are pinned against a real capture.

Two properties of the repo that this plan must respect:

- `agentic.config.json` is the single seam table. A new console key is also a new
  `console.komun_defaults` entry until a fork changes it, or the INSPECT seam panel mislabels it.
- `AGENTS.md` cites `Cargo.toml:5` (`Read-only by construction.`) and `README.md:3` (`A terminal
  window onto a repository's agentic quality gate.`), and the conformance gate reads those citations.
  Changing the console's description moves both citations, so they change in the same task.

## 4. What this does to the doctrine (and why it is an extension, not a break)

The console's founding rule (`docs/architecture.md:9` `Every reading carries its source and its age,
or it is not shown.`) survives intact. What changes is that the console now owns exactly one thing of
its own, and the plan makes it say so:

- **The transcript pane distinguishes what it believes from what it read.** An item drawn from the live
  stream is `LIVE` (the console's own buffer, unconfirmed). Once the turn ends, the console re-reads the
  session `.jsonl` and every item whose `uuid` appears there becomes `CONFIRMED`. Items that do not
  match stay `LIVE` and stay labelled. Nothing is deleted and nothing is upgraded silently.
- **The pane names its own state in its header**, the way every other panel names its source and age:
  `buffer: 41 items, 34 confirmed against <session-id> at age 12s`.
- **A reading that fails is still a failure.** A stream line the console does not understand becomes an
  item that says so, quoting the type and the CLI version — never silence, never a guess.
- **`--dump` prints the conversation under a heading that says it is the console's own**, so the
  machine-readable form keeps the distinction the screens draw.
- **The confirmation screen is not softened.** The composer's `Enter` produces the exact argv; `y` runs
  it. There is still no path from a single keypress to a command.
- **The console reads nothing new about the pipeline and writes nothing inside the repository.** The
  one write it has always done (`briefs_dir`, outside the tree) is unchanged, and `--session-id` writes
  nothing on the host at all.

---

## Phase 0 — the protocol spike (no product code)

Everything here is evidence-gathering. Each task records real output into
`docs/spikes/prompt-surface-protocol.md` (a new file; it is not in `gates.conformance.files`, so it is
prose without citations and says so at its head).

**Where it ran, and what was up at the time:** the first protocol probe was taken in `agent-rev-m3`
(the console's configured container was down); every Phase 0 task below then ran in
**`agent-console-m1`**, the container `agentic.config.json` names as `console.container`, once it came
up. Both carry `claude 2.1.280`. The spike record says which container each command ran in, because a
spike that does not say where it ran is not evidence.

### Task 0.1 — prove `--session-id` is honored, and that the id is the transcript's name

**DONE, 2026-09-30.** Both answered yes: all six envelopes of the turn carried the minted id, and the
session record landed at `/root/.claude/projects/-workspace/<minted-id>.jsonl`. The measurement, with
the command and the verbatim output, is in `docs/spikes/prompt-surface-protocol.md` §1. So the
read-the-id-out-of-`init` fallback is not the first turn's path; Phase 2 keeps it only as the second
door it already describes, and a first turn that has to be retried mints a **fresh** id, because
`--session-id` is refused with `already in use` when a session record for that id exists.

The command, kept for reproducibility:

    ID=$(docker exec agent-console-m1 cat /proc/sys/kernel/random/uuid)
    docker exec -w /workspace agent-console-m1 claude -p "Reply with the single word: pong" \
      --session-id "$ID" --output-format stream-json --verbose --include-partial-messages

**Step 2 — check the init envelope's `session_id` equals `$ID`:**

    docker exec -w /workspace agent-console-m1 claude -p "..." --session-id "$ID" ... | head -3 | grep -o '"session_id":"[^"]*"'

Expected: the same id three times.

**Step 3 — check the transcript landed at that name:**

    docker exec agent-console-m1 ls -l /root/.claude/projects/-workspace/"$ID".jsonl

Expected: the file exists. **If it does not, the first-turn id must instead be read out of the `init`
envelope, and the composer must refuse a second turn until that reading has arrived** — record which
of the two happened, because Phase 2's session-minting task is written against it.

**Step 4 — record the exit status and how long the turn took.**

### Task 0.2 — prove `--resume` continues that conversation

**DONE, 2026-09-30.** Both answered: the resumed turn carried the same session id, and **it replayed
nothing** — 0 of turn one's 6 uuids reappeared in turn two's capture, of which 4 were new. The session
record grew across the resume even though the turn failed (42170 → 46951 bytes), which corroborates the
growth signal the existing replay guard in `src/app.rs` already uses. Measurement and verbatim output:
`docs/spikes/prompt-surface-protocol.md` §2.

The command, kept for reproducibility:

    docker exec -w /workspace agent-console-m1 claude --resume "$ID" -p "Reply with the single word: again" \
      --output-format stream-json --verbose --include-partial-messages

Record: the `session_id` in the envelopes (same id?), whether the resumed stream replays the earlier
turns' envelopes, and the session `.jsonl`'s new byte size. **The replay answer decides Task 4.2's
dedupe rule** (a resume that replays history would otherwise duplicate the whole transcript).

### Task 0.3 — settle the Phase 5 question (deferred decision, real evidence)

**DONE, 2026-09-30.** The measurement is in `docs/spikes/prompt-surface-protocol.md` §3: with a fresh
minted id the CLI accepted the stream-json stdin envelope, stayed alive between two envelopes written
ten seconds apart, and produced two complete turns (10 envelopes, 2 `result`s) in one child, exiting 1
only because both turns 429'd. The envelope shape that worked is recorded there, and the second turn's
own `init` envelope is now a pinned parser rule.

The command, kept for reproducibility:

    ( printf '%s\n' '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Reply with the single word: pong"}]}}'
      sleep 8
      printf '%s\n' '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Reply with the single word: again"}]}}'
    ) | docker exec -i -w /workspace agent-console-m1 claude -p \
          --input-format stream-json --output-format stream-json --verbose --include-partial-messages

Record verbatim: whether the CLI accepted that envelope shape (and its error if not), whether it stayed
alive between the lines, how many `result` envelopes came back, and whether `queued_turn_count` was
non-zero on either. Expected outcome for the recommendation: either the protocol works and Phase 5 is
schedulable, or the CLI exits after one turn and Option A is simply the design. Either answer is a
success for this task.

### Task 0.4 — freeze the envelopes into fixtures

**DONE, 2026-09-30.** Four fixtures are in `tests/fixtures/stream-json/`, cut from the captures above
and redacted minimally, and the builder that produces them sits beside them:

| file | envelopes/records | what it pins |
| --- | --- | --- |
| `one-shot.jsonl` | 6 envelopes | the first-turn sequence, hooks included |
| `resumed.jsonl` | 4 envelopes | a resume starts at `init` and replays nothing |
| `realtime-two-turns.jsonl` | 10 envelopes | the second-turn `init` rule |
| `session-record.jsonl` | 19 records | reconciliation by uuid |
| `build_fixtures.py` | — | the builder: `build_fixtures.py <captures-dir> <fixtures-dir>` |

Redaction replaces the session id with one fixed test id, replaces the SessionStart hook's output (the
real one carried this machine's injected memory layer), truncates message text to 120 characters, and
maps every uuid through a **shared** map keyed on the original — so the one correlation that matters
survives into the fixtures. The fixtures agree with §4's measurement: exactly one uuid is shared
between `one-shot.jsonl` and `session-record.jsonl`, and it is the assistant message. Re-running the
builder over the same captures reproduces all four files byte for byte (verified with `md5sum -c`),
which is what keeps a fixture from becoming a magic file. The raw captures are **not** committed.

### Task 0.5 — re-measure what the 429 blocked (a gate, not a nicety)

**Files:** append to `docs/spikes/prompt-surface-protocol.md`; add the new fixtures.

Every Phase 0 turn failed at the API with HTTP 429, so four things are still unmeasured and Phase 1
must not be called done without the first two: a successful `result` envelope's fields, the
`tool_use`/`tool_result` block shapes the parser's `Tool` and `Result` arms read, the
`--include-partial-messages` chunks the streaming cursor depends on, and a non-zero
`queued_turn_count`.

**Step 1 — wait for the account to answer, then run one turn that provokes a tool call and a write:**

    python3 -c 'import uuid; print(uuid.uuid4())'   # a fresh id: a used one is refused
    docker exec -w /workspace agent-console-m1 claude -p \
      "Run this exact command and quote its output: printf hello. Then reply DONE." \
      --session-id "<that id>" --output-format stream-json --verbose --include-partial-messages \
      --agent orchestrator --permission-mode acceptEdits

**Step 2 — capture the block shapes.** Record, verbatim, the `assistant` envelope carrying
`tool_use` and the `user` envelope carrying `tool_result`, including whether `tool_result` arrives on
a `user` envelope or elsewhere.

**Step 3 — cut the corpus into `tests/fixtures/stream-json/tool-turn.jsonl`** with the same shared-uuid
builder, and re-run it so the fixtures stay reproducible.

**Step 4 — delete the "not measured" rows this settles from the spike record's section 5 table**, so
the record never claims ignorance it no longer has.

**Verification for Phase 0:** the spike document exists, quotes real command output for 0.1–0.3, and
states which of the two session-id outcomes happened. Do not start Phase 1 until 0.1 and 0.2 have real
answers — they do — and do not call Phase 1 done until 0.5's tool-block capture is in.

---

## Phase 1 — the conversation model (pure, no processes, no UI)

**LANDED, 2026-09-30.** `src/conversation.rs` is in the tree, `pub mod conversation;` is registered in
`src/lib.rs`, `dump::render_conversation` carries the buffer into `--dump`, and 15 unit tests cover the
model and the parser against the fixtures. The whole suite is green (39 unit, 48 frame, 8 refresh), fmt
is clean, `clippy --release --all-targets -- -D warnings` is silent, and the conformance gate passes
(base 206 = current 206).

Three things the code changed about this phase, each because the plan's shape did not survive contact
with the parser:

* **`Item` carries two identities, not one.** The plan gave every item a single `uuid` serving as both
  the buffer's dedupe key and the reconciliation key. A tool call and its result are two lines about one
  `tool_use` id, so with one field the result rewrites the call's line -- caught by
  `a_tool_call_and_its_result_are_read_from_the_shapes_the_parser_expects` on its first run, not by
  review. `Item` now has `key: Option<String>` (the buffer's own identity: the envelope's uuid for a
  message, `call:<id>` for a call, `result:<id>` for what it returned) and `uuid: Option<String>` (the
  run's own identity for the message -- the only thing the session transcript can corroborate).
* **One session record can corroborate several lines.** So `confirm_from` does not consume its match: a
  message that produced a call and a result has three lines the one record names, and confirming the
  first and skipping the rest would be an ordering accident rather than a finding.
* **`--prompt TEXT` moves to Phase 2**, where `ActionKind::Prompt` exists to give it something to send.
  Task 1.5 lands its `--dump` half only: the transcript belongs to a running console, `--dump` has no
  session, and so it prints the buffer empty and says why in the buffer's own words.

Two shapes the parser recognises without drawing them, recorded rather than guessed at: `stream_event`
(the `--include-partial-messages` chunk) is counted in `partial_events` and never drawn per token, and a
`user` envelope carrying text is the `--replay-user-messages` echo of the console's own prompt, ignored
rather than appended twice. Neither chunk shape is measured against a successful turn yet -- Task 0.5's
gate still stands over the tool-block shapes, and the test that covers them says so in its own comment.

### Task 1.1 — create `src/conversation.rs` with the model

**LANDED, 2026-09-30**, with the two-identity `Item` correction above. The ring is `DEFAULT_RING = 400`
items, and `Conversation::dropped` counts what fell off the front so the header line can say it loudly
instead of a pane silently showing fewer lines than were written.

**Files:** create `src/conversation.rs`; modify `src/lib.rs` (add `pub mod conversation;` after
`pub mod config;`, keeping the module list alphabetical).

    //! The console's own transcript: what it sent, what came back, and which of those lines it has
    //! been able to confirm against the run's own session transcript.
    //!
    //! This is the one thing the console owns. Everything the other screens draw is a reading; this is
    //! a buffer, and it is drawn as a buffer: an item taken from the live stream is `Live` until the
    //! same item's uuid is found in the session's own `.jsonl`, at which point it is `Confirmed`.

    use std::collections::VecDeque;
    use std::time::SystemTime;

    /// Who a line in the transcript came from.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ItemKind {
        /// The operator's own words, as sent.
        You,
        /// The run's own words.
        Agent,
        /// A tool call the run made, as its own stream named it.
        Tool,
        /// What a tool returned, one line.
        Result,
        /// A line this console wrote: the argv it ran, a reconciliation count, an unknown envelope.
        Console,
        /// A refusal, in the console's own words.
        Refused,
    }

    /// How much this item is worth trusting.
    ///
    /// Three of these four are states of a claim about the run, and the fourth is not a claim at all.
    /// Phase 0 measured which stream envelopes the session's own record corroborates (an `assistant`
    /// or `user` envelope's uuid is the session record's uuid for that same message) and which it
    /// never will (`system/*` and `result` envelopes have no uuid-bearing record). So a console line
    /// built from a `system` envelope is not "a reading that failed to corroborate" -- it is the
    /// console's own line, and it says so.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ItemState {
        /// The run's own words, read from the live stream, not yet found in the session transcript.
        Live,
        /// The same uuid was found in the session's own transcript.
        Confirmed,
        /// This console's own line: an argv, a reconciliation count, a hook's outcome, an envelope it
        /// does not understand. Not a reading about the run and not confirmable by construction.
        Own,
        /// The stream said this failed, or the console could not read it.
        Failed,
    }

    /// One line of the transcript, with the identity the stream gave it.
    #[derive(Clone, Debug)]
    pub struct Item {
        /// The stream envelope's own uuid, when it carried one: the dedupe key, the reconciliation
        /// key, and the reason a partial message that arrives again updates its own item instead of
        /// appending a second one.
        pub uuid: Option<String>,
        pub kind: ItemKind,
        pub state: ItemState,
        pub text: String,
        /// The moment the console appended it. Not a reading's age: this is the buffer's own clock.
        pub at: SystemTime,
    }

    /// One send and everything that followed it, as the `result` envelope described it.
    #[derive(Clone, Debug, Default)]
    pub struct Turn {
        pub started: SystemTime,
        pub ended: Option<SystemTime>,
        pub stop_reason: Option<String>,
        pub num_turns: Option<u64>,
        pub cost_usd: Option<f64>,
        pub is_error: bool,
        pub error: Option<String>,
    }

    /// The console's transcript.
    #[derive(Debug)]
    pub struct Conversation {
        pub items: VecDeque<Item>,
        pub turns: Vec<Turn>,
        /// The session id, once the stream has named it.
        pub session: Option<String>,
        /// The model and the CLI version the `init` envelope reported, so the pane can say what it is
        /// talking to instead of assuming.
        pub model: Option<String>,
        pub cli_version: Option<String>,
        /// Items dropped off the front of the ring, so the pane can say so loudly rather than clip.
        pub dropped: usize,
        pub max_items: usize,
    }

**Step 2 — the accessors and the ring:**

    impl Conversation {
        pub fn new(max_items: usize) -> Conversation { /* VecDeque::new(), dropped: 0 */ }

        /// Append an item, dropping the oldest when the ring is full, and count what was dropped.
        pub fn push(&mut self, item: Item) {
            self.items.push_back(item);
            while self.items.len() > self.max_items {
                self.items.pop_front();
                self.dropped += 1;
            }
        }

        /// The console's own line: an argv, a refusal, a reconciliation count. `Own`, because a line
        /// this console wrote is not a claim about the run and could never be corroborated by it.
        pub fn note(&mut self, text: impl Into<String>) {
            self.push(Item { uuid: None, kind: ItemKind::Console, state: ItemState::Own,
                             text: text.into(), at: SystemTime::now() });
        }

        pub fn refuse(&mut self, text: impl Into<String>) { /* kind: Refused, state: Failed */ }

        /// How many items are the run's own words and still unconfirmed.
        pub fn live(&self) -> usize {
            self.items.iter().filter(|i| i.state == ItemState::Live).count()
        }

        pub fn confirmed(&self) -> usize {
            self.items.iter().filter(|i| i.state == ItemState::Confirmed).count()
        }

        /// How many items are this console's own lines: not a reading, and not confirmable.
        pub fn own(&self) -> usize {
            self.items.iter().filter(|i| i.state == ItemState::Own).count()
        }
    }

**Step 3 — the header line**, so every pane that draws it says the same words:

    /// One line naming the buffer's own state: what it is, how much of it the run's own record
    /// corroborates, and against which session. The console's provenance rule, applied to the one
    /// thing it owns.
    ///
    /// The three counts are separate on purpose. Phase 0 measured that only an `assistant` or `user`
    /// envelope's uuid is corroborated by the session record, so a console line is `Own` and stays
    /// `Own` -- folding it into "live" would read as a claim that failed to verify, when it was never
    /// a claim about the run at all.
    pub fn header_line(&self) -> String {
        let session = self.session.clone().unwrap_or_else(|| "no session named yet".to_string());
        let dropped = if self.dropped > 0 {
            format!("   {} earlier item(s) dropped (the {}-item ring)", self.dropped, self.max_items)
        } else { String::new() };
        format!("buffer: {} item(s) -- {} confirmed against {session}, {} live, {} this console's own{}",
                self.items.len(), self.confirmed(), self.live(), self.own(), dropped)
    }

**Step 4 — verify it compiles alone:**

    cargo check --offline

Expected: no errors, one or two `never used` warnings (the module is not wired up yet). Warnings are
fine here; `-D warnings` is a Phase 3 gate.

### Task 1.2 — the envelope parser

**LANDED, 2026-09-30.** `Conversation::apply_line` returns `Applied::{Item, Updated, TurnEnded,
Ignored, Unknown}` and the five measured rules hold as tests, not as comments: a repeated uuid updates
its own line (`Applied::Updated`), a later `init` adds nothing (asserted before/after around the second
`init` of `realtime-two-turns.jsonl`, not by a total that a later change would shift), `is_error` plus
`terminal_reason` decide the verdict and `subtype: "success"` is pinned as a trap, an unread type and a
non-JSON line are both reported as `Unknown` with a line in the transcript, and a `result` with no turn
open is reported rather than hung on an invented turn.

**Files:** modify `src/conversation.rs` (add `apply_line`), create the parser tests in the same file's
`#[cfg(test)] mod tests`.

The parser is `serde_json`-only and never panics on a line it does not understand:

    /// What one stream line did to the transcript.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Applied { Item, Updated, TurnEnded, Ignored, Unknown }

    impl Conversation {
        /// Apply one line of the run's own stream.
        ///
        /// The line is the CLI's, so an unknown shape is reported as unknown, with the type it carried
        /// and the CLI version this console is talking to -- never silently dropped, and never guessed
        /// at. A line that is not JSON at all is reported the same way.
        pub fn apply_line(&mut self, line: &str) -> Applied {
            let trimmed = line.trim();
            if trimmed.is_empty() { return Applied::Ignored; }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                self.note(format!("a stream line was not JSON and was not applied: {}",
                                  head(trimmed, 120)));
                return Applied::Unknown;
            };
            match value.get("type").and_then(serde_json::Value::as_str) {
                Some("system") => self.apply_system(&value),
                Some("assistant") => self.apply_assistant(&value),
                Some("user") => self.apply_user(&value),
                Some("result") => self.apply_result(&value),
                Some(other) => {
                    self.note(format!("a stream line of type '{other}' is not one this console reads \
                                       (claude {}); it was not applied",
                                      self.cli_version.clone().unwrap_or_else(|| "version unread".into())));
                    Applied::Unknown
                }
                None => { self.note("a stream line carried no type and was not applied"); Applied::Unknown }
            }
        }
    }

The arms, each with the fields it reads and nothing else:

| envelope | reads | produces |
| --- | --- | --- |
| `system` / `init` | `session_id`, `model`, `claude_code_version`, `permissionMode`, `cwd` | sets `session`, `model`, `cli_version`; **the first one** also adds a `Console` item naming what this console is talking to — a later one updates the fields and adds nothing (rule 4) |
| `system` / `hook_started` / `hook_response` | `hook_name`, `exit_code`, `outcome` | a `Console` item, one line per hook, dim |
| `system` / `status` | `status` | updates the turn's live status line, no item |
| `assistant` | `uuid`, `message.content[]` | for `type: "text"` an `Agent` item, keyed by `uuid`: a second arrival of the same `uuid` **replaces** its text (that is partial streaming, not a duplicate); for `type: "tool_use"` a `Tool` item naming the tool and its one-line argument |
| `user` | `uuid`, `message.content[]` | for `type: "tool_result"` a `Result` item, first line only, `Failed` when `is_error` |
| `result` | `stop_reason`, `num_turns`, `total_cost_usd`, `is_error`, `terminal_reason`, `result` | closes the turn: `TurnEnded` |

Three rules the tests must pin, and one Phase 0 measured:

1. **A repeated `uuid` updates its own item.** Feed the same assistant envelope twice with growing text
   → exactly one item, holding the longer text, `Applied::Updated`.
2. **An unknown type is reported, not dropped.** Feed `{"type":"whatever","session_id":"x"}` → one
   `Console` item containing the word `whatever`, and `Applied::Unknown`.
3. **A non-JSON line is reported.** Feed `not json` → one `Console` item, `Applied::Unknown`, no panic.
4. **A second `init` updates the session and touches no items** (measured: a persistent stream re-emits
   `init` on every turn). Feed `realtime-two-turns.jsonl` → the item count equals the items the two
   turns' *content* envelopes produced, the `session`/`model`/`cli_version` fields hold the newest
   values, and no item was added by either `init`. Without this rule a persistent stream would reset or
   duplicate its own transcript on every turn, which is exactly the bug the two-turn fixture exists to
   catch.
5. **A turn's verdict comes from `is_error` and `terminal_reason`, never from `subtype`.** Phase 0
   measured a 429'd turn whose envelope reads `"subtype":"success"` with `"is_error":true` and
   `"terminal_reason":"api_error"` (`docs/spikes/prompt-surface-protocol.md` §4b). A parser that keys on
   `subtype` would draw a rate-limited turn as a green one. Pin it with a fixture line that carries that
   exact combination and assert the turn closes as a **failure**, with the API's own message in a
   `Failed` item.

**Verify:**

    cargo test --offline conversation:: 2>&1 | tail -5

Expected: the parser tests pass; the suite's total rises by the number of tests written.

### Task 1.3 — `open_turn`, `close_turn`, and the prompt echo

**LANDED, 2026-09-30.** A turn closes once and through one of two doors: the run's own `result`
envelope, or `close_turn` for a child that died without one. Whichever arrives second changes nothing --
both orders are pinned by `a_turn_closes_once_whichever_door_closes_it`. The prompt echo is ignored on
the `--replay-user-messages` shape rather than matched by text, because a text comparison would have to
guess at whitespace the CLI may not preserve.

**Files:** modify `src/conversation.rs`.

    impl Conversation {
        /// Start a turn: record the operator's own words, then open the turn the stream will close.
        pub fn open_turn(&mut self, prompt: &str) {
            self.push(Item { uuid: None, kind: ItemKind::You, state: ItemState::Live,
                             text: prompt.to_string(), at: SystemTime::now() });
            self.turns.push(Turn { started: SystemTime::now(), ..Turn::default() });
        }

        /// Close the newest turn with how the child actually ended. Called from the exit path, so a
        /// turn that died without a `result` envelope is still closed -- with the exit word, not with
        /// a stop reason the console invented.
        pub fn close_turn(&mut self, word: &str) {
            let Some(turn) = self.turns.last_mut() else { return };
            if turn.ended.is_some() { return; }
            turn.ended = Some(SystemTime::now());
            turn.error = Some(word.to_string());
            self.note(format!("the turn ended: {word}"));
        }
    }

**Note for the implementer:** `apply_result` closes the turn from the `result` envelope; `close_turn`
is the other door, for a child that died. Both leave `Turn::ended` set, and neither overwrites the
other — that is what the `if turn.ended.is_some() { return; }` is for. Test both orders (envelope then
exit, exit then envelope) and assert one `ended`.

### Task 1.4 — reconciliation against the session transcript

**LANDED, 2026-09-30.** `confirm_from(&[String]) -> usize` reads the session transcript's `uuid` fields
and flips matching `Live` items to `Confirmed`, leaving every `Own` line alone. The fixture shares
exactly one uuid with `one-shot.jsonl`, so the test asserts exactly one confirmation, and a read that
matches nothing says in the transcript that the read is a bounded tail rather than letting the operator
read an unconfirmed claim as a doubtful one.

**Files:** modify `src/conversation.rs`; add tests.

    /// Confirm every item the session's own transcript carries.
    ///
    /// Only the run's own words can be confirmed, and Phase 0 measured exactly which: an `assistant`
    /// or `user` envelope's uuid is the session record's uuid for that same message, while
    /// `system/*` and `result` envelopes have no uuid-bearing record at all. So this walks the items
    /// that have a uuid and leaves everything else alone -- a console line is `Own` and is not
    /// "waiting" for anything.
    ///
    /// Returns how many items this confirmed. An item whose uuid is not in the transcript stays
    /// `Live`: a claim the run's own record does not carry is a claim this console has only its own
    /// word for, and it says so by staying live.
    pub fn confirm_from(&mut self, jsonl_lines: &[String]) -> usize {
        let mut seen = std::collections::HashSet::new();
        for line in jsonl_lines {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(uuid) = value.get("uuid").and_then(serde_json::Value::as_str) {
                    seen.insert(uuid.to_string());
                }
            }
        }
        let mut confirmed = 0;
        for item in self.items.iter_mut() {
            if let Some(uuid) = &item.uuid {
                if item.state == ItemState::Live && seen.contains(uuid) {
                    item.state = ItemState::Confirmed;
                    seen.remove(uuid);
                    confirmed += 1;
                }
            }
        }
        if confirmed > 0 {
            self.note(format!("reconciled: {confirmed} item(s) matched the session transcript"));
        }
        confirmed
    }

**Pitfall to encode in the test:** the session transcript is read as a *capped tail*
(`probe::TRANSCRIPT_TAIL_BYTES`, 256 KiB, scanned for `TRANSCRIPT_TAIL_LINES`). A long conversation
will therefore not confirm its oldest items. That is correct behaviour and must be stated: the pane
prints `N item(s) live (the transcript read is a bounded tail)`, not `N item(s) unverified`. Write that
sentence into `confirm_from`'s follow-up note when `confirmed == 0 && self.live() > 0`.

### Task 1.5 — `--dump` carries the conversation

**LANDED (the `--dump` half), 2026-09-30.** `dump::render_conversation` appends a TRANSCRIPT section
that names it as the console's own buffer rather than a reading, prints the header line's three counts,
and says in words why it is empty. Verified by running the binary:

    == TRANSCRIPT (this console's own buffer, not a reading) ==
      buffer: 0 item(s) -- 0 confirmed against no session named yet, 0 live, 0 this console's own
      no turn has been sent from this console: --dump reads the repository and exits.

The `--prompt TEXT` half moved to Phase 2 with `ActionKind::Prompt`.

**Files:** modify `src/dump.rs` (add `render_conversation(&mut out, snapshot)` called after
`render_live`, or a new `render_conversation(out, conversation)` that `main.rs` calls, since the dump's
signature takes a `Snapshot` and the conversation lives on the `App`).

**Design decision, made here to avoid a signature fight:** `dump::render` keeps its signature
(`&Config, &Snapshot`). The conversation is *not* part of a `Snapshot` — it is not a reading, and
putting it in the snapshot would be exactly the confusion this plan is avoiding. Instead:

    // src/dump.rs
    /// The console's own transcript, printed as what it is: this console's buffer, not the run's
    /// record. `--dump` has no session, so it prints the empty buffer's own state and says why.
    pub fn render_conversation(out: &mut String, conversation: &crate::conversation::Conversation) {
        out.push_str("\n== CONVERSATION (this console's own buffer, not a reading) ==\n");
        out.push_str(&format!("state       : {}\n", conversation.header_line()));
        out.push_str(&format!("items       : {}\n", conversation.items.len()));
        for item in &conversation.items {
            out.push_str(&format!("  {:?}/{:<9} {}\n", item.state, format!("{:?}", item.kind), item.text));
        }
        for (index, turn) in conversation.turns.iter().enumerate() {
            out.push_str(&format!("turn {}      : stop {:?} turns {:?} cost {:?}\n",
                                  index + 1, turn.stop_reason, turn.num_turns, turn.cost_usd));
        }
    }

and a new flag `--prompt TEXT` in `main.rs` that sends one turn and prints the transcript as it
arrives, for a script and for the end-to-end test:

    --prompt TEXT          send one prompt to the orchestrator and print the turn as JSONL on stdout,
                           then the console's own transcript; the same argv the composer would build

**Verify:**

    cargo run --release -- --repo . --dump | grep -A3 "CONVERSATION"
    cargo run --release -- --help | grep -- --prompt

Expected: the heading, and the flag in the usage text.

---

## Phase 2 — the send path

**LANDED, 2026-09-30** — all six tasks, with the corrections recorded per task below. The end-to-end
proof is in "What the first real turn measured" at the end of this phase.

### Task 2.1 — mint a session id with no new dependency

**Files:** create `src/uuid.rs`; modify `src/lib.rs`.

`claude --session-id` requires a valid UUID. The crate has three dependencies and this is not a reason
for a fourth: v4 from `/dev/urandom` is a dozen lines, and the shape can be checked with the validator
the crate already owns (`probe::uuid_token`).

    //! A v4 UUID, for the one thing this console has to mint: the id of a conversation it starts.
    //!
    //! `claude --session-id` requires a valid UUID, and this crate's dependency list is part of what it
    //! promises (`Cargo.toml`), so this is done here rather than by adding a crate.

    use std::io::Read;

    /// A random v4 UUID, read from the kernel's entropy pool.
    pub fn v4() -> Result<String, String> {
        let mut bytes = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .map_err(|error| format!("/dev/urandom: {error}"))?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
        bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
        Ok(format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ))
    }

**Test, using the crate's own validator** (`probe::uuid_token` checks exactly the 8-4-4-4-12 hex shape
and rejects anything else): two calls differ, and both survive `uuid_token`. The `Err` path is
unreachable on Linux and is not tested — say so in the test's own comment rather than writing a
mock-filesystem test that proves nothing.

### Task 2.2 — `ActionKind::Prompt`

**Files:** modify `src/actions.rs`.

    /// A prompt sent to the orchestrator, as a turn of one conversation.
    ///
    /// The first turn mints the conversation's id and starts it; every later turn resumes that id. The
    /// argv is the brief's shape plus the streaming flags, so the console reads the run's own words as
    /// events rather than as prose.
    Prompt,

Add to `id()` (`"prompt"`), `title()` (`"Prompt the orchestrator (a turn of this conversation)"`),
`value_hint()` (`"the prompt: what the orchestrator should do next"`), `needs_value()` (true), and
`example_value()` (`"Change request for this repository, in one sentence: …"`).

`all_kinds()` becomes `[ActionKind; 8]` with `Prompt` last, and `docker_exec_kinds()` becomes
`[ActionKind; 5]`. **Every test that asserts either array's length must be updated in this task** — the
point of those two arrays is that a new action cannot be added without a test noticing, so the tests
are updated deliberately, not deleted.

The argv, in `build`:

    ActionKind::Prompt => {
        if value.is_empty() { return Err("the prompt is empty; write it first".to_string()); }
        if value.chars().count() > 32_000 { /* same refusal as Brief, same wording */ }
        if guards.in_flight {
            return Err("refused: an orchestrated run is already in flight in this container. A second \
                        run would share the workspace and the journals with it. Approve the checkpoint \
                        or wait for the run to end.".to_string());
        }
        let mut command = vec![cfg.console.claude_command.clone()];
        match &guards.prompt.session {
            // A later turn: resume the one conversation this console started, by id.
            Some(session) => {
                command.push("--resume".to_string());
                command.push(session.clone());
                command.push("-p".to_string());
                command.push(value.to_string());
                command.extend(cfg.console.claude_flags.iter()
                    .filter(|flag| !matches!(flag.as_str(), "--agent" | "orchestrator"))
                    .cloned());
            }
            // The first turn: mint the id, so the console knows which conversation it is from here.
            None => {
                command.push("--session-id".to_string());
                command.push(guards.prompt.minted.clone()
                    .ok_or_else(|| "refused: no session id was minted for this turn".to_string())?);
                command.push("-p".to_string());
                command.push(value.to_string());
                command.extend(cfg.console.claude_flags.clone());
            }
        }
        command.extend(cfg.console.conversation.stream_flags.clone());
        let argv = docker_exec_in_workspace(cfg, &command);
        // ... Command { kind, guards, value, argv, cwd, env: Vec::new(), writes: Vec::new(), note }
    }

with the note lines, in this order:

    "streamed as events: --output-format stream-json, so the run's own words arrive as they are written"
    "the session id comes from console.conversation or from `--session-id <minted uuid>`"
    "resumed by id, never by `--continue`: `--continue` resumes whichever session happens to be newest"

### Task 2.3 — `Guards` carries the conversation target

**Files:** modify `src/actions.rs` (`Guards`, `Default`, `summary`), `src/app.rs` (`App::guards()`),
`src/actions.rs` and `src/state.rs` call sites of `dry_run_all` / `dry_run_one`, `tests/frames.rs`.

    /// The conversation this console is driving: its own state, not a reading.
    ///
    /// It sits in `Guards` because that is what the command builder reads, and it is the one field
    /// here that is not a reading. The confirmation screen says so on its own line, and `--dry-run`
    /// prints it as the console's own, so nothing that carries a guards summary can present this as
    /// something the console observed.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct PromptTarget {
        /// The session every turn after the first resumes.
        pub session: Option<String>,
        /// The id minted for the first turn, when there is one.
        pub minted: Option<String>,
    }

Add `pub prompt: PromptTarget` to `Guards`, `PromptTarget::default()` in `Guards::default()`, and one
line to `summary()`:

    format!("{}{}", ..., if self.prompt.session.is_some() || self.prompt.minted.is_some() {
        " prompt=<this console's own conversation target>" } else { "" })

**Verify:** `cargo test --release --offline` — the existing 52 tests must still pass before any new
test is written, which is the check that the field was added to every construction site.

### Task 2.4 — the app's send path

**Files:** modify `src/app.rs`.

    impl App {
        /// The conversation this console is driving.
        pub fn conversation(&self) -> &Conversation { &self.conversation }

        /// Send one turn: open the turn, build the command, and put it on the confirmation screen.
        ///
        /// Nothing is started here. The operator's `y` on the confirmation screen is still the only
        /// door to a running command, exactly as it is for every other action.
        pub fn propose_prompt(&mut self) {
            let text = std::mem::take(&mut self.composer);
            if text.trim().is_empty() {
                self.status = "the prompt is empty; write it first".to_string();
                return;
            }
            self.prompt_text = text;
            self.propose(ActionKind::Prompt, self.prompt_text.clone());
        }
    }

`App` gains: `pub conversation: Conversation`, `pub composer: String`, `prompt_text: String`, and
`pub view: Screen` (Phase 3). The first turn mints its id when the command is *built*, not when it is
confirmed, so the confirm screen shows the exact id:

    fn guards(&self) -> Guards {
        Guards {
            in_flight: self.snapshot.live.run.in_flight,
            checkpoint: self.snapshot.live.checkpoint.state,
            session: self.snapshot.live.checkpoint.session.clone(),
            checkpoint_conflict: self.snapshot.live.checkpoint.checkpoint_conflict.clone(),
            evaluated: true,
            prompt: PromptTarget {
                session: self.conversation.session.clone(),
                minted: self.minted.clone(),
            },
        }
    }

and in `propose_prompt`, before `propose`: `if self.conversation.session.is_none() && self.minted.is_none()
{ self.minted = Some(crate::uuid::v4()?) }` — with the error surfaced as a refusal in the composer, not
a panic (`uuid::v4()` failing means `/dev/urandom` is unreadable, which the operator should read as
exactly that).

### Task 2.5 — route the turn's output into the conversation

**Files:** modify `src/app.rs` (`on_confirm`, `drain_running`).

One rule, in `drain_running`: a `Prompt` action's stream lines are the conversation's, not the log's.

    for line in lines {
        if action.kind == ActionKind::Prompt {
            self.conversation.apply_line(&line);
        } else {
            self.note(line);
        }
    }

and in `on_confirm`'s `Ok(started)` arm, for `ActionKind::Prompt`: `self.conversation.open_turn(&command.value);`
plus a `Conversation.note` of the exact argv (so the transcript itself carries what was run), and on the
exit path (`drain_running`'s `Ok(Some(status))`) `self.conversation.close_turn(&word)`.

Also: the first turn's `init` envelope names the session, so `on_confirm` stores
`self.conversation.session` as soon as the stream provides it — `apply_line` returning `Applied::Item`
from an `init` envelope is the trigger; simplest correct form is to re-read
`self.conversation.session.clone()` into `self.minted = None` after every `drain_running`, so the
second turn resumes the id the run actually used. This is the one place where Task 0.1's answer decides
the code: if `--session-id` is honored, `conversation.session` and `minted` are the same string and the
line is a no-op; if it is not, the read-out-of-`init` path is what makes turn two possible.

### Task 2.6 — refusals and the never-a-single-key rule for the send path

**Files:** modify `src/app.rs`; tests in `tests/frames.rs` / `tests/refresh.rs`.

Refusals that must exist, each with its own test:

| situation | refusal |
| --- | --- |
| the composer is empty | "the prompt is empty; write it first" |
| a run is in flight in the container | `Brief`'s existing wording, verbatim |
| the console's own child is still alive | a new one: "this console's own turn is still running (pid N, el N) -- a second turn would be a second writer on session \<short-id\>" |
| `/dev/urandom` unreadable | "no session id could be minted: /dev/urandom: \<errno\>; nothing was sent" |
| an unknown action id / a refusal from `build` | pass the builder's own reason through, unchanged |

**What landed, and the four corrections this phase's measurements forced:**

* **2.1 — `src/uuid.rs`** (72 lines, `pub mod uuid;` in `lib.rs`), three tests rather than the two
  sketched: two mints differ **and** both survive `probe::uuid_token` (so a mint is a shape the CLI
  accepts), the version nibble is `4` and the variant nibble is RFC 4122 (`8|9|a|b`), and the `Err`
  path is documented as unreachable rather than mocked. Measured: the mint survived `--session-id`
  against the live CLI, and all six envelopes of the turn carried it.

* **2.2 — `ActionKind::Prompt`**, eighth in `all_kinds()`, fifth in `docker_exec_kinds()`. Two
  corrections: the no-mint refusal is **"refused: this would be the first turn of a new conversation
  and no session id was minted for it"** (the sketch's wording named the turn, not the situation), and
  `example_value()` is `"Run the fmt gate and report what it says."` The note list is **four** lines,
  not three: the streaming note names the flags it actually read, the resume/mint note is
  branch-dependent (one line, whichever branch this turn is), and a fourth was added deliberately —
  *"this console writes nothing into the repository; the agent this turn drives can, and what it may
  touch is decided by its own permission mode (console.claude_flags)"*. That fourth line is not
  decoration: the doctrine sentence this console is built around is read-only **toward the repository**,
  and a turn is the one action that makes an agent write. The confirmation says so in the console's own
  words before anything runs.

* **2.3 — `PromptTarget { session, minted }`** in `Guards`, empty in `Default`, and the summary clause
  is `prompt=<the console's own session target; not a reading>`, printed whenever either field is set.
  Five construction sites updated: `App::guards()`, `dry_run_all`, `dry_run_one`, the checkpoint card's
  ruling preview, and three in `tests/frames.rs`.

* **2.4 — the send path**: `App::propose_prompt` mints **before** building, so the confirmation screen
  shows the exact id the turn will carry; `turn_in_flight()` refuses a second turn on one session by
  name (`pid`, elapsed, short session id); `on_confirm` opens the turn and records the argv **only after
  the spawn succeeds**, so a spawn that fails leaves no turn behind to close.

* **2.5 — routing**: a turn's lines go to the transcript and everything else to the action log. Measured
  against a real child and a real stream in `a_turns_stream_lands_in_the_transcript_and_not_in_the_action_log`:
  the log's own line for a turn is its argv label (`-- prompt: … exited with code 0`) and never an
  envelope; the run's `init` adopts the session; the mint is retired once the run has named it.

* **2.6 — refusals**: four of the five rows are tested. The fifth — `/dev/urandom` unreadable — cannot
  be reached from a test on Linux, which is Task 2.1's own rule applied again: it is documented in the
  code rather than mocked. **This gap is deliberate and recorded here rather than hidden by a test that
  proves nothing.**

* **New, and not in the plan:** `dry_run_line` mints a throwaway session id **for the preview alone**
  when the action is `prompt` and the guards carry no target. Without it, the one action whose shape *is*
  the minted id could never be previewed — `--dry-run-action prompt` refused with "no session id was
  minted for it" and printed no argv at all. The preview prints its own line saying the id was minted for
  the preview and nothing was sent, and the guards clause says the target is not a reading. A throwaway
  random id is a side effect of nothing: nothing is started and nothing is written.

**What the first real turn measured** (2026-09-30, against `agent-console-m1` and `claude 2.1.280`):

    ./target/release/agentic-console --repo . --prompt "Do not use any tools. Reply with exactly: pong"

The account is still rate-limited, so this turn **failed** — and a failed turn through the real path is
the more valuable measurement, because it tests the trap the spike pinned:

* the minted id was honoured: `hook_started`, `hook_response`, `init`, `status`, `assistant` and
  `result` all carried `d3028318-92f4-4409-93da-1601ec43513a`, and `init` carried
  `agents: [… "orchestrator" …]` — so `--agent orchestrator` reached the CLI as configured;
* the `result` envelope said **`"subtype":"success"` with `"is_error":true`** and
  `terminal_reason: "api_error"` — and the console drew it as a failure, in the run's own words
  (`[Agent/Live] API Error: Request rejected (429) …`) and in its own
  (`[Refused/Failed] the turn failed (api_error): …`). A parser keyed on `subtype` would have drawn a
  rate-limited turn as a green one. **Rule 5 is now pinned by a real turn, not only by a fixture line.**
* the transcript's own header is the doctrine in one line:
  `buffer: 6 item(s) -- 0 confirmed against d3028318-…, 1 live, 4 this console's own`. Nothing claims to
  have been confirmed, because nothing has been read back yet — Phase 4 is what does that;
* the console exited `0` while reporting a failed turn: a failed reading is still a reading, and the exit
  status of the console is not the exit status of the run.

**Verify (all of Phase 2):**

    cargo fmt --check
    cargo clippy --release --all-targets --offline -- -D warnings
    cargo test --release --offline
    cargo run --release -- --repo . --dry-run-action prompt --value "say pong and stop"

Expected: clean fmt, no clippy output, tests pass (52 + the new ones), and the dry run prints the exact
`docker exec … --session-id <uuid> … --output-format stream-json …` line without running anything.

**Measured, 2026-09-30:** fmt clean, clippy silent under `-D warnings`, and the suite runs
**106 tests — 42 unit, 56 frame, 8 refresh, 0 failed** (the plan's "52" was already stale: the docs'
test count is the defect recorded in Task 3.7). The dry run prints:

    command  : docker exec -w /workspace agent-console-m1 claude --session-id <uuid> -p 'say pong and
               stop' --agent orchestrator --permission-mode acceptEdits --output-format stream-json
               --verbose --include-partial-messages
    note     : the id above was minted for this preview alone; nothing was sent

with `prompt=<the console's own session target; not a reading>` on the guards line, and
`scripts/run-conformance-gate.py` still passes (`"verdict": "pass"`, 12 files checked).

---

## Phase 3 — the CONVERSATION screen (the visual change)

**LANDED, 2026-09-30** — all seven tasks. CONVERSATION is the primary screen; FLOW, LIVE and INSPECT are
opaque overlays in front of it; the frame tests render every screen at the six sizes the design targets.
Measured on the final tree: `cargo fmt --check` clean, `cargo clippy --release --all-targets --offline --
-D warnings` finished with zero warnings, `cargo test --release --offline` → **120 passed, 0 failed** (48
unit, 64 frame, 8 refresh), `scripts/run-conformance-gate.py` → `"verdict": "pass"` with AGENTS.md's
citation count *down* 6 → 4 (both `Cargo.toml:5` citations were re-pointed at the sentence that moved, and
they resolve as a pair).

### What this phase's measurements forced (four corrections)

1. **The footer's key line has never been drawn.** `Constraint::Length(2)` with `Borders::TOP` leaves one
   row for content, so the paragraph's *second* line — the key line, the one the docs promise — was clipped
   by the border for this console's entire life. No test had ever looked for it: the pre-existing suite
   asserts the Help modal's key table, never the footer's line. The footer is now **3 rows** (border,
   checkpoint line, key line) in both the legacy and overlay compositions, and a new test asserts the key
   line is on the glass at a normal height *and* that the row it gives up below 24 rows says so loudly.
2. **An overlay was translucent.** A `Paragraph` writes only the cells it has text for, so drawn over a
   conversation with content in it, the conversation's own words and the composer's draft showed *through*
   the gaps — text the overlay never wrote, read as if it had. Found on glass in the smoke, not by the
   frame tests: the fix (`frame.render_widget(Clear, area)` before the overlay, in `ui::draw`) was verified
   by re-running the smoke, and the test that pins it is honest about being unable to reproduce the smear
   in a `TestBackend` — a `TestBackend` diffs against its own buffer and a live terminal diffs against the
   terminal's, which is where the stale cells came from. The test asserts the *intent* (nothing the
   conversation wrote may appear in an overlay frame); the smoke is the evidence for the artifact.
3. **The confirmation box was one row shorter than its own content**, so the "press y" question fell off
   the bottom of the box. `composer_rows` now measures with the very lines the box draws.
4. **The composer's draft must survive the confirmation.** `propose_prompt` originally `mem::take`-d the
   text, which meant the confirmation could not show the operator the sentence they were about to send —
   the exact thing the box exists to show. The draft now stays in the composer until the turn actually
   starts, so the box shows it and `n` returns to it with the text intact.

Two stale counts the move also fixed, found by the gate's own doc-vs-code comparison:
`docs/architecture.md` and `AGENTS.md` both claimed "52 tests" (they described the suite before the
conversation model landed); both now carry the measured 120.

### Task 3.1 — `Screen` and the overlay refactor, with the existing frame tests untouched

**LANDED, 2026-09-30.** `Screen` is the four-variant enum and `ui::draw_overlay` is the preservation trick: all 41 pre-existing frame assertions pass, and the only test change is mechanical (the render helper names its screen; the tests that relied on the old FLOW default set `app.view`). The one key-model assertion that legitimately moved is the Tab cycle — four screens now, so Tab from INSPECT returns to CONVERSATION instead of FLOW — and it says so.

**Files:** modify `src/app.rs` (`Tab` → `Screen`), `src/ui/mod.rs`, `tests/frames.rs`,
`tests/refresh.rs`.

    /// Which screen the console is showing.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Screen {
        /// The primary screen: the transcript and the composer.
        Conversation,
        /// The three screens this console shipped with, now overlays over the conversation.
        Flow,
        Live,
        Inspect,
    }

**The preservation trick, stated plainly:** `ui::draw_overlay(frame, area, app, screen)` renders
exactly what `ui::draw` renders today for that tab, into whatever `area` it is given. The existing 41
frame tests then call `ui::draw_overlay(frame, frame.area(), app, Screen::Flow)` instead of
`app.tab = Tab::Flow; ui::draw(...)`, with **no assertion changed**. An overlay is a full-screen overlay:
it is not a smaller canvas, it is the same drawing in front of a different screen. A test edited to fit
a change is no longer a regression test, and this refactor does not require editing one.

`ui::draw` becomes:

    pub fn draw(frame: &mut Frame, app: &App) {
        // ... layout unchanged: header, body, detail, log, footer ...
        match app.view {
            Screen::Conversation => conversation::draw(frame, chunks[1], app),
            other => { conversation::draw(frame, chunks[1], app); draw_overlay(frame, area, app, other); }
        }
        // ...
    }

**Verify:** `cargo test --release --offline` — unchanged count, unchanged results. This task is the
proof that the visual change is additive.

### Task 3.2 — the transcript pane

**LANDED, 2026-09-30.** `src/ui/conversation.rs`: the 15-column gutter carrying kind and state, `wrap` with the continuation indented, the ring's drop announced as an item rather than hidden, and a `▌` on the newest item while a turn is open.

**Files:** create `src/ui/conversation.rs`; modify `src/ui/mod.rs` (add `pub mod conversation;`).

    //! The CONVERSATION screen: this console's own transcript, and the composer under it.
    //!
    //! Everything here is the console's, and the pane says so: the header names the buffer, the item
    //! count and how many items the run's own transcript has confirmed. Nothing on this screen is a
    //! probe reading, which is why it is the one pane that does not print a source age.

Layout: gutter (12 columns) + text, one item to a run of rows, wrapped at the pane's width.

| kind | gutter | style | notes |
| --- | --- | --- | --- |
| `You` | `you` | white, bold | the operator's own words, echoed by `open_turn` |
| `Agent` | `orchestrator` | default | a `▌` cursor while the turn is open and the item is the last one |
| `Tool` | `tool` | yellow | `Name(one-line argument)`, argument clipped per `style::clip_row` |
| `Result` | `result` | dim; red when `Failed` | first line of the tool's output only |
| `Console` | `console` | dim | argv, reconciliation counts, unknown envelopes |
| `Refused` | `refused` | red, bold | in the console's own words |

Item state is part of the drawing, not decoration: `Live` items draw a leading `·`, `Confirmed` a
leading `✓`, `Failed` a leading `!`. A `Live` **agent** item is what the operator is watching stream; a
`Confirmed` one is what the run's own transcript corroborates. Both are readable at 80 columns, which
is the design target.

New style helpers, in `src/ui/style.rs`:

    pub fn you() -> Style        // white, bold
    pub fn agent() -> Style      // default
    pub fn tool() -> Style       // yellow
    pub fn result() -> Style     // dark gray
    pub fn console() -> Style    // dark gray
    pub fn refused() -> Style    // red, bold
    pub fn streaming() -> Style  // cyan, bold -- the `▌` cursor

### Task 3.3 — the composer, and the key model that keeps the doctrine

**LANDED, 2026-09-30**, with corrections 3 and 4 above: the confirmation draws in the composer box itself (not the modal), the box is measured with its own lines, and the draft survives the confirmation until the turn actually starts.

**Files:** modify `src/app.rs` (`Mode`), `src/ui/conversation.rs`, `src/ui/mod.rs` (KEY_REFERENCE).

    pub enum Mode {
        Normal,
        /// The composer has the keyboard: printable keys are text until Esc.
        Composer { cursor: usize },
        /// The exact command the prompt becomes, waiting for `y`.
        PromptConfirm { command: Box<Command> },
        // ... the existing Menu, Choose, Input, Confirm, Help
    }

The key model, in a table because it is the contract:

| when | key | what happens |
| --- | --- | --- |
| `Screen::Conversation`, `Mode::Normal` | `i`, `/` | focus the composer (`Mode::Composer`) |
| | every other key | **exactly as today** — `q`, `1/2/3`, `Tab`, `j/k`, `g`, `r`, `a`, `e`, `t`, `L`, `?` |
| `Mode::Composer` | any printable character | appended to the composer |
| | `Backspace` | pops |
| | `Ctrl-U` | clears (the whole line, no partial-line editing in v1) |
| | `Enter` | `propose_prompt`: the exact argv goes into the composer box, `Mode::PromptConfirm` |
| | `Esc` | back to `Normal`, the text kept |
| `Mode::PromptConfirm` | `y`, `Enter` | run it |
| | `n`, `Esc` | back to the composer, text intact |
| | `1/2/3`, `Tab` | still switch screens — the promise that a screen is never more than one keystroke away |

A prompt therefore costs `i` → type → `Enter` → `y`. No keypress reaches a command, and the composer box
is the confirmation screen: in `Mode::PromptConfirm` its second row is the exact argv
(`command.display()`), its third is `guards.summary()`, and its fourth is the note lines — the same four
things the modal `Mode::Confirm` shows today, in the place the operator is already looking.

**Pitfall, and the test that pins it:** `q` quit the console in `Mode::Normal` and must keep doing so;
in `Mode::Composer` a literal `q` is text. Write the frame test that asserts both, because this is
exactly the class of bug that silently types into the user's work.

### Task 3.4 — width discipline, the size matrix, and loud dropping

**LANDED, 2026-09-30**, plus correction 1: the matrix is real (80x24, 86x38, 100x30, 120x40, 160x50, 230x60), and the footer needed a third row before the key line the docs promise could exist at all.

**Files:** modify `src/ui/conversation.rs`, `src/ui/mod.rs`; tests in `tests/frames.rs`.

- The transcript pane wraps at its own width; the composer is **one column at every width** and never
  gains a second field.
- Above 160 columns the pane gains a right gutter carrying the last `result` envelope's own numbers
  (turns, tokens, cost) — the header, not the transcript, is where extra width goes.
- Below 24 rows the footer's key line is dropped **loudly**: the row it frees prints
  `… the key line is hidden at this height (press ? )`. Silent clipping is a lie the operator cannot
  detect.
- The ring's drop notice is an item, not a truncation: `… 240 earlier item(s) dropped (the 400-item
  ring)`.

Frame tests, at the roadmap's T1.8 matrix plus the fixture's existing wide sizes:

    80x24, 86x38, 100x30, 120x40, 160x50, 230x60

with three assertions per size: the composer is on the last rows; the transcript pane has at least 8
rows; and a long item wraps rather than being cut mid-word (`style::clip_row`'s ellipsis rule, applied
per row).

**This is the task that fixes the defect the roadmap names**: the suite rendered only at 230x60 and
200x50, so no test had ever exercised the width an operator actually has. The new screen does not
inherit that.

### Task 3.5 — the overlays, and what happens to the old tabs' keys

**LANDED, 2026-09-30**, plus correction 2: the three screens kept their existing keys, became opaque overlays with their own title row, and `Esc` closes one back to the conversation.

**Files:** modify `src/ui/mod.rs`, `docs/keys.md`, `docs/screens.md`.

- `1` / `2` / `3` open FLOW / LIVE / INSPECT as overlays; `Tab` / `Shift-Tab` cycle; `Esc` closes back
  to CONVERSATION.
- The overlay draws the screen at the full frame minus one row of its own title bar, so nothing in
  `flow.rs`, `live.rs` or `inspect.rs` changes.
- `KEY_REFERENCE` gains a CONVERSATION section: `i`/`/` focus, `Enter` review, `p` … and the overlay
  section says which keys are the overlay's own.

### Task 3.6 — the live smoke, driven for real

**LANDED, 2026-09-30.** Driven in a throwaway tmux session on its own socket — *not* the `ac` session on the `agentic-console` socket, which is attached to the operator's own terminal: a stray keystroke into that one opened a confirmation modal in front of them, cancelled with `Esc`, nothing run. Captured on glass: the frame as drawn, the composer holding the draft, the confirmation showing `--session-id <minted>` exactly, the turn arriving line by line (and drawn as a **failure** when the account's rate limit refused it), and the LIVE overlay over a transcript followed by `Esc` bringing the conversation back intact.

**Verify (this is the acceptance test for the whole visual change):**

    ./open.sh
    tmux -L agentic-console capture-pane -pt ac          # the frame as drawn
    tmux -L agentic-console send-keys -t ac i
    tmux -L agentic-console send-keys -t ac "run the fmt gate and report the result"
    tmux -L agentic-console send-keys -t ac Enter
    tmux -L agentic-console capture-pane -pt ac          # the exact argv, on the confirmation row
    tmux -L agentic-console send-keys -t ac y
    tmux -L agentic-console capture-pane -pt ac          # the turn streaming in

Expected: the transcript fills item by item, the composer keeps its line, the header's buffer line
counts up, and the checkpoint footer keeps saying what it said before. A frame assertion does not prove
a screen draws; this step is where a real run is read off the glass.

### Task 3.7 — the docs move with the code

**LANDED, 2026-09-30.** The doctrine sentence moved and both of its citations were re-pointed in the same change (`README.md:3`, `Cargo.toml:5`, `AGENTS.md` twice), `docs/architecture.md` gained a "The conversation" section, `docs/keys.md`/`docs/screens.md`/`docs/config.md`/`docs/roadmap.md` follow the shipped screen, and both stale "52 tests" counts (architecture.md, AGENTS.md) now carry the measured 120. Gate: `"verdict": "pass"`, 12 files checked, no rule's count rose.

**Files:** modify `README.md`, `AGENTS.md`, `Cargo.toml`, `docs/architecture.md`, `docs/keys.md`,
`docs/screens.md`, `docs/config.md`, `docs/roadmap.md`.

- `Cargo.toml:5`'s description no longer says only `Read-only by construction.` — it says read-only
  toward the repository and a driver of one conversation. **`AGENTS.md` cites that line by number and
  by literal**; both the citation and the literal are updated in the same edit, and `README.md:3` is
  cited too, so the README's opening sentence moves with it.
- `docs/architecture.md` gains the section §4 of this plan turned into prose: the one thing the console
  owns, and the rule that it says so.
- `docs/config.md` documents `console.conversation` and the new cadence key.
- `docs/roadmap.md`: Phase 2 and Phase 3 are marked as **superseded by this plan**, with a pointer to
  `.hermes/plans/`, and Phase 1's width tasks are marked as satisfied by Task 3.4.
- **Two stale test counts, measured on 2026-09-30 while running the gate.** `docs/architecture.md:99`
  says `52 tests: 41 frame tests, 8 refresh tests, and 3 unit tests`, and `AGENTS.md` says the last
  measured run is `52 passed, 0 failed`. The suite that actually runs is **80**: 24 unit tests, 48 frame
  tests, 8 refresh tests (`cargo test --release --offline` → `24 passed`, `48 passed`, `8 passed`, zero
  failures, fmt clean and clippy silent). This repository's rule is that a doc and the code disagree,
  fix one of them, so fix these: the counts go in as the measured numbers, with the size matrix Task 3.4
  adds counted in the same pass. It is a one-line fix in two files and it is not optional — a test count
  nobody re-measured is the same class of claim as a reading without its age.
- Run the conformance gate over the edited prose and quote its real output:

    python3 scripts/run-conformance-gate.py

Expected: no new drift. A citation left stale by a moved line is a gate failure, not a cosmetic issue.

---

## Phase 4 — reconciliation and liveness (the honesty work)

### Task 4.1 — the reconcile read, through the existing probe layer

**Files:** modify `src/app.rs`, `src/state.rs` (`LiveView` gains nothing; `Probes.session_transcript` is
reused as-is), `agentic.config.json` (one cadence key).

After a turn's `result` envelope (or after `close_turn`), the console asks the worker for a **forced**
collect and reasons over `snapshot.live.session_transcript`:

    fn reconcile(&mut self) {
        let read = &self.snapshot.live.session_transcript;
        match (&read.value, &read.error) {
            (Some(transcript), _) if transcript.session_id == self.conversation.session.clone().unwrap_or_default() => {
                let lines = transcript.lines.clone();
                self.conversation.confirm_from(&lines);
            }
            (_, Some(error)) => self.conversation.note(format!(
                "the session transcript could not be read, so nothing was confirmed: {error}")),
            _ => self.conversation.note(
                "the transcript read named no session, so nothing was confirmed"),
        }
    }

Three properties the tests must pin: the read is attributed to a session and refuses to reconcile
against another session's transcript; a failed read confirms **nothing** and says so; and a bounded tail
that matches nothing reports the bounded tail, not "unverified".

### Task 4.2 — a resumed turn contributes only new items (measured, not assumed)

**Files:** modify `src/conversation.rs`, tests.

Phase 0 measured this and the answer inverts the worry this task was written to address: a resumed
turn's stream **replays nothing** — of turn one's six uuids, zero reappeared in turn two's capture
(`docs/spikes/prompt-surface-protocol.md` §2). So the parser needs no cross-invocation dedupe, and this
task becomes the test that keeps it that way.

The test reads the fixtures and asserts the measured property directly: feed `one-shot.jsonl`, then
`resumed.jsonl`, and assert (a) the item count is the sum of both turns' *items*, with neither turn's
items lost, and (b) the uuids the two fixtures share is zero — the property the CLI exhibited, pinned
so that a future CLI version that starts replaying history fails a test instead of silently doubling a
transcript. Then the same-uuid rule inside a turn still applies (rule 1): a partial message arriving
again updates its own item, which is what makes the streaming cursor possible.

### Task 4.3 — the header line, everywhere the buffer is named

**Files:** modify `src/ui/mod.rs` (header), `src/ui/conversation.rs`.

The header carries `conversation.header_line()` on every screen, not only CONVERSATION, so the operator
who is looking at the LIVE overlay still reads that the transcript is `12 item(s), 9 confirmed`. One
line, one wording, one place (`Conversation::header_line`), so the pane, the footer and `--dump` cannot
drift apart.

### Task 4.4 — the end-to-end drive

**Verify, with the container up and a real credential staged:**

    cargo build --release
    ./open.sh
    tmux -L agentic-console capture-pane -pt ac
    # i, type a real brief, Enter, y
    tmux -L agentic-console capture-pane -pt ac          # the turn in progress
    # after the result envelope: the checkpoint footer, the buffer line, and the reconciliation note
    docker exec agent-console-m1 ls -l /root/.claude/projects/-workspace/<the id the console printed>.jsonl

Expected: the session file exists under **the id the console minted**, the transcript's items are
`Confirmed` where the file corroborates them, and the checkpoint card's state is unchanged from what it
reported before this change. Quote all of it — the pane's capture and the `ls` — into the task's
handoff. This is the task that proves the console drives the pipeline rather than describing it.

---

## Phase 5 — the persistent stream (deferred, evidence-gated)

Scheduled only if Task 0.3's answer was "the CLI accepted stdin envelopes and stayed alive".

### Task 5.1 — `StreamSender`

**Files:** create `src/session.rs` (the `Sender` trait from §2, `OneShotSender` wrapping
`actions::start`, and `poll` returning `Vec<StreamLine>`).

### Task 5.2 — `StreamSender`: one child, many turns

`docker exec -i … --input-format stream-json --output-format stream-json --replay-user-messages`, with
the console writing one JSON envelope per turn and reading until `result`. **The input envelope shape is
whatever Task 0.3 recorded** — the plan does not invent it here, and the implementer must not either.

### Task 5.3 — interrupt, as a doctrine amendment

Stopping the console's **own** child is not the destructive act the console refuses elsewhere (it found
no process; it started one). Even so, it changes a sentence in `docs/architecture.md`, so it is the
operator's call, not the implementer's: present the wording and the key binding, get the go-ahead, then
write it. Until then, `Ctrl-C` in the composer keeps its present meaning (leave the turn running) and
`q` with a turn in flight says which process is left behind.

---

## Files likely to change

| file | what | phase |
| --- | --- | --- |
| `src/conversation.rs` | **new** — the transcript model, the envelope parser, reconciliation | 1 |
| `src/uuid.rs` | **new** — a v4 UUID from `/dev/urandom`, no new dependency | 2 |
| `src/session.rs` | **new** — the `Sender` seam (Phase 5's swap point) | 5 |
| `src/ui/conversation.rs` | **new** — the transcript pane, the composer, the review box | 3 |
| `src/actions.rs` | `ActionKind::Prompt`, `PromptTarget` in `Guards`, the argv builder | 2 |
| `src/app.rs` | `Screen`, `Mode::{Composer, PromptConfirm}`, the conversation, the send path, routing | 2–3 |
| `src/ui/mod.rs` | `draw_overlay`, the new body dispatch, the header's buffer line, KEY_REFERENCE | 3 |
| `src/ui/style.rs` | six new styles, one per item kind | 3 |
| `src/main.rs` | `--prompt TEXT` | 1 |
| `src/dump.rs` | `render_conversation` | 1 |
| `src/lib.rs` | three new module declarations | 1–2 |
| `agentic.config.json` | `console.conversation`, one cadence key, `komun_defaults` | 2 |
| `tests/fixtures/stream-json/{one-shot,resumed,realtime-two-turns,session-record}.jsonl` + `build_fixtures.py` | **new** — four real-envelope fixtures and the reproducible builder beside them | 0 |
| `docs/spikes/prompt-surface-protocol.md` | **new** — the spike's real output | 0 |
| `docs/{architecture,keys,screens,config,roadmap}.md`, `README.md`, `AGENTS.md`, `Cargo.toml` | the doctrine and the citations | 3 |
| `tests/{frames,refresh}.rs` | the new assertions; the existing ones preserved | 3–4 |

## Tests and validation

The gate is what it already is, and it runs after every task:

    cargo fmt --check
    cargo clippy --release --all-targets --offline -- -D warnings
    cargo test --release --offline

Today: 52 tests, 0 clippy warnings. New coverage, and what each suite is for:

| suite | covers |
| --- | --- |
| `src/conversation.rs` unit tests | the parser (the three pinned rules), the ring and its drop count, uuid-keyed updates, reconciliation, both turn-closing doors |
| `src/uuid.rs` unit tests | two calls differ; each survives `probe::uuid_token` |
| `src/actions.rs` unit tests | `Prompt`'s argv on the first turn and on a resume; every refusal, in a test each |
| `tests/frames.rs` | the conversation screen at the six sizes; the composer in Normal, Composer and PromptConfirm; `q` quits in Normal and is text in Composer; the three overlays at full area (**existing assertions unchanged**) |
| `tests/refresh.rs` | a turn streaming in while the UI keeps drawing and answering keys; the reconcile read's cadence; a failed reconcile confirming nothing |
| `scripts/port-self-test.sh --fork` | every new config key's consumer row | run if a new consumer reads the seam table |

## Risks, tradeoffs and open questions

| risk | why it matters | what the plan does |
| --- | --- | --- |
| `--session-id` may not be honored with `-p` | the first-turn id is the whole basis for resuming by id instead of by "the newest session" | Task 0.1 settles it before any code; Task 2.5 carries the fallback (read the id out of `init`) |
| a resumed stream may replay history | a naive parser would double the transcript | Task 0.2 measures it; Task 4.2 pins the dedupe on the shared `uuid` |
| the transcript is read as a bounded tail | a long conversation cannot confirm its oldest items | stated as correct behaviour and reported in the pane's own words (Task 4.1) |
| a long-lived child (Phase 5) is a second writer | it is the exact hazard the ruling refusal exists to prevent | Phase 5 is deferred and gated on Task 0.3; Option A cannot produce it |
| killing the console's own child | the console has never killed anything | raised as Task 5.3 and left to the operator, with the wording shown first |
| the doctrine sentence moves | `AGENTS.md` cites `Cargo.toml:5` and `README.md:3` | Task 3.7 changes the literals and the citations together and runs the conformance gate |
| the screen's own newness | the suite has only ever rendered at 230x60 and 200x50 | Task 3.4 tests the six sizes and drops rows loudly |

**Open questions for the operator** (not decided by this plan):

1. **Interrupt.** Should `Ctrl-C` in the composer stop the turn the console itself started? Phase 5.3
   assumes yes-after-approval; until then the plan leaves the turn running and says so.
2. **`--prompt` on the command line.** Phase 1 adds it for tests and scripts. It is a second way to send
   a prompt that bypasses the composer; if that is unwanted, it can be `--dump`-shaped (print the argv,
   send nothing) instead. Recommend keeping it: it is what makes the end-to-end test possible without a
   terminal.
3. **Where the conversation lives between runs.** Today the buffer dies with the console. A `--resume
   <session-id>` flag on the console itself would re-attach the transcript to an existing session by
   re-reading its `.jsonl`. Not in this plan (YAGNI); say so if it is wanted and it becomes Phase 6.
