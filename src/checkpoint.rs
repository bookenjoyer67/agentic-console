//! The checkpoint card: the one UI element that turns a state reading into a human decision.
//!
//! The orchestration runs headless: `claude -p "<brief>" --agent orchestrator --permission-mode
//! acceptEdits`. At a human checkpoint the orchestrator ends its turn and the CLI process exits, and
//! the next phase is a separate invocation that resumes **one named session**: `claude --resume
//! <session-id> -p "<ruling>" --permission-mode acceptEdits`. Two invocations of the same session,
//! then: a process being in flight means the run is *working*, never waiting, and the waiting
//! happens after the process is gone. So the actionable state is a checkpoint the evidence names
//! while nothing is running -- the run stopped there and the ruling is what resumes it -- and a
//! checkpoint named while a process is alive only means the checkpoint lies ahead of that process.
//!
//! The console never decides that a checkpoint is open. It looks for evidence, in this order, and
//! says which piece of evidence it used:
//!
//! 1. the prompt of a run that is in flight in the container right now (the `-p` argument of the
//!    agent process): what that run was told to do, including any checkpoint it is told to stop at;
//! 2. the tail of the **session's own transcript** in the container
//!    (`<console.session_dir>/<session-id>.jsonl`), read through the probe layer for the session the
//!    card names: the run's own words, and the one read that settles the checkpoint on its own;
//! 3. an evidence-directory transcript, but only one that is **attributable to that session** -- its
//!    own file name carries the session id, or its own metadata block names it. A transcript there
//!    that carries no session id is somebody else's copy until it says otherwise, so it is ignored
//!    and the card says so;
//! 4. the storage journal's supporting shape: a plan entry with no implementation entry after it.
//!
//! The journal is corroboration, not a tie-breaker: when its shape and the session's own transcript
//! name different checkpoints the card reports the disagreement and offers no ruling, the same way
//! it treats two reads that name two different sessions. Nothing here invents a checkpoint, and
//! nothing here lets one run's evidence name another run's checkpoint.
//!
//! A ruling also has to say *which* conversation it is resuming, and `--continue` -- resume the
//! newest session -- cannot: with two runs in one container it sends the ruling into whichever
//! session wrote last, whoever started it. So the card names one session too, from its own read of
//! the container's session directory (the CLI writes one `<session-id>.jsonl` per session, so the
//! file's name is the id), and the ruling is built with `--resume <that-id>` or refused. A refusal
//! is the right answer when the evidence cannot name exactly one session: resuming the wrong
//! conversation is worse than not resuming at all.

use serde_json::Value;

use crate::actions::{self, ActionKind, Guards};
use crate::config::Config;
use crate::iso;
use crate::probe;
use crate::probe::SessionFile;
use crate::state::{Light, Probes};

use std::time::{Duration, SystemTime};

/// The card's state.
///
/// Three states, because two questions have to be answered separately: does the evidence name a
/// checkpoint, and is a process running right now. The orchestration is headless, so a process being
/// in flight means the run is working, not stopped: at a human checkpoint the orchestrator ends its
/// turn and the CLI process exits, and the session continues in a separate invocation. The one state
/// a human has to act on is therefore the evidence naming a checkpoint while nothing is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardState {
    /// The evidence names a checkpoint and no process is in flight: the run stopped there and its
    /// process is gone, so the ruling the operator writes is what resumes the session. This is the
    /// actionable state -- and only when the evidence also names exactly one session to resume, which
    /// is a separate reading the card shows beside it.
    Stopped,
    /// A process is in flight: the run is working right now, so any checkpoint named by the evidence
    /// lies ahead of it rather than being waited on. A ruling is refused here, because resuming that
    /// session would start a second process on the same session jsonl -- two writers on one session.
    Running,
    /// No run and no checkpoint evidence.
    Idle,
}

impl CardState {
    /// The word the card prints for this state.
    ///
    /// The stopped state's word names the checkpoint it stopped at -- the name comes from the
    /// evidence (`Card::which`), so the band reads `STOPPED AT HUMAN CHECKPOINT 1 (plan approval) --
    /// A RULING RESUMES IT` rather than a word that would fit any checkpoint. The other two states
    /// read the same whatever the evidence named.
    pub fn word(self, which: &str) -> String {
        match self {
            CardState::Stopped => format!("STOPPED AT {which} -- A RULING RESUMES IT"),
            CardState::Running => "RUN IN FLIGHT".to_string(),
            CardState::Idle => "NO RUN, NO CHECKPOINT".to_string(),
        }
    }

    /// The state's name, for the readings that have to fit it into one word.
    pub fn basis(self) -> &'static str {
        match self {
            CardState::Stopped => "stopped",
            CardState::Running => "running",
            CardState::Idle => "idle",
        }
    }

    /// What the state rests on, in one sentence. Every screen that names the state prints this
    /// sentence with it, so the state word is never read without its evidence basis.
    pub fn basis_note(self) -> &'static str {
        match self {
            CardState::Stopped => {
                "the evidence names a checkpoint and no process is running, so the run stopped \
                 there; the ruling resumes the session"
            }
            CardState::Running => {
                "a process is running in the container, so the run is working and any checkpoint \
                 lies ahead of it; the ruling is refused until that process stops"
            }
            CardState::Idle => "no run in flight and no checkpoint named by any evidence",
        }
    }

    /// The state's name and what it rests on, as one line: the status bar, the confirmation screen,
    /// the guards summary and `--dump` all print this, so no screen carries its own wording.
    pub fn basis_line(self) -> String {
        format!("{} -- {}", self.basis(), self.basis_note())
    }
}

/// The checkpoint card's model.
#[derive(Clone, Debug)]
pub struct Card {
    pub state: CardState,
    pub which: String,
    pub question: String,
    pub evidence: Vec<String>,
    pub can_approve: bool,
    pub refuse_reason: Option<String>,
    pub command_preview: String,
    /// Which session a ruling would resume, with the read that named it and whether the reads
    /// agree, so the operator can see the session and the checkpoint side by side before acting.
    pub session: SessionEvidence,
    /// Why the checkpoint is contested, when two attributable reads name two different checkpoints.
    ///
    /// `Some` means the card shows one checkpoint from the strongest read and refuses the ruling in
    /// the same words, rather than quietly preferring one read over another. `None` is the ordinary
    /// case: the reads that named the checkpoint agree, or only one of them named one.
    pub checkpoint_conflict: Option<String>,
    /// The session's own transcript read, as its own source and age: the primary evidence.
    pub transcript_read: String,
    /// The evidence-directory listing, as its own source and age: how many transcripts are there
    /// and how many carry a session id of their own.
    pub evidence_dir_read: String,
}

impl Card {
    /// The word the card's band prints: the state's own word, unless the checkpoint is contested --
    /// in that case no ruling is offered, so the band cannot read as though one resumes the run.
    pub fn word(&self) -> String {
        match &self.checkpoint_conflict {
            Some(_) => format!(
                "STOPPED AT {} -- THE EVIDENCE DISAGREES: NO RULING OFFERED",
                self.which
            ),
            None => self.state.word(&self.which),
        }
    }

    /// The light and the one-line status the FLOW map shows on a checkpoint box.
    pub fn light_and_status(&self) -> (Light, String) {
        let light = match self.state {
            // The actionable state: the run stopped here and one act resumes it.
            CardState::Stopped => Light::Ok,
            // Work in progress, not a warning: a process is running, so no decision is waiting.
            CardState::Running => Light::Human,
            CardState::Idle => Light::Human,
        };
        (light, self.word())
    }
}

/// The phrases a run uses to say a checkpoint is **not** the one it stopped at.
///
/// A finished run's closing summary names the checkpoint it stopped at *and*, in the same breath,
/// the one it never reached -- `Checkpoint 2 (release approval): not reached`. The second of those
/// names no stop; reading it as one would have the console quote the run's own words backwards and
/// settle the card on the checkpoint the run says is still ahead of it.
const NEGATIONS: [&str; 6] = [
    "not reached",
    "never reached",
    "not cleared",
    "not taken",
    "not started",
    "not yet approved",
];

/// Every `checkpoint 1` / `checkpoint 2` mention in a lowered text, in the order it appears.
fn checkpoint_mentions(lowered: &str) -> Vec<(usize, u8)> {
    let mut found: Vec<(usize, u8)> = Vec::new();
    for (needle, index) in [
        ("checkpoint 1", 1u8),
        ("checkpoint one", 1),
        ("checkpoint 2", 2),
        ("checkpoint two", 2),
    ] {
        let mut from = 0usize;
        while let Some(at) = lowered[from..].find(needle) {
            let position = from + at;
            found.push((position, index));
            from = position + needle.len();
        }
    }
    found.sort();
    found
}

/// The checkpoint a piece of text names, and where in the text the mention is.
///
/// The newest mention wins, because both places a run names a checkpoint end with the decision it
/// is on: a brief ends `... STOP at HUMAN CHECKPOINT 1 (plan approval)` and a closing summary ends
/// `Checkpoint 1 (plan approval): PENDING, this is where the run is stopped`. A mention the words
/// around it negate names no stop and is skipped, so the two mentions inside one summary do not
/// cancel each other out and the not-reached one cannot win by being written last.
fn named_mention(text: &str) -> Option<(String, usize)> {
    // ASCII lowercasing only: the needles are ASCII, and it leaves every byte offset in `text`
    // usable as a slice boundary, so the quote can be cut from the original characters.
    let lowered = text.to_ascii_lowercase();
    let mut newest: Option<(u8, usize)> = None;
    for (position, index) in checkpoint_mentions(&lowered) {
        let window: String = lowered[position..].chars().take(80).collect();
        if NEGATIONS.iter().any(|phrase| window.contains(phrase)) {
            continue;
        }
        newest = Some((index, position));
    }
    newest.map(|(index, position)| (checkpoint_name(index), position))
}

/// The checkpoint a piece of text names, when it names one.
pub fn mentioned(text: &str) -> Option<String> {
    named_mention(text).map(|(name, _)| name)
}

/// The readable prose of one transcript line.
///
/// A session transcript line is one JSON record: the run's own prose is nested inside it and the
/// rest is bookkeeping. The console scans and quotes the prose, so what the card prints as "the
/// run's own words" is what the run wrote rather than the JSON envelope around it. A line that is
/// not a record at all (the harness's plain `.txt` copies) is its own text.
fn readable_line(line: &str) -> String {
    let trimmed = line.trim();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return trimmed.to_string();
    };
    let mut out: Vec<String> = Vec::new();
    collect_text(&value, &mut out);
    if out.is_empty() {
        trimmed.to_string()
    } else {
        out.join(" ")
    }
}

/// Every string a transcript record holds under a key that carries prose, in order.
fn collect_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_text(item, out)),
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "text" | "content" | "message" | "toolUseResult"
                ) {
                    match child {
                        Value::String(text) => out.push(text.clone()),
                        other => collect_text(other, out),
                    }
                }
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// A quote around a mention: the words on either side of it, so the line the card prints carries
/// the sentence the reading came from rather than the opening of a long message.
fn quote_around(text: &str, position: usize) -> String {
    let characters: Vec<char> = text.chars().collect();
    let index = text[..position].chars().count();
    let start = index.saturating_sub(50);
    let end = (index + 90).min(characters.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(characters[start..end].iter());
    if end < characters.len() {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// The canonical name of a checkpoint, as `CLAUDE.md`'s ordered sequence writes it.
pub fn checkpoint_name(index: u8) -> String {
    match index {
        1 => "HUMAN CHECKPOINT 1 (plan approval)".to_string(),
        _ => "HUMAN CHECKPOINT 2 (release approval)".to_string(),
    }
}

/// The question a checkpoint asks, as the orchestration's ordered sequence frames it.
fn checkpoint_question(which: &str) -> String {
    if which.contains("CHECKPOINT 1") {
        "Approve or amend the plan before any implementation work runs?".to_string()
    } else if which.contains("CHECKPOINT 2") {
        "Approve the merge, or halt, before anything reaches main?".to_string()
    } else {
        "The run's own prompt names the decision; read it in the transcript".to_string()
    }
}

/// The session a ruling would resume, as the console read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRef {
    /// The session id, exactly as the container's transcript file names it.
    pub id: String,
    /// The transcript's path inside the container.
    pub path: String,
    pub size: u64,
    /// The age of the transcript's own mtime, formatted from the reading's timestamp.
    pub age: String,
    /// Which read named this session, in the card's own words.
    pub named_by: String,
    /// The reading's own source line, so the id's provenance can be checked.
    pub source: String,
}

impl SessionRef {
    /// The first eight characters of the id: enough to tell two sessions apart on one line.
    pub fn short_id(&self) -> String {
        short_id(&self.id)
    }
}

/// The container's session-directory reading, as every renderer prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRead {
    /// The read's own source line, e.g. the `docker exec ... find ...` that listed the directory.
    pub source: String,
    /// The age of that reading.
    pub age: String,
    /// How many session transcripts the read returned.
    pub candidates: usize,
}

/// Which session the evidence names -- the one a ruling resumes -- or why it names none.
///
/// Two variants and no third: either exactly one session is named, or the evidence does not name
/// one and the ruling is refused with the reason. There is deliberately no "probably this one".
#[derive(Clone, Debug)]
pub enum SessionEvidence {
    /// Exactly one session is named. The ruling resumes this one and no other.
    Named {
        session: SessionRef,
        read: SessionRead,
        notes: Vec<String>,
    },
    /// The evidence cannot name one session. `reason` is the refusal, in the card's own words, so
    /// the card, the confirmation screen and the dry run all refuse in the same sentence.
    Unnamed {
        reason: String,
        read: SessionRead,
        notes: Vec<String>,
    },
}

impl SessionEvidence {
    /// Nothing was read: a caller that took no session reading must not read as one.
    pub fn not_read() -> SessionEvidence {
        SessionEvidence::Unnamed {
            reason: "no session reading was taken, so nothing names a session to resume"
                .to_string(),
            read: SessionRead {
                source: "no reading".to_string(),
                age: "n/a".to_string(),
                candidates: 0,
            },
            notes: Vec::new(),
        }
    }

    /// The session the ruling resumes, when the evidence names one.
    pub fn named(&self) -> Option<&SessionRef> {
        match self {
            SessionEvidence::Named { session, .. } => Some(session),
            SessionEvidence::Unnamed { .. } => None,
        }
    }

    /// Why no ruling may be built, when none may be.
    pub fn refusal(&self) -> Option<&str> {
        match self {
            SessionEvidence::Named { .. } => None,
            SessionEvidence::Unnamed { reason, .. } => Some(reason),
        }
    }

    /// What the card prints under the session: which read named it, and whether the reads agree.
    pub fn notes(&self) -> &[String] {
        match self {
            SessionEvidence::Named { notes, .. } | SessionEvidence::Unnamed { notes, .. } => notes,
        }
    }

    /// The reading behind the choice.
    pub fn read(&self) -> &SessionRead {
        match self {
            SessionEvidence::Named { read, .. } | SessionEvidence::Unnamed { read, .. } => read,
        }
    }

    /// The one line the card, the dump and the dry run all print for the session: the short id, the
    /// container path and the age, or the reason no session is named.
    pub fn line(&self) -> String {
        match self {
            SessionEvidence::Named { session, .. } => format!(
                "{}  {}  (age {})",
                session.short_id(),
                session.path,
                session.age
            ),
            SessionEvidence::Unnamed { reason, .. } => format!("none named -- {reason}"),
        }
    }

    /// Which read named the session, or that none did.
    pub fn source_line(&self) -> String {
        match self {
            SessionEvidence::Named { session, .. } => session.named_by.clone(),
            SessionEvidence::Unnamed { .. } => "no session is named by this reading".to_string(),
        }
    }

    /// The session as the guards summary prints it, on one line.
    pub fn summary(&self) -> String {
        match self {
            SessionEvidence::Named { session, .. } => format!("session={}", session.short_id()),
            SessionEvidence::Unnamed { .. } => "session=none".to_string(),
        }
    }
}

/// The first eight characters of a session id.
pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// What the reads that named the checkpoint said about a session, if anything.
///
/// Of the three reads that can name a checkpoint, two can carry a session id at all: an in-flight
/// command that is already resuming a session names it, and a transcript whose file name carries
/// the session's own uuid names it. This carries every id those reads named, with the read that
/// named it, so two reads that name two different sessions are visible rather than whoever spoke
/// first.
#[derive(Clone, Debug, Default)]
pub struct SessionNaming {
    /// The read that named the checkpoint, as the card's own evidence line words it.
    pub checkpoint_from: Option<String>,
    /// Every session id a read named, with the read that named it.
    pub names: Vec<(String, String)>,
}

impl SessionNaming {
    /// The read that named the checkpoint, or a sentence saying no read did.
    fn checkpoint_read(&self) -> String {
        self.checkpoint_from
            .clone()
            .unwrap_or_else(|| "no read named a checkpoint".to_string())
    }
}

/// Whether a session was written inside `window` of the newest session's own mtime.
fn within_window(file: &SessionFile, newest: &SessionFile, window: Duration) -> bool {
    match (file.modified, newest.modified) {
        (Some(then), Some(latest)) => latest.duration_since(then).is_ok_and(|age| age <= window),
        // Without mtimes nothing can be compared, and the only session that can be named on this
        // evidence is the newest itself.
        _ => file.id == newest.id,
    }
}

/// Which session the ruling resumes, resolved from the reads -- or why none is named.
///
/// The rule, in the order it is applied:
///
/// 1. a read that named the checkpoint and carries a session id **names** that session, and the
///    container's session directory has to carry it; the id wins even when a newer session exists,
///    because the newer one is exactly the conversation the ruling is not for;
/// 2. two reads naming two different sessions is not one session: refuse, naming both;
/// 3. with no id named anywhere, the container's own recency is the only evidence left, and it has
///    to name one: exactly one session written inside `console.session_window_seconds` of the
///    newest. Two sessions inside that window is the run-in-one-container, harness-in-the-other
///    case -- the newest is not knowably the right one, so the ruling is refused rather than aimed
///    at it;
/// 4. a read that failed, or a directory with no transcript at all, names nothing: refuse.
pub fn resolve_session(cfg: &Config, probes: &Probes, naming: &SessionNaming) -> SessionEvidence {
    let where_from = format!(
        "{} inside {}",
        cfg.console.session_dir, cfg.console.container
    );
    let reading = &probes.sessions;
    let read = SessionRead {
        source: reading.source.clone(),
        age: iso::age_text(reading.at, probes.at),
        candidates: reading.value.len(),
    };
    let mut notes: Vec<String> = naming
        .names
        .iter()
        .map(|(id, from)| format!("a read names session {id}: {from}"))
        .collect();
    if let Some(error) = &reading.error {
        return SessionEvidence::Unnamed {
            reason: format!(
                "the session directory {where_from} could not be read ({error}), so the evidence \
                 cannot name one session and nothing here invents an id"
            ),
            read,
            notes,
        };
    }
    let Some(newest) = reading.value.first() else {
        return SessionEvidence::Unnamed {
            reason: format!(
                "{where_from} carries no session transcript (*.jsonl), so there is no session a \
                 ruling could resume"
            ),
            read,
            notes,
        };
    };
    let newest_age = match newest.modified {
        Some(modified) => iso::format_age(modified, probes.at),
        None => "mtime unknown".to_string(),
    };
    // Every id the checkpoint's own reads named, deduplicated: one id is a name, two are not.
    let mut distinct: Vec<&str> = Vec::new();
    for (id, _) in &naming.names {
        if !distinct.contains(&id.as_str()) {
            distinct.push(id.as_str());
        }
    }
    if distinct.len() > 1 {
        return SessionEvidence::Unnamed {
            reason: format!(
                "the reads that named the checkpoint name {} different sessions ({}), so the \
                 evidence does not name one session",
                distinct.len(),
                distinct.join(", ")
            ),
            read,
            notes,
        };
    }
    if let Some(id) = distinct.first() {
        let named_by = naming
            .names
            .first()
            .map(|(_, from)| from.clone())
            .unwrap_or_default();
        return match reading.value.iter().find(|file| file.id == *id) {
            Some(found) => {
                if found.id == newest.id {
                    notes.insert(
                        0,
                        format!(
                            "the session the checkpoint's own read names is also the newest session \
                             in {where_from} ({newest_age})"
                        ),
                    );
                } else {
                    notes.insert(
                        0,
                        format!(
                            "the container's newest session is {} ({newest_age}), a different one: \
                             newer, and this console does not resume it -- the ruling resumes the \
                             session the checkpoint's own read names",
                            short_id(&newest.id)
                        ),
                    );
                }
                SessionEvidence::Named {
                    session: session_ref(found, named_by, &read, probes.at),
                    read,
                    notes,
                }
            }
            None => SessionEvidence::Unnamed {
                reason: format!(
                    "the read that named the checkpoint names session {id}, and {where_from} does \
                     not carry it (it carries {} session(s), newest {} ({newest_age})): the two \
                     reads disagree, so the evidence does not name one session",
                    reading.value.len(),
                    short_id(&newest.id)
                ),
                read,
                notes,
            },
        };
    }
    // No read named an id: the container's own recency has to name exactly one session. The
    // checkpoint's read and this one are different reads, and the card says so.
    let window = cfg.session_window();
    let candidates: Vec<&SessionFile> = reading
        .value
        .iter()
        .filter(|file| within_window(file, newest, window))
        .collect();
    let window_text = iso::format_age(probes.at - window, probes.at);
    if candidates.len() == 1 {
        notes.push(format!(
            "the session and the checkpoint came from different reads: the session from {}, the \
             checkpoint from {} -- no single read names both, so the card shows the two to be \
             checked against each other",
            reading.source,
            naming.checkpoint_read()
        ));
        notes.push(format!(
            "it is the only session of {} written inside the {window_text} window the config \
             allows, and the newest by mtime",
            reading.value.len()
        ));
        SessionEvidence::Named {
            session: session_ref(
                newest,
                format!(
                    "{where_from}: the newest by mtime, and the only session written inside the \
                     {window_text} window"
                ),
                &read,
                probes.at,
            ),
            read,
            notes,
        }
    } else {
        let next = candidates.get(1).copied().unwrap_or(newest);
        let next_age = match next.modified {
            Some(modified) => iso::format_age(modified, probes.at),
            None => "mtime unknown".to_string(),
        };
        SessionEvidence::Unnamed {
            reason: format!(
                "{} sessions in {where_from} were written inside the {window_text} window the \
                 config allows (newest {} ({newest_age}), next {} ({next_age})), and the read that \
                 named the checkpoint ({}) names no session id: the evidence cannot say which one \
                 the run stopped in, so no ruling is offered -- resuming the wrong conversation is \
                 worse than not resuming",
                candidates.len(),
                short_id(&newest.id),
                short_id(&next.id),
                naming.checkpoint_read()
            ),
            read,
            notes,
        }
    }
}

/// Build the session reference the card prints, from the file the reading returned.
fn session_ref(
    file: &SessionFile,
    named_by: String,
    read: &SessionRead,
    at: SystemTime,
) -> SessionRef {
    SessionRef {
        id: file.id.clone(),
        path: file.path.clone(),
        size: file.size,
        age: match file.modified {
            Some(modified) => iso::format_age(modified, at),
            None => "mtime unknown".to_string(),
        },
        named_by,
        source: read.source.clone(),
    }
}

/// The reads that can name a session before the checkpoint is settled: the `--resume <id>` of an
/// in-flight process, and the session id the newest evidence transcript's own file name carries.
///
/// Both name a session by their own words, which is what every piece of checkpoint evidence has to
/// be able to do. Nothing is completed from anything else: a file name that carries no id names no
/// session, and the container's own recency is the only evidence left when no read names one.
pub fn naming_reads(cfg: &Config, probes: &Probes) -> SessionNaming {
    let mut naming = SessionNaming::default();
    if let Some(row) = agent_process(cfg, probes) {
        // An in-flight command that is already resuming a session names that session itself: the
        // `--resume <id>` it carries is the session it is writing into.
        if let Some(id) = crate::state::resume_argument(&row.command) {
            naming.names.push((
                id.clone(),
                format!(
                    "the in-flight process pid {}: its own `--resume {id}` argument",
                    row.pid
                ),
            ));
        }
    }
    if let Some(newest) = probes.evidence_files.value.first() {
        if let Some(id) = &newest.session_id {
            naming.names.push((
                id.clone(),
                format!(
                    "the newest transcript in the evidence directory, {}: its own file name carries \
                     the session id",
                    newest.path.display()
                ),
            ));
        }
    }
    naming
}

/// The agent process running in the container right now, when one is.
fn agent_process<'a>(cfg: &Config, probes: &'a Probes) -> Option<&'a crate::probe::ProcRow> {
    probes
        .container_ps
        .value
        .iter()
        .find(|row| row.is_agent(&cfg.console.claude_command) && row.command.contains("-p"))
}

/// The newest line of a transcript tail that names a checkpoint, with the checkpoint it named and
/// the words around the mention, so the card can quote the run's own sentence.
fn newest_mention(lines: &[String]) -> Option<(String, String)> {
    let mut found: Option<(String, String)> = None;
    for line in lines {
        let text = readable_line(line);
        if let Some((name, position)) = named_mention(&text) {
            found = Some((name, quote_around(&text, position)));
        }
    }
    found
}

/// The last segment of a container path: the file name, for the lines that label a read.
fn file_name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// A shorter quote, for the lines that report a transcript they did not use.
fn short_quote(text: &str) -> String {
    text.trim().chars().take(90).collect()
}

/// One checkpoint a read named, with the read that named it.
///
/// Every read that names a checkpoint is kept, whether it speaks for this run (its own process, its
/// own transcript, its own copy of it) or only corroborates from the journal's shape: two of them
/// naming two different checkpoints is a disagreement the card reports rather than resolves.
#[derive(Clone, Debug)]
struct NamedRead {
    /// The checkpoint, as the canonical name.
    checkpoint: String,
    /// A short label for the read: the evidence line carries the full one.
    by: String,
}

/// Read the checkpoint state out of the evidence the caller already collected.
///
/// The order the evidence is asked in is how directly each read speaks for this run: the in-flight
/// process's own prompt, then the session's own transcript in the container, then an
/// evidence-directory transcript attributable to that session, then the storage journal's shape.
/// Every read that named a checkpoint is kept; two of them naming two different checkpoints is
/// reported as a disagreement, and no ruling is offered rather than one being aimed at either.
pub fn detect(cfg: &Config, probes: &Probes) -> Card {
    let dir = cfg.console.evidence_dir.clone();
    let in_flight = agent_process(cfg, probes).is_some();

    // The session this card names. It is resolved first because the primary evidence is that
    // session's own transcript, and nothing in the evidence directory is attributable without it.
    let naming = naming_reads(cfg, probes);
    let session = resolve_session(cfg, probes, &naming);
    let session_id = session.named().map(|named| named.id.clone());
    let session_age = session
        .named()
        .map(|named| named.age.clone())
        .unwrap_or_else(|| "age unknown".to_string());
    let mut named: Vec<NamedRead> = Vec::new();

    // 1. The in-flight run's own prompt.
    let mut liveness: Vec<String> = Vec::new();
    if let Some(row) = agent_process(cfg, probes) {
        match crate::state::prompt_argument(&row.command) {
            Some(prompt) => match mentioned(&prompt) {
                Some(name) => {
                    let from = format!(
                        "in-flight process pid {} (`{}`, elapsed {}): its prompt names {name}",
                        row.pid, cfg.console.claude_command, row.elapsed
                    );
                    liveness.push(from);
                    named.push(NamedRead {
                        checkpoint: name,
                        by: format!("the in-flight process pid {} (its own prompt)", row.pid),
                    });
                }
                None => liveness.push(format!(
                    "in-flight process pid {} names no checkpoint in its prompt",
                    row.pid
                )),
            },
            None => liveness.push(format!(
                "in-flight process pid {} carries no `-p` prompt to read",
                row.pid
            )),
        }
    } else {
        liveness.push(format!(
            "no `{}` process inside {} at this reading ({})",
            cfg.console.claude_command,
            cfg.console.container,
            iso::age_text(probes.container_ps.at, probes.at)
        ));
    }

    // 2. The session's own transcript inside the container: the run's own words, read from its
    // tail, and the one read that settles this run's checkpoint on its own. It counts only when it
    // is the named session's own transcript: a transcript of some other session speaks for that
    // session, never for this one.
    let mut primary: Vec<String> = Vec::new();
    match &session_id {
        None => primary.push(format!(
            "the session's own transcript was not read: no read names a single session, so no \
             transcript in {} is attributable to this run and none of its words are quoted",
            cfg.console.session_dir
        )),
        Some(id) => match &probes.session_transcript.value {
            Some(read) if read.session_id != *id => primary.push(format!(
                "the transcript that was read is session {}'s, not this card's session {}: it is \
                 not attributable to this run and names no checkpoint for it",
                short_id(&read.session_id),
                short_id(id)
            )),
            Some(read) => match newest_mention(&read.lines) {
                Some((name, line)) => {
                    let from = format!(
                        "the session's own transcript {} (age {}, {} line(s) read from its tail): \
                         \"{}\" is the run's own words and names {name}",
                        read.path,
                        session_age,
                        read.lines.len(),
                        line
                    );
                    primary.push(from);
                    named.push(NamedRead {
                        checkpoint: name,
                        by: format!("the session's own transcript {}", file_name_of(&read.path)),
                    });
                }
                None => primary.push(format!(
                    "the session's own transcript {} (age {}, {} line(s) read from its tail) names \
                     no checkpoint",
                    read.path,
                    session_age,
                    read.lines.len()
                )),
            },
            None => primary.push(match &probes.session_transcript.error {
                Some(error) => format!(
                    "the session's own transcript of session {} in {} could not be read: {error}",
                    short_id(id),
                    cfg.console.session_dir
                ),
                None => format!(
                    "the session's own transcript of session {} in {} was not read",
                    short_id(id),
                    cfg.console.session_dir
                ),
            }),
        },
    }

    // 3. The evidence directory. It holds the harness's own copies, and every one of them is
    // another run's artefact until its own name says otherwise: only a transcript attributable to
    // the session this card names may name its checkpoint. Being newest is not attribution, which
    // is exactly the read that used to name another run's checkpoint on this card.
    let files = &probes.evidence_files.value;
    let with_id = files
        .iter()
        .filter(|file| file.session_id.is_some())
        .count();
    let attributable: Vec<&probe::EvidenceFile> = match &session_id {
        Some(id) => files
            .iter()
            .filter(|file| file.session_id.as_deref() == Some(id.as_str()))
            .collect(),
        None => Vec::new(),
    };
    // The newest transcript in that directory that carries a session id which is not this card's:
    // the summary names it, so the reader can see which of those files is another run's copy.
    let others: Option<String> = files
        .iter()
        .find(|file| {
            file.session_id
                .as_deref()
                .is_some_and(|id| Some(id) != session_id.as_deref())
        })
        .map(probe::EvidenceFile::name);
    let mut attributable_copies: Vec<String> = Vec::new();
    let mut directory: Vec<String> = Vec::new();
    for tail in &probes.evidence_tails.value {
        let path = tail.file.path.display().to_string();
        let age = tail
            .file
            .modified
            .map(|modified| iso::format_age(modified, probes.at))
            .unwrap_or_else(|| "mtime unknown".to_string());
        if let Some(error) = &tail.error {
            directory.push(format!(
                "the transcript {path} (age {age}) could not be read: {error}"
            ));
            continue;
        }
        let mine = tail
            .file
            .session_id
            .as_deref()
            .is_some_and(|id| session_id.as_deref() == Some(id));
        let whose = match &tail.file.session_id {
            Some(id) if mine => {
                format!("its own name carries this card's session {}", short_id(id))
            }
            Some(id) => format!(
                "its own name carries session {}, a different one",
                short_id(id)
            ),
            None => "its own name carries no session id".to_string(),
        };
        let newest = files
            .first()
            .is_some_and(|file| file.path == tail.file.path);
        match newest_mention(&tail.lines) {
            Some((name, line)) if mine => {
                let from = format!(
                    "the evidence directory's own copy of this session, {path} (age {age}, {whose}): \
                     \"{}\" names {name}",
                    short_quote(&line)
                );
                attributable_copies.push(from);
                named.push(NamedRead {
                    checkpoint: name,
                    by: format!(
                        "the evidence directory's own copy of this session, {}",
                        tail.file.name()
                    ),
                });
            }
            Some((name, line)) if newest => directory.push(format!(
                "the transcript {path} (age {age}, {whose}) names {name} (\"{}\") and is not \
                 attributable to this card's session, so it is IGNORED",
                short_quote(&line)
            )),
            Some((name, line)) => directory.push(format!(
                "another transcript there, {path} (age {age}, {whose}) names {name} (\"{}\") and is \
                 not attributable to this card's session, so it is IGNORED",
                short_quote(&line)
            )),
            None if mine => attributable_copies.push(format!(
                "the evidence directory's own copy of this session, {path} (age {age}, {whose}): its \
                 last {} lines name no checkpoint",
                tail.lines.len()
            )),
            None if newest => directory.push(format!(
                "the transcript {path} (age {age}, {whose}): its last {} lines name no checkpoint, \
                 and it is not attributable to this card's session",
                tail.lines.len()
            )),
            None => directory.push(format!(
                "another transcript there, {path} (age {age}, {whose}): its last {} lines name no \
                 checkpoint",
                tail.lines.len()
            )),
        }
    }
    // The listing itself, with the counts and, when the newest file is the one that used to win,
    // what it named and why it was not allowed to. Then the quoted words of the newest, unfiltered
    // by attribution, for the reader.
    let newest = files.first();
    let summary = match &probes.evidence_files.error {
        Some(error) => format!(
            "evidence directory {} could not be listed ({error}), so no transcript there was read, \
             none of them is attributable and none names a checkpoint",
            probes.evidence_files.source
        ),
        None if files.is_empty() => format!(
            "evidence directory {} ({}): no transcript named run* in it, so nothing there names a \
             checkpoint",
            dir.display(),
            iso::age_text(probes.evidence_files.at, probes.at)
        ),
        None => {
            let newest_note = match newest {
                Some(newest) => {
                    let named_one = probes
                        .evidence_tails
                        .value
                        .iter()
                        .find(|tail| tail.file.path == newest.path)
                        .and_then(|tail| newest_mention(&tail.lines))
                        .map(|(name, _)| name);
                    let mine = newest
                        .session_id
                        .as_deref()
                        .is_some_and(|id| session_id.as_deref() == Some(id));
                    match (&newest.session_id, named_one, mine) {
                        (_, Some(name), false) => format!(
                            ", and the newest, {}, names {name} while {}: being newest is not \
                             attribution, so it is IGNORED and does not name this run's checkpoint",
                            newest.name(),
                            match &newest.session_id {
                                Some(id) => format!(
                                    "its own name carries session {}, a different one",
                                    short_id(id)
                                ),
                                None => "its own name carries no session id".to_string(),
                            }
                        ),
                        (_, Some(name), true) => format!(
                            ", and the newest, {}, carries this card's session in its own name and \
                             names {name}",
                            newest.name()
                        ),
                        (_, None, _) => format!(
                            ", and the newest, {}, names no checkpoint in its tail",
                            newest.name()
                        ),
                    }
                }
                None => String::new(),
            };
            let session_note = match &session_id {
                Some(id) => format!(
                    "; {} of them is this card's session {}, and only a transcript whose own name \
                     carries it may name this run's checkpoint{other_note}",
                    attributable.len(),
                    short_id(id),
                    other_note = match others {
                        Some(name) => format!(
                            ", while {} carry another session's id and were ignored (the newest of \
                             them, {name})",
                            with_id - attributable.len()
                        ),
                        None => String::new(),
                    }
                ),
                None => {
                    "; no session is named by the reads, so none of them is attributable to this \
                         run and none names its checkpoint"
                        .to_string()
                }
            };
            let errors: Vec<String> = probes
                .evidence_tails
                .value
                .iter()
                .filter_map(|tail| tail.error.clone())
                .collect();
            format!(
                "evidence directory {} ({}) was listed: {} transcript(s) named run*, {} carry a \
                 session id in their own names{session_note}{newest_note}{}",
                dir.display(),
                iso::age_text(probes.evidence_files.at, probes.at),
                files.len(),
                with_id,
                match errors.first() {
                    Some(first) => format!(" (a read failed: {first})"),
                    None => String::new(),
                }
            )
        }
    };
    directory.insert(0, summary);

    // 4. The storage journal's shape. It is corroboration: it may name a checkpoint, but it never
    // outranks the run's own words, and when the two name different checkpoints the card says so.
    let mut journal: Vec<String> = Vec::new();
    let stores = &probes.storage_journal.value;
    let last_role = stores.last().map(|entry| entry.role.clone());
    let last_at = stores.last().and_then(|entry| entry.at);
    let fresh = last_at
        .map(|at| {
            probes
                .at
                .duration_since(at)
                .map(|age| age <= cfg.checkpoint_fresh())
                .unwrap_or(true)
        })
        .unwrap_or(false);
    let last_planner = stores.iter().rposition(|entry| entry.role == "planner");
    let last_implementer = stores.iter().rposition(|entry| entry.role == "implementer");
    if let Some(planner) = last_planner {
        let entry = &stores[planner];
        let after_planner = match last_implementer {
            Some(implementer) if implementer > planner => "an implementer record follows it",
            _ => "no implementer record follows it",
        };
        journal.push(format!(
            "supporting evidence (storage journal): last planner {} at {}, {}",
            entry.operation, entry.timestamp, after_planner
        ));
    }
    // The ordered sequence is planner -> CHECKPOINT 1 -> implementer and reviewer -> CHECKPOINT 2
    // -> project-manager closes. So a journal whose newest record is the planner's plan, with
    // nothing after it and nothing newer than the freshness bound, is the journal's own shape for a
    // run stopped at checkpoint 1. This is an inference from the journal and the card says so.
    let mut journal_open: Option<(String, String)> = None;
    if fresh {
        match last_role.as_deref() {
            Some("planner")
                if last_implementer.is_none_or(|index| index < last_planner.unwrap_or(0)) =>
            {
                journal_open = Some((
                    checkpoint_name(1),
                    "the newest storage record is the planner's plan and no implementer record follows it".to_string(),
                ));
            }
            Some("reviewer") => {
                let closes = stores
                    .iter()
                    .rposition(|entry| entry.role == "project-manager");
                let reviewer = stores.iter().rposition(|entry| entry.role == "reviewer");
                if closes.is_none_or(|index| Some(index) < reviewer) {
                    journal_open = Some((
                        checkpoint_name(2),
                        "the newest storage record is the reviewer's verdict and no project-manager close follows it".to_string(),
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some((name, why)) = &journal_open {
        let from = format!(
            "journal shape: {why}, and that record is inside the {} the config allows, so {name} \
             appears to be open (inferred from the journal, not from a run's prompt)",
            iso::format_age(probes.at - cfg.checkpoint_fresh(), probes.at)
        );
        journal.push(from);
        named.push(NamedRead {
            checkpoint: name.clone(),
            by: "the storage journal's shape (an inference, not a run's own words)".to_string(),
        });
    } else if let Some(role) = &last_role {
        journal.push(format!(
            "journal shape: the newest storage record is {role}'s and it is {}{}, which is not a \
             shape the ordered sequence stops at",
            iso::age_text(last_at.unwrap_or(probes.at), probes.at),
            if fresh {
                ""
            } else {
                " (older than the freshness bound)"
            }
        ));
    }

    // The disagreement, when two reads named two different checkpoints. The card shows the
    // strongest read's own words and says the reads disagree: it does not pick the one it likes.
    let which = named.first().map(|read| read.checkpoint.clone());
    let mut checkpoints: Vec<&str> = Vec::new();
    for read in &named {
        if !checkpoints.contains(&read.checkpoint.as_str()) {
            checkpoints.push(&read.checkpoint);
        }
    }
    let mut disagreement_line: Option<String> = None;
    let mut disagreement: Option<String> = None;
    if checkpoints.len() > 1 {
        let mut reads: Vec<String> = Vec::new();
        for read in &named {
            if !reads.iter().any(|seen| seen.starts_with(&read.checkpoint)) {
                reads.push(format!("{} -- {}", read.checkpoint, read.by));
            }
        }
        let listed = reads.join("; ");
        disagreement_line = Some(format!(
            "the reads disagree: {listed} -- the card shows the checkpoint the strongest read names \
             and does not choose between them"
        ));
        disagreement = Some(format!(
            "the reads disagree about which checkpoint this run stopped at: {listed}. The card does \
             not choose between them, so no ruling is offered until the disagreement is read"
        ));
    }

    // The truth table, in one place. `which` is the checkpoint the evidence names and `in_flight`
    // is whether an agent process is running in the container right now; the two are separate
    // facts, and only the pair of them decides what the card may say.
    //
    // The run is headless and one invocation per phase: at a checkpoint the orchestrator ends its
    // turn and the process exits, and the ruling is a separate invocation resuming one session. So
    // a process being alive means the run is working -- a checkpoint named while one is running is
    // ahead of it, not being waited on -- and the state the operator acts on is the evidence naming
    // a checkpoint once the process is gone.
    let state = match (&which, in_flight) {
        // A process is running: the run is working right now. Whatever the evidence names lies ahead
        // of it, and a ruling is refused, because resuming that session would start a second process
        // on the same session jsonl -- two writers on one session.
        (_, true) => CardState::Running,
        // Nothing is running and the evidence names a checkpoint: the run stopped there, and the
        // ruling the operator writes is what resumes the session. This is the actionable state.
        (Some(_), false) => CardState::Stopped,
        // Neither a run nor evidence.
        (None, false) => CardState::Idle,
    };
    let resolved = which.unwrap_or_else(|| "no checkpoint named by any evidence".to_string());
    // The session and the checkpoint, resolved together one last time with the read that named the
    // checkpoint filled in, so the card's own notes name it: the card, the confirmation screen, the
    // guards summary and the dry run all name the same session -- or refuse in the same words.
    let mut naming = naming;
    naming.checkpoint_from = named.first().map(|read| read.by.clone());
    let session = resolve_session(cfg, probes, &naming);
    // The refusal, in the order of what stops the ruling: a checkpoint the reads disagree about,
    // then -- in the one state that offers one -- a session the evidence cannot name.
    let conflict = match state {
        CardState::Stopped => disagreement.clone(),
        CardState::Running | CardState::Idle => None,
    };
    let refuse_reason = match state {
        CardState::Stopped => conflict
            .clone()
            .or_else(|| session.refusal().map(str::to_string)),
        CardState::Running | CardState::Idle => Some(actions::ruling_refusal(cfg, state)),
    };
    let can_approve = refuse_reason.is_none();
    // The command preview is the command the default canned ruling would build -- the one `Enter`
    // reaches in one keystroke -- so the card shows the exact argv that key would confirm, not a
    // placeholder. A config with no canned ruling falls back to the placeholder, as before.
    let preview_value = cfg
        .default_ruling()
        .map(|ruling| ruling.text.clone())
        .unwrap_or_else(|| "<your ruling text>".to_string());
    let command_preview = match actions::build(
        cfg,
        ActionKind::Ruling,
        &preview_value,
        Guards {
            in_flight,
            checkpoint: state,
            session: session.clone(),
            checkpoint_conflict: conflict.clone(),
            evaluated: true,
        },
    ) {
        Ok(command) => command.display(),
        Err(reason) => format!("({reason})"),
    };

    // Every line the card rests on, with its own source and age, in the order it was asked in.
    let mut evidence: Vec<String> = Vec::new();
    evidence.extend(liveness);
    evidence.extend(primary);
    if let Some(line) = &disagreement_line {
        evidence.push(line.clone());
    }
    evidence.extend(attributable_copies);
    evidence.extend(directory);
    evidence.extend(journal);
    if evidence_carries_no_checkpoint(&resolved) {
        evidence.push(format!(
            "the checkpoint is not attributable: the evidence directory {} was listed ({} \
             transcript(s) named run*, {} carrying a session id) and every transcript there that is \
             not attributable to this run was ignored rather than used to name one, so the card \
             names none",
            dir.display(),
            files.len(),
            with_id
        ));
    }
    let transcript_read = format!(
        "{} ({}): {}{}",
        probes.session_transcript.source,
        iso::age_text(probes.session_transcript.at, probes.at),
        match &session_id {
            Some(id) => format!("session {}", short_id(id)),
            None => "no session is named".to_string(),
        },
        match &probes.session_transcript.error {
            Some(error) => format!(" -- {error}"),
            None => String::new(),
        }
    );
    let evidence_dir_read = format!(
        "{} -- {} transcript(s) named run*, {} carrying a session id in their own names{}",
        dir.display(),
        files.len(),
        with_id,
        match &probes.evidence_files.error {
            Some(error) => format!(" -- {error}"),
            None => String::new(),
        }
    );
    Card {
        state,
        which: resolved.clone(),
        question: checkpoint_question(&resolved),
        evidence,
        can_approve,
        refuse_reason,
        command_preview,
        session,
        checkpoint_conflict: conflict,
        transcript_read,
        evidence_dir_read,
    }
}

/// Whether the resolved checkpoint is the card's own placeholder: nothing named one.
fn evidence_carries_no_checkpoint(resolved: &str) -> bool {
    resolved == "no checkpoint named by any evidence"
}
