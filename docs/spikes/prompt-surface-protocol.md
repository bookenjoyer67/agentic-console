# The prompt surface: what the agent CLI's stream protocol actually does

This is a spike record, not a governed document: it is not in `gates.conformance.files`, it is not
cited by anything, and its `path:line`-shaped mentions are quotations of command output rather than
citation-form claims. It records what was run, what came back, and what that settles for
`.hermes/plans/2026-09-30_173248-prompt-surface-conversation-primary.md`, whose Phases 1 to 4 were
written against the answers below.

## Where this ran, and one honest limitation

Every command was run inside `agent-console-m1` — the container `agentic.config.json` names as
`console.container` — with `-w /workspace` and the console's own flags (`--agent orchestrator
--permission-mode acceptEdits`) where the console would pass them. The CLI there is `2.1.280 (Claude
Code)`, the same version as in `agent-rev-m3`.

**Every turn in this spike failed at the API with HTTP 429**, and no successful model turn was
observed. `apiKeySource` read `none` in the `init` envelope of every capture, and the `result`
envelope carried `"api_error_status": 429` with the message `API Error: Request rejected (429) · This
request would exceed your account's rate limit.`

What that does and does not affect: everything this spike settles is about the **CLI's own protocol**
— is the id honored, does the transcript land under it, does a resume replay, does one process carry
two turns — and a 429 leaves all of that measurable, because the CLI still emits its full envelope
sequence and still writes its session record. What it does *not* settle is anything that depends on a
turn completing real work: a `tool_use` block, a `tool_result` block, a successful `result`, or
`queued_turn_count` being non-zero. Those are marked unmeasured below and must be re-measured once the
account answers again, before Phase 1's parser is called done.

## 1. Is `--session-id` honored, and does the transcript land under that name?

Yes to both, and this was the question the whole architecture rests on.

    $ ID=$(docker exec agent-console-m1 cat /proc/sys/kernel/random/uuid)
    $ echo "$ID"
    19c5d842-d26a-4110-98c3-320067a06e3b

    $ time docker exec -w /workspace agent-console-m1 claude -p "Reply with the single word: pong" \
        --session-id "$ID" --output-format stream-json --verbose --include-partial-messages \
        --agent orchestrator --permission-mode acceptEdits
    real    0m1.771s

Every one of the six envelopes it emitted carried that id, and nothing else:

    session_ids: {'19c5d842-d26a-4110-98c3-320067a06e3b'}
    minted       : 19c5d842-d26a-4110-98c3-320067a06e3b
    MATCH

and the session's own record was written under exactly that name:

    -rw------- 1 root root 42170 Sep 30 17:50 /root/.claude/projects/-workspace/19c5d842-d26a-4110-98c3-320067a06e3b.jsonl

The `init` envelope reported the rest of what the console wanted to know:

    session_id = 19c5d842-d26a-4110-98c3-320067a06e3b
    model = claude-opus-5-5[1m]
    permissionMode = acceptEdits
    apiKeySource = none
    claude_code_version = 2.1.280
    cwd = /workspace
    agents = ['claude', 'Explore', 'general-purpose', 'implementer', 'komun-contract-auditor',
              'komun-docs-stylist', 'orchestrator', 'Plan', 'planner', 'project-manager',
              'researcher', 'reviewer', 'statusline-setup', 'tester']

`orchestrator` is in that list, so the console's own `claude_flags` were accepted unchanged.

**Decides:** the console can mint the conversation's id before sending, so turn two resumes an id the
console chose rather than one it inferred from the newest file in a session directory. This is the
difference between the composer's resume target and the ruling action's, and it is why the plan can
promise that the composer never resumes somebody else's conversation.

## 2. Does `--resume` continue that conversation, and does it replay what was already said?

It continues it exactly, and a resumed stream **replays nothing**. This one was worth measuring: a
parser that assumed a resume replayed history would have doubled every transcript.

    $ docker exec -w /workspace agent-console-m1 claude --resume "$ID" -p "Reply with the single word: again" \
        --output-format stream-json --verbose --include-partial-messages --permission-mode acceptEdits
    exit=1 lines=4

    === replay check: 0 uuid(s) in both captures; 4 uuid(s) new in the second
    === same session across both turns: True

Turn two's four envelopes were `init`, `status`, `assistant`, `result` — the earlier turn's envelopes
were absent, and none of the six uuids from turn one reappeared. The session record grew across the
resume even though the turn failed:

    42170 bytes  mtime 2026-09-30 17:50:59   (before)
    46951 bytes  mtime 2026-09-30 17:51:20   (after)

**Decides:** Phase 1's parser appends a resumed turn's items and needs no cross-invocation dedupe; the
history lives in the session record, not in the stream. It also corroborates the existing replay guard
in `src/app.rs`, which uses the session's own byte size as a "did the run move" signal: a failed turn
still grows the file.

## 3. Does one process carry more than one turn over stdin?

Yes. The envelope shape the CLI accepts on stdin is:

    {"type":"user","message":{"role":"user","content":[{"type":"text","text":"…"}]}}

Two of those, written eight and ten seconds apart into one `docker exec -i` with stdin held open,
produced ten envelopes and **two complete turns in a single process**:

    system/hook_started, system/hook_response, system/init, system/status, assistant, result
    system/init, system/status, assistant, result

with `pipeline exit=1 lines=10` and empty stderr.

**Two facts here change the plan.** The first is that the persistent-stream architecture is real
rather than hypothetical: the CLI stays alive between turns and the envelope shape is accepted. The
second is a trap: **the second turn emits its own `init` envelope.** A parser that treats `init` as
"the session starts here" would reset the transcript on every turn of a persistent stream — so `init`
after the first must update the session, model and CLI version fields and touch no items.

**Decides:** Phase 5 is schedulable rather than dead, and Phase 1's parser gets one more pinned rule.

## 4. Which stream uuids does the session record actually corroborate?

Only the run's own words. This is the measurement the reconciliation design in Phase 4 rests on, and
it was taken by intersecting the real captures rather than reasoning about them:

    stream envelope uuids : 6
    session record uuids  : 16
    SHARED                : 1
      593eea90-8783 stream: [('assistant', None)]  session record: ['assistant']

The session record's own types, split by whether they carry a uuid at all:

    carries a uuid : attachment, user, assistant
    no uuid        : agent-setting, queue-operation, atis-latch, last-prompt, cost-state, mode

So the correlation is precise in both directions. An `assistant` or `user` envelope is the same uuid
as the session record for that same message, and can therefore be **confirmed**. A `system/init`,
`system/status`, `system/hook_*` or `result` envelope has no session record under its uuid and can
**never** be confirmed by this rule.

**Decides:** the plan's `ItemState` needs a third state beyond confirmed-and-live. A console line
taken from a `system` envelope is not an unconfirmed claim about the run — it is the console's own
line, and drawing it as "live" would invite the operator to read it as a reading that failed to
corroborate. Phase 1 gained `ItemState::Own` and Phase 4's header line now counts the three classes
separately.

## 4b. What the 429 did teach: the failure shape, in full

A retry after the first spike (fresh id, a prompt that asks for a tool call, `--allowedTools
"Bash(printf *)"`) also returned 429, so the tool-block shapes remain unmeasured. It did return the
**complete** `result` envelope, and one field in it is a trap for any parser:

    {"duration_api_ms":0,"stop_reason":"stop_sequence","session_id":"d872828b-…","total_cost_usd":0,
     "usage":{"output_tokens_details":{"thinking_tokens":0},"input_tokens":0,"output_tokens":0,…},
     "modelUsage":{},"permission_denials":[],"terminal_reason":"api_error",
     "fast_mode_state":"off","fast_mode_disabled_reason":"sdk_opt_in_required",
     "subagent_stats":{…},"is_error":true,"num_turns":1,
     "subtype":"success",                     <-- a failure, reported as "success"
     "api_error_status":429,
     "result":"API Error: Request rejected (429) · This request would exceed your account's rate limit…",
     "duration_ms":620,"uuid":"e8171880-…","queued_turn_count":0,"result_index":0}

**`subtype` is `"success"` on a run whose `is_error` is true and whose `terminal_reason` is
`api_error`.** A parser that treats `subtype == "success"` as success would draw a rate-limited turn as
a green one, which is precisely the class of lie this console exists not to tell. The verdict comes from
`is_error` and `terminal_reason`; `subtype` is a field name that happens to say "success".

The rest of the envelope is now known to carry, and the console may draw: `usage` (tokens, with a
`thinking_tokens` breakdown), `modelUsage`, `permission_denials` (a list — empty here),
`subagent_stats` (spawned, requested, max_depth, killed, refused), `total_cost_usd`, `num_turns`,
`duration_ms`, and `queued_turn_count` with `result_index`. `queued_turn_count` exists in the shape and
read `0`; a **non-zero** value is still unmeasured, which is the last row of section 5.

**Re-measured through the console itself, 2026-09-30.** Phase 2's send path ran a real turn
(`agentic-console --prompt "…"`, `agent-console-m1`, `claude 2.1.280`) and the account is still rate
limited, so the turn failed — which reproduced this shape a second time, through the artifact that reads
it rather than through a capture:

* the minted id was on **every** envelope (`hook_started`, `hook_response`, `init`, `status`,
  `assistant`, `result`), and the `assistant` envelope carried `"error":"rate_limit"` and
  `"is_api_error_message":true` on top of the message — fields this section did not previously record;
* `"subtype":"success"` with `"is_error":true`, `terminal_reason:"api_error"`, `api_error_status:429`,
  `num_turns:1`, `total_cost_usd:0` — as above;
* the console drew it as a failure in both registers (the run's own words and its own refusal line),
  and its transcript header read `0 confirmed … 1 live, 4 this console's own`. So the trap is now pinned
  by a real turn as well as by a fixture line, and the console's exit status stayed `0`: a failed reading
  is still a reading.

## 5. What this spike did not measure

| question | why not | when it must be answered |
| --- | --- | --- |
| `queued_turn_count` non-zero | nothing was ever queued behind a live turn | before Phase 5 is scheduled |
| whether `--session-id` refuses an id in use *because a session exists* or *because a process holds it* | observed once, not isolated; see below | before the first-turn retry path is written |

Everything else in this table was **measured on 2026-09-30** (§6): a successful turn's `result`, the
`tool_use` / `tool_result` block shapes, and the partial-message chunks.

That last one is a real finding and it is recorded here as a partial one. Re-using the id from turns
one and two was refused outright:

    $ ... | docker exec -i -w /workspace agent-console-m1 claude -p --input-format stream-json \
            --output-format stream-json --verbose --include-partial-messages --session-id "$ID"
    Error: Session ID 19c5d842-d26a-4110-98c3-320067a06e3b is already in use.

with zero envelopes and exit 1. No process was running at the time, so "in use" is at least partly
about the session record existing on disk — but the two cases were not isolated, and the plan treats it
as a refusal to expect rather than a rule to encode: a first turn that has to be retried mints a fresh
id, and the console shows the CLI's own sentence rather than one of its own.

## 6. The first completed turn, 2026-09-30

The account's rate limit cleared, and this is the first turn that ran to a result. It closes Task 0.5.

    $ ./target/release/agentic-console --repo . --prompt "Use the Read tool to read the first line of
      README.md, then reply with exactly that line and nothing else."
    exit 0

**The `result` envelope, successful**: `"subtype":"success"` *with* `"is_error":false`,
`"terminal_reason":"completed"`, `"num_turns":2`, `"result":"# agentic-console"`, `duration_ms:8108`,
`ttft_ms:4849`, `total_cost_usd:0.05202360000000001`, `"permission_denials":[]`, `"queued_turn_count":0`,
and a populated `usage` and `modelUsage` — `{"claude-opus-5-5[1m]":{"inputTokens":2,"outputTokens":4,
"cacheReadInputTokens":0,"cacheCreationInputTokens":9370,"costUSD":0.046938,"contextWindow":1000000,
"maxOutputTokens":128000,"canonicalModel":"claude-opus-5-5","provider":"firstParty",
"costBasis":"list"}}`. Two turns of cost share the one figure, which is why the total is the larger
number and the per-model entry the smaller one.

This is the contrast the failure shape needed: §4b's 429 also said `"subtype":"success"`, so a parser
keying on `subtype` cannot tell the two apart. `is_error` and `terminal_reason` can, and now both sides
of the pair are measured facts rather than one fact and one fixture.

**Envelope census for the turn** (one stream, one `-p` invocation): 8 `system`, 26 `stream_event`,
3 `assistant`, 1 `user`, 1 `result`.

**`tool_use`, verbatim**:

    {"type":"tool_use","id":"toolu_01Xq8Q25P6DhHCNHciR3LghK","name":"Read",
     "input":{"file_path":"/workspace/README.md","limit":1},"caller":{"type":"direct"}}

**`tool_result`, verbatim**:

    {"tool_use_id":"toolu_01Xq8Q25P6DhHCNHciR3LghK","type":"tool_result","content":"1\t# agentic-console"}

Two things here are not in the fixtures and would have been guessed wrong:

- the `caller` object on the tool call (`{"type":"direct"}`), which nothing in the plan or the fixtures
  anticipated;
- `content` is a **plain string**, not a list of content blocks. A parser written against the
  Anthropic-API shape would read an empty result.

The call and the result share `tool_use_id`, which is why the console keys the call as `call:<id>`: the
result arrives in a `user` envelope carrying the same id, and a key of the bare id would collide with it.

**A subtype the fixtures do not carry**: the turn emitted `system/thinking_tokens` **three times**. The
console reports a shape it does not read, in its own words and by name; three identical lines per turn is
the console's own noise burying the lines worth reading, so it says it once and counts the repeats
(`... it was not applied (3 so far)`). Nothing is dropped, and the count is on the glass.

**Partial messages**, as `stream_event` envelopes whose `event.type` walks
`message_start` → `content_block_start` → `content_block_delta` (`{"text":"pong"}`) →
`content_block_stop` → `message_delta` → `message_stop`. They are recognised and counted
(`Conversation::partial_events`) rather than drawn: the final `assistant` envelope carries the same
message, and drawing both would double every line.

**Reconciliation, on that turn**: the console's transcript ended
`buffer: 11 item(s) -- 3 confirmed against b8a9d957-…, 0 live, 8 this console's own`, with
`[Tool/Confirmed] Read(/workspace/README.md)`, `[Result/Confirmed] 1\t# agentic-console` and
`[Agent/Confirmed] # agentic-console` all found in the session record the console read back out of the
container. The tool call, its result and the run's own summary line were each corroborated by the record
the run wrote for itself — which is the whole claim the transcript makes.

## The fixtures this produced

`tests/fixtures/stream-json/` carries four files cut from these captures, so Phase 1's parser is tested
against the CLI's real output rather than against a shape invented in a test:

| file | what it is | what it is for |
| --- | --- | --- |
| `one-shot.jsonl` | 6 envelopes | the first-turn sequence, hooks included; **also carries the 429 failure shape** (`"subtype":"success"` with `"is_error":true`), so parser rule 5 is pinned by a real line and not by a hand-written one |
| `resumed.jsonl` | the four envelopes of turn two | that a resume starts at `init` and replays nothing |
| `realtime-two-turns.jsonl` | the ten envelopes of the stdin experiment | the second `init` rule |
| `session-record.jsonl` | the session's own `.jsonl`, minus attachments | reconciliation by uuid |

Redaction is deliberately minimal, because the value of these files is that they are real: session ids
are replaced with one fixed test id, the SessionStart hook's output is replaced (the real one carried
this machine's injected memory layer), long message text is truncated to 120 characters, and every uuid
is replaced through a **shared** map keyed on the original — so the one correlation that matters
(`assistant` envelope ↔ `assistant` session record) survives into the fixtures and can be asserted on.
The fixtures agree with the measurement in section 4: exactly one uuid is shared between
`one-shot.jsonl` and `session-record.jsonl`, and it is the assistant message.

The builder lives beside the fixtures and is reproducible in two arguments — a captures directory and
the fixtures directory. Re-running it over the same captures reproduces all four files byte for byte
(checked with `md5sum -c`), which is the property that keeps a fixture from becoming a magic file:

    python3 tests/fixtures/stream-json/build_fixtures.py <captures-dir> tests/fixtures/stream-json

The captures themselves are **not** committed: they carry this machine's injected memory layer, and the
commands that produce them are the four sections above.
