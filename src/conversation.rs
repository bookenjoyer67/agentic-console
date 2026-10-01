//! The console's own transcript: what it sent, what came back, and which of those lines the run's own
//! record has corroborated.
//!
//! This is the one thing the console owns. Everything the other screens draw is a reading -- a file, a
//! `docker` call, a command's output, each carrying its source and the age of the read -- and this is a
//! *buffer*. It is therefore drawn as a buffer, and it says so in its own words: an item taken from the
//! live stream is [`ItemState::Live`] until the same item's uuid is found in the session's own
//! `.jsonl`, at which point it is [`ItemState::Confirmed`].
//!
//! Four rules this module exists to hold, each measured against the CLI rather than assumed (see
//! `docs/spikes/prompt-surface-protocol.md`):
//!
//! * **A repeated uuid updates its own item.** `--include-partial-messages` sends the same message more
//!   than once with growing text. That is one item being written, not two items, and treating it as two
//!   would triple a transcript.
//! * **A second `init` touches no items.** A resumed turn, and every turn of a persistent stream, emits
//!   its own `init` envelope. It updates the session, the model and the CLI version and adds nothing, or
//!   a stream would reset or duplicate its own transcript on every turn.
//! * **A turn's verdict comes from `is_error` and `terminal_reason`, never from `subtype`.** The CLI
//!   reports a rate-limited turn as `"subtype":"success"` with `"is_error":true`; a parser keyed on
//!   `subtype` would draw a failed turn as a green one, which is exactly the class of lie this console
//!   exists not to tell.
//! * **Only the run's own words are confirmable.** An `assistant` or `user` envelope's uuid is the
//!   session record's uuid for that same message; a `system/*` or `result` envelope has no uuid-bearing
//!   record at all. So a line this console wrote about the run is [`ItemState::Own`] and stays `Own` --
//!   folding it into "live" would read as a claim that failed to verify, when it was never a claim about
//!   the run in the first place.
//!
//! Nothing here reads a file, spawns a process or holds a lock: this module is the model and the parser,
//! and the process that feeds it lives in the send path.

use std::collections::{BTreeMap, VecDeque};
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
    /// A line this console wrote: the argv it ran, a reconciliation count, an envelope it does not read.
    Console,
    /// A refusal, in the console's own words.
    Refused,
}

/// How much an item is worth trusting.
///
/// Three of these are states of a claim about the run; the fourth is not a claim at all. The
/// distinction is measured, not stylistic: the session's own record corroborates an `assistant` or
/// `user` envelope's uuid and never corroborates a `system/*` or `result` envelope's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// The run's own words, read from the live stream, not yet found in the session transcript.
    Live,
    /// The same uuid was found in the session's own transcript.
    Confirmed,
    /// This console's own line: an argv, a reconciliation count, a hook's outcome, an envelope it does
    /// not read. Not a reading about the run and not confirmable by construction.
    Own,
    /// The stream said this failed, or the console could not read it.
    Failed,
}

/// One line of the transcript, with the identity the stream gave it.
///
/// Two identities, because a line can be unique in this buffer for a reason that is not the run's own
/// identity for the message. A tool call and its result are two lines about one `tool_use` id, so the
/// id cannot be the key for both -- while the *message* they came from is the same, so the envelope's
/// uuid is the run's identity for both. Collapsing the two would either merge a call into its result or
/// make a corroborated line look uncorroborated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// What makes this line unique in this buffer: a later line carrying the same key updates this
    /// item rather than appending a second one. `None` for a line that is always appended.
    pub key: Option<String>,
    /// The run's own identity for the line's message, when the stream carried one: the uuid the
    /// session transcript is searched for. `None` for a line this console wrote.
    pub uuid: Option<String>,
    pub kind: ItemKind,
    pub state: ItemState,
    pub text: String,
    /// The moment the console appended it. Not a reading's age: this is the buffer's own clock.
    pub at: SystemTime,
}

/// One send and everything that followed it, as the `result` envelope described it.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub started: SystemTime,
    pub ended: Option<SystemTime>,
    /// The `result` envelope's own reason, verbatim.
    pub stop_reason: Option<String>,
    /// The `result` envelope's own count of model turns, verbatim.
    pub num_turns: Option<u64>,
    /// The `result` envelope's own cost, verbatim.
    pub cost_usd: Option<f64>,
    /// Whether the run reported a failure. Read from `is_error`, never from `subtype`: the CLI calls a
    /// rate-limited turn `"success"`.
    pub is_error: bool,
    /// Why it failed, when it did: `terminal_reason` plus the API's own message.
    pub error: Option<String>,
}

impl Default for Turn {
    fn default() -> Turn {
        Turn {
            started: SystemTime::now(),
            ended: None,
            stop_reason: None,
            num_turns: None,
            cost_usd: None,
            is_error: false,
            error: None,
        }
    }
}

/// What one stream line did to the transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// An item was appended.
    Item,
    /// An existing item was updated in place: a partial message arriving again under its own uuid.
    Updated,
    /// The turn was closed by the run's own `result` envelope.
    TurnEnded,
    /// A line this console reads and does not draw, or an empty line.
    Ignored,
    /// A line whose shape this console does not read. It is reported, never guessed at.
    Unknown,
}

/// How many items a transcript holds before the oldest is dropped, with the drop counted rather than
/// hidden. A ring rather than an unbounded `Vec` because a session is unbounded and a console that
/// grows without limit is a console that eventually dies mid-conversation.
pub const DEFAULT_RING: usize = 400;

/// The console's transcript.
#[derive(Debug)]
pub struct Conversation {
    pub items: VecDeque<Item>,
    pub turns: Vec<Turn>,
    /// The session id, once the stream has named it.
    pub session: Option<String>,
    /// The model the `init` envelope reported, so the pane can say what it is talking to.
    pub model: Option<String>,
    /// The CLI's own version, from the same envelope.
    pub cli_version: Option<String>,
    /// How many partial-message chunks have arrived. Counted rather than drawn: rendering them is the
    /// streaming cursor's job, and what a chunk looks like on the wire is not yet measured against a
    /// successful turn (`docs/spikes/prompt-surface-protocol.md` section 5).
    pub partial_events: u64,
    /// Items dropped off the front of the ring, so the pane can say so loudly rather than clip.
    pub dropped: usize,
    pub max_items: usize,
    /// How many times each distinct unfamiliar shape has arrived, by its own key. The line stays one
    /// line and the count grows in it: see `note_once_counted`.
    counted: BTreeMap<String, usize>,
}

impl Conversation {
    /// A transcript with room for `max_items` items and not one more.
    pub fn new(max_items: usize) -> Conversation {
        Conversation {
            items: VecDeque::new(),
            turns: Vec::new(),
            session: None,
            model: None,
            cli_version: None,
            partial_events: 0,
            dropped: 0,
            max_items,
            counted: BTreeMap::new(),
        }
    }

    /// Append an item, dropping the oldest when the ring is full, and count what was dropped.
    ///
    /// The count is kept rather than the items: a pane that silently shows fewer lines than were
    /// written reads as "there is nothing more", which is a lie the operator has no way to detect.
    pub fn push(&mut self, item: Item) {
        self.items.push_back(item);
        while self.items.len() > self.max_items {
            self.items.pop_front();
            self.dropped += 1;
        }
    }

    /// The console's own line: an argv, a hook's outcome, a reconciliation count.
    pub fn note(&mut self, text: impl Into<String>) {
        self.push(Item {
            key: None,
            uuid: None,
            kind: ItemKind::Console,
            state: ItemState::Own,
            text: text.into(),
            at: SystemTime::now(),
        });
    }

    /// A refusal, in the console's own words.
    pub fn refuse(&mut self, text: impl Into<String>) {
        self.push(Item {
            key: None,
            uuid: None,
            kind: ItemKind::Refused,
            state: ItemState::Failed,
            text: text.into(),
            at: SystemTime::now(),
        });
    }

    /// How many items are the run's own words and still unconfirmed.
    pub fn live(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == ItemState::Live)
            .count()
    }

    /// How many items the run's own record has corroborated.
    pub fn confirmed(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == ItemState::Confirmed)
            .count()
    }

    /// Whether the buffer holds nothing yet.
    ///
    /// The header draws its buffer line on every screen only when there is something in the buffer: a
    /// console that has never been asked anything saying `0 item(s)` on all four screens is noise, and
    /// noise is what the operator learns to read past.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many items are this console's own lines: not a reading, and not confirmable.
    pub fn own(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == ItemState::Own)
            .count()
    }

    /// Say the same thing once, however many times it arrives, and count the repetitions in the line.
    ///
    /// A `system` envelope of a subtype this console does not read can arrive several times in one turn:
    /// a real turn that uses one tool emitted `system/thinking_tokens` three times. One line naming the
    /// shape is a fact about the CLI; three identical lines are the console's own noise, and noise is
    /// what the operator learns to read past -- which costs the lines that matter. Nothing is dropped:
    /// the shape is still named, and the count shows how many arrived.
    ///
    /// `build` is handed the running count, so the first line and the later ones can word themselves.
    pub fn note_once_counted(&mut self, key: &str, build: impl Fn(usize) -> String) {
        let count = self.counted.entry(key.to_string()).or_insert(0);
        *count += 1;
        let text = build(*count);
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|item| item.key.as_deref() == Some(key))
        {
            // The line keeps the instant it was first seen: this is the same fact, more often.
            existing.text = text;
            return;
        }
        self.push(Item {
            key: Some(key.to_string()),
            uuid: None,
            kind: ItemKind::Console,
            state: ItemState::Own,
            text,
            at: SystemTime::now(),
        });
    }

    /// The newest turn, open or closed.
    pub fn last_turn(&self) -> Option<&Turn> {
        self.turns.last()
    }

    /// One line naming the buffer's own state: what it is, how much of it the run's own record
    /// corroborates, and against which session.
    ///
    /// The three counts are separate on purpose. Only an `assistant` or `user` envelope's uuid is
    /// corroborated by the session record, so a console line is `Own` and stays `Own`: folding it into
    /// "live" would read as a claim that failed to verify, when it was never a claim about the run.
    pub fn header_line(&self) -> String {
        let session = self
            .session
            .clone()
            .unwrap_or_else(|| "no session named yet".to_string());
        let dropped = if self.dropped > 0 {
            format!(
                "   {} earlier item(s) dropped (the {}-item ring)",
                self.dropped, self.max_items
            )
        } else {
            String::new()
        };
        format!(
            "buffer: {} item(s) -- {} confirmed against {session}, {} live, {} this console's own{}",
            self.items.len(),
            self.confirmed(),
            self.live(),
            self.own(),
            dropped
        )
    }

    /// Start a turn: record the operator's own words, then open the turn the stream will close.
    pub fn open_turn(&mut self, prompt: &str) {
        self.push(Item {
            key: None,
            uuid: None,
            kind: ItemKind::You,
            state: ItemState::Own,
            text: prompt.to_string(),
            at: SystemTime::now(),
        });
        self.turns.push(Turn::default());
    }

    /// Close the newest turn with how the child actually ended.
    ///
    /// This is the door for a child that died without a `result` envelope. [`Conversation::apply_result`]
    /// is the other door, and neither overwrites the other: a turn closes once, and a turn that closed
    /// on the run's own words is not rewritten by an exit status that arrived afterwards.
    pub fn close_turn(&mut self, word: &str) {
        let Some(turn) = self.turns.last_mut() else {
            return;
        };
        if turn.ended.is_some() {
            return;
        }
        turn.ended = Some(SystemTime::now());
        turn.is_error = true;
        turn.error = Some(word.to_string());
        self.note(format!("the turn ended: {word}"));
    }

    /// Confirm every item the session's own transcript carries.
    ///
    /// Only the run's own words can be confirmed, and which ones is measured: an `assistant` or `user`
    /// envelope's uuid is the session record's uuid for that same message, while `system/*` and `result`
    /// envelopes have no uuid-bearing record at all. So this walks the items that carry a uuid and
    /// leaves everything else alone -- a console line is `Own` and is not waiting for anything.
    ///
    /// An item whose uuid is not in the transcript stays `Live`: a claim the run's own record does not
    /// carry is a claim this console has only its own word for, and it says so by staying live.
    ///
    /// This writes no prose of its own, deliberately. How much a confirming read says depends on facts
    /// the lines cannot carry -- which session was read, and whether the read could see the whole record
    /// or only a bounded tail of it -- so the notes belong to the caller that made the read
    /// (`App::reconcile`), and this returns the count it is asked for.
    pub fn confirm_from(&mut self, jsonl_lines: &[String]) -> usize {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for line in jsonl_lines {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(uuid) = value.get("uuid").and_then(serde_json::Value::as_str) {
                    seen.insert(uuid.to_string());
                }
            }
        }
        let mut confirmed = 0usize;
        for item in self.items.iter_mut() {
            if let Some(uuid) = &item.uuid {
                // No "consume the match" here: one session record corroborates every line this console
                // drew from that message, and a tool call and its result both hang off the message the
                // record names. Confirming the first and skipping the second would be an accident of
                // ordering, not a finding.
                if item.state == ItemState::Live && seen.contains(uuid) {
                    item.state = ItemState::Confirmed;
                    confirmed += 1;
                }
            }
        }
        confirmed
    }

    /// Apply one line of the run's own stream.
    ///
    /// The line is the CLI's, so a shape this console does not read is reported as unknown, with the
    /// CLI version it is talking to -- never silently dropped, and never guessed at. A line that is not
    /// JSON at all is reported the same way.
    pub fn apply_line(&mut self, line: &str) -> Applied {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Applied::Ignored;
        }
        let parsed = serde_json::from_str::<serde_json::Value>(trimmed);
        let Ok(value) = parsed else {
            self.note(format!(
                "a stream line was not JSON and was not applied: {}",
                head(trimmed, 120)
            ));
            return Applied::Unknown;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("system") => self.apply_system(&value),
            Some("assistant") => self.apply_assistant(&value),
            Some("user") => self.apply_user(&value),
            Some("result") => self.apply_result(&value),
            // A partial chunk. Recognised so that it is not reported as unknown on every token, and
            // counted rather than drawn: see `partial_events`.
            Some("stream_event") => {
                self.partial_events += 1;
                Applied::Ignored
            }
            Some(other) => {
                // Counted for the same reason the system subtypes are: an unfamiliar line is reported
                // once with its count, not once per envelope, so the pane stays readable. The version is
                // read out here, not inside the closure, because the closure must not borrow what this
                // call borrows mutably.
                let version = self
                    .cli_version
                    .clone()
                    .unwrap_or_else(|| "version unread".to_string());
                self.note_once_counted(&format!("unknown:type:{other}"), |count| {
                    if count == 1 {
                        format!(
                            "a stream line of type '{other}' is not one this console reads (claude {version}); it was not applied"
                        )
                    } else {
                        format!(
                            "a stream line of type '{other}' is not one this console reads (claude {version}); it was not applied ({count} so far)"
                        )
                    }
                });
                Applied::Unknown
            }
            None => {
                self.note("a stream line carried no type and was not applied");
                Applied::Unknown
            }
        }
    }

    /// A `system` envelope: the session's own facts, a hook's outcome, a status word.
    fn apply_system(&mut self, value: &serde_json::Value) -> Applied {
        let subtype = value
            .get("subtype")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        match subtype {
            "init" => {
                // The first init names the conversation; every later one is the same conversation
                // starting another turn, so it updates the fields and adds nothing. A persistent
                // stream re-emits this on every turn.
                let first = self.session.is_none();
                if let Some(session) = value.get("session_id").and_then(serde_json::Value::as_str) {
                    self.session = Some(session.to_string());
                }
                if let Some(model) = value.get("model").and_then(serde_json::Value::as_str) {
                    self.model = Some(model.to_string());
                }
                if let Some(version) = value
                    .get("claude_code_version")
                    .and_then(serde_json::Value::as_str)
                {
                    self.cli_version = Some(version.to_string());
                }
                if !first {
                    return Applied::Ignored;
                }
                let session = short(&self.session);
                let model = self
                    .model
                    .clone()
                    .unwrap_or_else(|| "model unread".to_string());
                let version = self
                    .cli_version
                    .clone()
                    .unwrap_or_else(|| "version unread".to_string());
                self.note(format!(
                    "the run started: claude {version}, model {model}, session {session}"
                ));
                Applied::Ignored
            }
            "hook_started" => Applied::Ignored,
            "hook_response" => {
                let name = value
                    .get("hook_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("(unnamed hook)");
                let outcome = match value.get("exit_code").and_then(serde_json::Value::as_i64) {
                    Some(code) => format!("exit {code}"),
                    None => "no exit code was reported".to_string(),
                };
                self.note(format!("hook {name}: {outcome}"));
                Applied::Ignored
            }
            "status" => Applied::Ignored,
            _ => {
                // One line per distinct shape, with its count, not one per envelope: see
                // `note_once_counted`. The shape is still named -- nothing is dropped -- and the
                // repetition is visible in the line instead of filling the pane.
                self.note_once_counted(&format!("unknown:system:{subtype}"), |count| {
                    if count == 1 {
                        format!(
                            "a system envelope of subtype '{subtype}' is not one this console reads; it was not applied"
                        )
                    } else {
                        format!(
                            "a system envelope of subtype '{subtype}' is not one this console reads; it was not applied ({count} so far)"
                        )
                    }
                });
                Applied::Unknown
            }
        }
    }

    /// An `assistant` envelope: the run's own words, or a tool call.
    fn apply_assistant(&mut self, value: &serde_json::Value) -> Applied {
        let uuid = value
            .get("uuid")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let blocks = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut applied = Applied::Ignored;
        for block in &blocks {
            match block.get("type").and_then(serde_json::Value::as_str) {
                Some("text") => {
                    let text = block
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if text.is_empty() {
                        continue;
                    }
                    // A message's key and its run-identity are the same thing: the envelope's uuid.
                    let outcome =
                        self.upsert(uuid.clone(), uuid.clone(), ItemKind::Agent, text, false);
                    if let Some(outcome) = outcome {
                        applied = outcome;
                    }
                }
                Some("tool_use") => {
                    let name = block
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("(unnamed tool)");
                    let argument = tool_argument(block);
                    // The call is keyed by its own id -- two calls in one message are two lines -- while
                    // its run-identity stays the message's uuid, so the record can corroborate it.
                    let key = block
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(|id| format!("call:{id}"))
                        .or_else(|| uuid.clone());
                    let text = format!("{name}({argument})");
                    let outcome = self.upsert(key, uuid.clone(), ItemKind::Tool, &text, false);
                    if let Some(outcome) = outcome {
                        applied = outcome;
                    }
                }
                // Thinking blocks are the run's own words too, but this console draws what it can
                // quote and label, and a block it does not draw is not a block it invents a line for.
                Some("thinking") => {}
                _ => {}
            }
        }
        applied
    }

    /// A `user` envelope: what a tool returned.
    fn apply_user(&mut self, value: &serde_json::Value) -> Applied {
        let uuid = value
            .get("uuid")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let blocks = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut applied = Applied::Ignored;
        for block in &blocks {
            match block.get("type").and_then(serde_json::Value::as_str) {
                Some("tool_result") => {
                    let failed = block
                        .get("is_error")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let body = tool_result_text(block);
                    let text = if body.is_empty() {
                        if failed {
                            "the tool failed and returned nothing".to_string()
                        } else {
                            "the tool returned nothing".to_string()
                        }
                    } else {
                        body
                    };
                    let id = block
                        .get("tool_use_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                        .or_else(|| uuid.clone());
                    // The result is about the same id as the call, so it cannot share the call's key:
                    // it would rewrite the call's line into the tool's output and lose the call.
                    let key = id.map(|id| format!("result:{id}"));
                    let outcome = self.upsert(key, uuid.clone(), ItemKind::Result, &text, failed);
                    if let Some(outcome) = outcome {
                        applied = outcome;
                    }
                }
                // A `user` envelope carrying text is this console's own prompt echoed back (the shape
                // `--replay-user-messages` produces). The console already recorded what it sent in
                // `open_turn`, so drawing it again would double the operator's own words.
                Some("text") => {}
                _ => {}
            }
        }
        applied
    }

    /// A `result` envelope: the turn's verdict.
    ///
    /// The verdict is `is_error` plus `terminal_reason`. It is emphatically **not** `subtype`: the CLI
    /// reports a rate-limited turn as `"subtype":"success"` with `"is_error":true`, and a parser keyed on
    /// `subtype` would draw a failed turn as a green one.
    fn apply_result(&mut self, value: &serde_json::Value) -> Applied {
        let is_error = value
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let terminal = value
            .get("terminal_reason")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let message = value
            .get("result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let stop_reason = value
            .get("stop_reason")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let num_turns = value.get("num_turns").and_then(serde_json::Value::as_u64);
        let cost_usd = value
            .get("total_cost_usd")
            .and_then(serde_json::Value::as_f64);
        match self.turns.last_mut() {
            Some(turn) => {
                if turn.ended.is_some() {
                    return Applied::Ignored;
                }
                turn.ended = Some(SystemTime::now());
                turn.stop_reason = stop_reason;
                turn.num_turns = num_turns;
                turn.cost_usd = cost_usd;
                turn.is_error = is_error;
                turn.error = if is_error {
                    Some(match &terminal {
                        Some(reason) => format!("{reason}: {message}"),
                        None => message.clone(),
                    })
                } else {
                    None
                };
            }
            // A `result` with no turn open: the console did not start this stream, so it will not
            // invent a turn to hang it on. It says what it read instead.
            None => {
                self.note(format!(
                    "a result envelope arrived with no turn open on this console: {}",
                    head(&message, 120)
                ));
                return Applied::Ignored;
            }
        }
        if is_error {
            let reason = terminal.unwrap_or_else(|| "an unstated reason".to_string());
            self.push(Item {
                key: None,
                uuid: None,
                kind: ItemKind::Refused,
                state: ItemState::Failed,
                text: format!("the turn failed ({reason}): {}", head(&message, 300)),
                at: SystemTime::now(),
            });
        }
        Applied::TurnEnded
    }

    /// Append a line, or update the line this key already owns.
    ///
    /// One line per key, always: a partial message that arrives again with more text is the same line
    /// being written, and `Applied::Updated` says so rather than the count growing.
    fn upsert(
        &mut self,
        key: Option<String>,
        uuid: Option<String>,
        kind: ItemKind,
        text: &str,
        failed: bool,
    ) -> Option<Applied> {
        let state = if failed {
            ItemState::Failed
        } else {
            ItemState::Live
        };
        if let Some(key) = &key {
            if let Some(existing) = self
                .items
                .iter_mut()
                .find(|item| item.key.as_deref() == Some(key.as_str()))
            {
                existing.text = text.to_string();
                if failed {
                    existing.state = ItemState::Failed;
                }
                if uuid.is_some() {
                    existing.uuid = uuid;
                }
                return Some(Applied::Updated);
            }
        }
        self.push(Item {
            key,
            uuid,
            kind,
            state,
            text: text.to_string(),
            at: SystemTime::now(),
        });
        Some(Applied::Item)
    }
}

/// The first `limit` characters of a string, never splitting a character.
fn head(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('\u{2026}');
    }
    out
}

/// The first eight characters of a session id, for a line that has to stay short.
fn short(session: &Option<String>) -> String {
    session
        .as_deref()
        .map(|id| id.chars().take(8).collect())
        .unwrap_or_else(|| "unread".to_string())
}

/// One line describing a tool call's own argument.
///
/// The stream names the tool and hands it a JSON object; the console draws the one field that says what
/// the call is about, and says so when it cannot find one rather than printing an empty pair of
/// brackets as though the call had no argument.
fn tool_argument(block: &serde_json::Value) -> String {
    let Some(input) = block.get("input") else {
        return "no argument was sent".to_string();
    };
    for key in ["command", "file_path", "path", "pattern", "url", "prompt"] {
        if let Some(value) = input.get(key).and_then(serde_json::Value::as_str) {
            return head(value, 160);
        }
    }
    head(&input.to_string(), 160)
}

/// The text a tool returned, first line only.
///
/// A result can be a string or a list of content blocks, so both shapes are read; anything else is
/// reported as unreadable rather than stringified into something that looks like output.
fn tool_result_text(block: &serde_json::Value) -> String {
    let content = block.get("content");
    let raw = match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<&str>>()
            .join(" "),
        Some(other) => format!(
            "(a result this console does not read: {})",
            head(&other.to_string(), 80)
        ),
        None => String::new(),
    };
    let first = raw
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    head(first.trim(), 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture line, as the console's own probe layer would hand it over.
    fn fixture(name: &str) -> Vec<String> {
        let path = format!(
            "{}/tests/fixtures/stream-json/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{path}: {error}"))
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Every `uuid` a capture carries: the identity the run's own record is matched against.
    fn uuids(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| {
                value
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    /// The envelope of a kind, from a fixture.
    fn envelope(name: &str, wanted: &str, subtype: Option<&str>) -> serde_json::Value {
        for line in fixture(name) {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if value.get("type").and_then(serde_json::Value::as_str) != Some(wanted) {
                continue;
            }
            match subtype {
                Some(subtype) => {
                    if value.get("subtype").and_then(serde_json::Value::as_str) == Some(subtype) {
                        return value;
                    }
                }
                None => return value,
            }
        }
        panic!("no {wanted} envelope in {name}");
    }

    fn conversation() -> Conversation {
        Conversation::new(400)
    }

    #[test]
    fn the_first_init_names_the_session_and_the_model_and_the_version() {
        let mut chat = conversation();
        let init = envelope("one-shot.jsonl", "system", Some("init"));
        assert_eq!(chat.apply_line(&init.to_string()), Applied::Ignored);
        assert_eq!(
            chat.session.as_deref(),
            Some("1f0a9c3e-0000-4000-8000-000000000001")
        );
        assert_eq!(chat.model.as_deref(), Some("claude-opus-5-5[1m]"));
        assert_eq!(chat.cli_version.as_deref(), Some("2.1.280"));
        // The console's own line, and its own state: it is not a claim about the run.
        assert_eq!(chat.items.len(), 1);
        assert_eq!(chat.items[0].kind, ItemKind::Console);
        assert_eq!(chat.items[0].state, ItemState::Own);
        assert!(chat.items[0].text.contains("claude 2.1.280"));
    }

    #[test]
    fn a_second_init_updates_the_fields_and_touches_no_items() {
        // The measured bug this rule exists for: every turn of a persistent stream emits its own
        // `init`, so a parser that treats it as "the session starts here" resets its transcript.
        let mut chat = conversation();
        let mut inits = 0usize;
        for line in fixture("realtime-two-turns.jsonl") {
            let value: serde_json::Value = serde_json::from_str(&line).expect("fixture json");
            let is_init = value.get("type").and_then(serde_json::Value::as_str) == Some("system")
                && value.get("subtype").and_then(serde_json::Value::as_str) == Some("init");
            if !is_init {
                chat.apply_line(&line);
                continue;
            }
            inits += 1;
            let before = chat.items.len();
            assert_eq!(
                chat.apply_line(&line),
                Applied::Ignored,
                "an init is read, not drawn"
            );
            if inits == 1 {
                assert_eq!(
                    chat.items.len(),
                    before + 1,
                    "the first init names the conversation and adds that one line"
                );
            } else {
                assert_eq!(
                    chat.items.len(),
                    before,
                    "init number {inits} appended an item"
                );
            }
            assert_eq!(
                chat.session.as_deref(),
                Some("1f0a9c3e-0000-4000-8000-000000000001")
            );
            assert_eq!(chat.model.as_deref(), Some("claude-opus-5-5[1m]"));
            assert_eq!(chat.cli_version.as_deref(), Some("2.1.280"));
        }
        assert_eq!(inits, 2, "the fixture must carry the second init");
        // The first turn's words are still here after the second turn started: that is the reset this
        // rule prevents, asserted directly rather than by a total that a later change would shift.
        assert_eq!(
            chat.items
                .iter()
                .filter(|item| item.kind == ItemKind::Agent)
                .count(),
            2,
            "both turns' words are in the buffer"
        );
        // This test never opened a turn, so neither `result` envelope has one to close. The console
        // did not start this stream, and it says so rather than inventing a turn to hang them on.
        assert!(chat.turns.is_empty());
        assert_eq!(
            chat.items
                .iter()
                .filter(|item| item.text.contains("with no turn open"))
                .count(),
            2,
            "both results are reported, not dropped"
        );
    }

    #[test]
    fn a_repeated_uuid_updates_its_own_item_rather_than_appending_one() {
        let mut chat = conversation();
        let assistant = envelope("one-shot.jsonl", "assistant", None);
        let uuid = assistant
            .get("uuid")
            .and_then(serde_json::Value::as_str)
            .expect("the fixture carries a uuid")
            .to_string();
        let partial = |text: &str| {
            let mut value = assistant.clone();
            value["message"]["content"] = serde_json::json!([{ "type": "text", "text": text }]);
            value.to_string()
        };
        assert_eq!(chat.apply_line(&partial("the answer")), Applied::Item);
        assert_eq!(
            chat.apply_line(&partial("the answer, streamed")),
            Applied::Updated
        );
        assert_eq!(
            chat.apply_line(&partial("the answer, streamed, finished")),
            Applied::Updated
        );
        let matching: Vec<&Item> = chat
            .items
            .iter()
            .filter(|item| item.uuid.as_deref() == Some(uuid.as_str()))
            .collect();
        assert_eq!(matching.len(), 1, "one uuid is one item");
        assert_eq!(matching[0].text, "the answer, streamed, finished");
    }

    #[test]
    fn a_failed_turn_is_read_from_is_error_and_never_from_subtype() {
        // Measured: the CLI reports a rate-limited turn as "subtype":"success" with "is_error":true.
        let mut chat = conversation();
        chat.open_turn("do the thing");
        let result = envelope("one-shot.jsonl", "result", None);
        assert_eq!(
            result.get("subtype").and_then(serde_json::Value::as_str),
            Some("success"),
            "the fixture must carry the trap this test exists for"
        );
        assert_eq!(chat.apply_line(&result.to_string()), Applied::TurnEnded);
        let turn = chat.last_turn().expect("a turn was open");
        assert!(turn.ended.is_some());
        assert!(
            turn.is_error,
            "subtype says success; is_error says otherwise"
        );
        let error = turn.error.clone().unwrap_or_default();
        assert!(error.contains("api_error"), "{error}");
        assert!(
            chat.items.iter().any(|item| item.kind == ItemKind::Refused
                && item.state == ItemState::Failed
                && item.text.contains("rate limit")),
            "the API's own words must reach the transcript"
        );
    }

    #[test]
    fn an_unknown_type_is_reported_and_a_non_json_line_is_reported() {
        let mut chat = conversation();
        chat.apply_line(&envelope("one-shot.jsonl", "system", Some("init")).to_string());
        let before = chat.items.len();
        assert_eq!(
            chat.apply_line(r#"{"type":"whatever","session_id":"x"}"#),
            Applied::Unknown
        );
        assert_eq!(chat.apply_line("not json"), Applied::Unknown);
        assert_eq!(chat.apply_line("   "), Applied::Ignored);
        assert_eq!(
            chat.items.len(),
            before + 2,
            "both are reported, not dropped"
        );
        assert!(chat.items[chat.items.len() - 2].text.contains("whatever"));
        assert!(chat.items[chat.items.len() - 1]
            .text
            .contains("was not JSON"));
        // Both are the console's own lines: a shape it does not read says nothing about the run.
        assert_eq!(chat.items[chat.items.len() - 1].state, ItemState::Own);
    }

    #[test]
    fn a_partial_chunk_is_counted_and_not_drawn_per_token() {
        let mut chat = conversation();
        let before = chat.items.len();
        for _ in 0..5 {
            assert_eq!(
                chat.apply_line(
                    r#"{"type":"stream_event","event":{"type":"content_block_delta"}}"#
                ),
                Applied::Ignored
            );
        }
        assert_eq!(chat.partial_events, 5);
        assert_eq!(chat.items.len(), before, "no item per chunk");
    }

    #[test]
    fn a_turn_closes_once_whichever_door_closes_it() {
        // Envelope first, then the exit status.
        let mut chat = conversation();
        chat.open_turn("first");
        chat.apply_line(&envelope("one-shot.jsonl", "result", None).to_string());
        let after_envelope = chat.last_turn().and_then(|turn| turn.ended);
        assert!(after_envelope.is_some());
        chat.close_turn("exited with code 1");
        assert_eq!(
            chat.last_turn().and_then(|turn| turn.ended),
            after_envelope,
            "the exit status must not rewrite a turn the run itself closed"
        );

        // Exit status first, then the envelope.
        let mut chat = conversation();
        chat.open_turn("second");
        chat.close_turn("was killed by signal 15 (SIGTERM)");
        let closed = chat.last_turn().and_then(|turn| turn.ended);
        assert!(closed.is_some());
        assert!(chat.last_turn().expect("open").is_error);
        assert_eq!(
            chat.last_turn().and_then(|turn| turn.ended),
            closed,
            "a turn already closed stays closed"
        );
    }

    #[test]
    fn a_result_with_no_open_turn_is_reported_rather_than_invented_into_one() {
        let mut chat = conversation();
        assert_eq!(
            chat.apply_line(&envelope("one-shot.jsonl", "result", None).to_string()),
            Applied::Ignored
        );
        assert!(chat.turns.is_empty(), "no turn is invented");
        assert!(chat
            .items
            .iter()
            .any(|item| item.text.contains("with no turn open")));
    }

    #[test]
    fn the_runs_own_words_are_confirmed_against_its_own_record_and_nothing_else_is() {
        let mut chat = conversation();
        for line in fixture("one-shot.jsonl") {
            chat.apply_line(&line);
        }
        assert_eq!(chat.confirmed(), 0, "nothing is confirmed before the read");
        let live = chat.live();
        assert_eq!(
            live, 1,
            "the assistant message is the only claim about the run"
        );
        let owned = chat.own();
        assert!(owned >= 2, "hooks and the init line are this console's own");
        let confirmed = chat.confirm_from(&fixture("session-record.jsonl"));
        assert_eq!(confirmed, 1, "the fixture shares exactly one uuid");
        assert_eq!(chat.live(), 0);
        assert_eq!(chat.confirmed(), 1);
        assert_eq!(
            chat.own(),
            owned,
            "no console line became confirmed, and the model added no line of its own: only the item's \
             own state moves"
        );
    }

    /// A read that matches nothing confirms nothing, and the model says nothing about *why*: whether
    /// the read could see the whole record or only a bounded tail of it is a fact about the read, and
    /// only the caller that made it knows. `App::reconcile` owns that sentence.
    #[test]
    fn a_read_that_matches_nothing_confirms_nothing_and_writes_no_prose() {
        let mut chat = conversation();
        for line in fixture("one-shot.jsonl") {
            chat.apply_line(&line);
        }
        let before = chat.items.len();
        assert_eq!(chat.confirm_from(&[]), 0);
        assert_eq!(chat.live(), 1, "nothing was quietly confirmed");
        assert_eq!(
            chat.items.len(),
            before,
            "and the model did not add a line of its own about the read"
        );
        assert!(
            !chat
                .items
                .iter()
                .any(|item| item.text.contains("bounded tail")),
            "the bounded-tail sentence belongs to whoever made the read, not to the model"
        );
    }

    /// **Task 0.5's real finding, pinned.** A real turn emitted `system/thinking_tokens` three times, and
    /// three identical lines of the console's own words bury the lines worth reading. One line names the
    /// shape and counts the repeats: nothing is dropped, and the repetition is on the glass.
    #[test]
    fn a_repeated_unfamiliar_shape_is_one_line_with_its_count() {
        let mut chat = conversation();
        for _ in 0..3 {
            chat.apply_line(r#"{"type":"system","subtype":"thinking_tokens","session_id":"x"}"#);
        }
        let lines: Vec<&str> = chat
            .items
            .iter()
            .filter(|item| item.text.contains("thinking_tokens"))
            .map(|item| item.text.as_str())
            .collect();
        assert_eq!(lines.len(), 1, "one line, however many arrived: {lines:?}");
        assert!(
            lines[0].contains("is not one this console reads"),
            "the shape is still named: {:?}",
            lines[0]
        );
        assert!(
            lines[0].contains("(3 so far)"),
            "and the count is in the line: {:?}",
            lines[0]
        );
        assert_eq!(
            chat.own(),
            1,
            "the counted line is this console's own, not a claim about the run"
        );
    }

    /// A resumed turn adds only its own items and shares no uuid with the turn before it: `--resume`
    /// replays nothing, so no cross-invocation dedupe is needed and a future CLI that starts replaying
    /// history fails here instead of silently doubling a transcript.
    #[test]
    fn a_resumed_turn_adds_only_its_own_items_and_shares_no_uuid_with_the_turn_before_it() {
        let mut chat = conversation();
        for line in fixture("one-shot.jsonl") {
            chat.apply_line(&line);
        }
        let after_first = chat.items.len();
        let first_uuids = uuids(&fixture("one-shot.jsonl"));
        assert!(
            !first_uuids.is_empty(),
            "the first turn's capture carries uuids, or this test proves nothing"
        );

        for line in fixture("resumed.jsonl") {
            chat.apply_line(&line);
        }
        let after_second = chat.items.len();
        let second_uuids = uuids(&fixture("resumed.jsonl"));
        assert!(
            !second_uuids.is_empty(),
            "the resumed capture carries uuids, or this test proves nothing"
        );

        let shared: Vec<&String> = first_uuids
            .iter()
            .filter(|uuid| second_uuids.contains(uuid))
            .collect();
        assert!(
            shared.is_empty(),
            "--resume replayed {shared:?}: the parser now needs a cross-invocation dedupe, because the \
             CLI has stopped behaving as measured"
        );
        assert!(
            after_second > after_first,
            "the resumed turn's items land in the same buffer, and none of the first turn's are lost"
        );
    }

    #[test]
    fn the_ring_drops_loudly_and_counts_what_it_dropped() {
        let mut chat = Conversation::new(3);
        for index in 0..10 {
            chat.note(format!("line {index}"));
        }
        assert_eq!(chat.items.len(), 3);
        assert_eq!(chat.dropped, 7);
        assert_eq!(chat.items[0].text, "line 7");
        let header = chat.header_line();
        assert!(header.contains("7 earlier item(s) dropped"), "{header}");
        assert!(header.contains("the 3-item ring"), "{header}");
    }

    #[test]
    fn the_header_line_counts_confirmed_live_and_the_consoles_own_separately() {
        let mut chat = conversation();
        chat.note("a line this console wrote");
        chat.push(Item {
            key: Some("u1".to_string()),
            uuid: Some("u1".to_string()),
            kind: ItemKind::Agent,
            state: ItemState::Live,
            text: "the run's own words".to_string(),
            at: SystemTime::now(),
        });
        chat.push(Item {
            key: Some("u2".to_string()),
            uuid: Some("u2".to_string()),
            kind: ItemKind::Agent,
            state: ItemState::Confirmed,
            text: "corroborated".to_string(),
            at: SystemTime::now(),
        });
        let header = chat.header_line();
        assert!(header.contains("3 item(s)"), "{header}");
        assert!(header.contains("1 confirmed"), "{header}");
        assert!(header.contains("1 live"), "{header}");
        assert!(header.contains("1 this console's own"), "{header}");
    }

    #[test]
    fn every_stream_fixture_parses_without_inventing_anything() {
        // The parser is fed the CLI's real output, so a shape the fixtures carry and the parser does
        // not read shows up here rather than in a live run. The session record is deliberately not in
        // this list: it is the CLI's transcript format, read by `confirm_from` and never fed to the
        // stream parser.
        for name in [
            "one-shot.jsonl",
            "resumed.jsonl",
            "realtime-two-turns.jsonl",
        ] {
            let mut chat = conversation();
            let mut unknowns = 0usize;
            for line in fixture(name) {
                if chat.apply_line(&line) == Applied::Unknown {
                    unknowns += 1;
                }
            }
            assert_eq!(
                unknowns, 0,
                "{name} carried a shape the parser does not read"
            );
            assert!(chat.session.is_some(), "{name} named no session");
        }
    }

    #[test]
    fn a_tool_call_and_its_result_are_read_from_the_shapes_the_parser_expects() {
        // NOT measured against a real capture: every Phase 0 turn failed at the API before a tool ran,
        // so these two block shapes are the ones the CLI documents rather than ones this console has
        // seen. `docs/spikes/prompt-surface-protocol.md` section 5 carries the gate that closes this,
        // and this test is written so that a real capture can replace its two lines without touching
        // the parser.
        let mut chat = conversation();
        let tool_use = serde_json::json!({
            "type": "assistant",
            "uuid": "aaaaaaaa-0000-4000-8000-000000000001",
            "message": { "content": [
                { "type": "text", "text": "Running the tests." },
                { "type": "tool_use", "id": "toolu_01", "name": "Bash",
                  "input": { "command": "cargo test --release" } }
            ]}
        });
        assert_eq!(chat.apply_line(&tool_use.to_string()), Applied::Item);
        let tool: Vec<&Item> = chat
            .items
            .iter()
            .filter(|item| item.kind == ItemKind::Tool)
            .collect();
        assert_eq!(tool.len(), 1);
        assert!(
            tool[0].text.contains("Bash(cargo test --release)"),
            "{:?}",
            tool[0].text
        );
        // The call's own id keys it; the message's uuid is what the record can corroborate.
        assert_eq!(tool[0].key.as_deref(), Some("call:toolu_01"));
        assert_eq!(
            tool[0].uuid.as_deref(),
            Some("aaaaaaaa-0000-4000-8000-000000000001")
        );

        let tool_result = serde_json::json!({
            "type": "user",
            "uuid": "bbbbbbbb-0000-4000-8000-000000000002",
            "message": { "content": [
                { "type": "tool_result", "tool_use_id": "toolu_01", "is_error": false,
                  "content": "test result: ok. 80 passed; 0 failed\nsecond line" }
            ]}
        });
        assert_eq!(chat.apply_line(&tool_result.to_string()), Applied::Item);
        let result: Vec<&Item> = chat
            .items
            .iter()
            .filter(|item| item.kind == ItemKind::Result)
            .collect();
        assert_eq!(result.len(), 1);
        assert!(result[0].text.contains("80 passed"), "{:?}", result[0].text);
        assert!(
            !result[0].text.contains("second line"),
            "one line, not the whole dump"
        );
        assert_eq!(result[0].state, ItemState::Live);

        let failed = serde_json::json!({
            "type": "user",
            "uuid": "cccccccc-0000-4000-8000-000000000003",
            "message": { "content": [
                { "type": "tool_result", "tool_use_id": "toolu_02", "is_error": true,
                  "content": "error: no such file" }
            ]}
        });
        assert_eq!(chat.apply_line(&failed.to_string()), Applied::Item);
        let failed_item = chat
            .items
            .iter()
            .find(|item| item.kind == ItemKind::Result && item.state == ItemState::Failed)
            .expect("a failed result is Failed");
        assert!(failed_item.text.contains("no such file"));
    }

    #[test]
    fn a_replayed_user_message_does_not_double_the_operators_own_words() {
        let mut chat = conversation();
        chat.open_turn("run the fmt gate");
        let echo = serde_json::json!({
            "type": "user",
            "uuid": "dddddddd-0000-4000-8000-000000000004",
            "message": { "content": [ { "type": "text", "text": "run the fmt gate" } ] }
        });
        assert_eq!(chat.apply_line(&echo.to_string()), Applied::Ignored);
        let you: Vec<&Item> = chat
            .items
            .iter()
            .filter(|item| item.kind == ItemKind::You)
            .collect();
        assert_eq!(you.len(), 1, "the operator's words appear once");
    }
}
