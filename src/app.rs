//! The application state machine: tabs, selection, modes and the action log.
//!
//! The interaction contract is deliberately small and always the same: an action is *chosen*, then
//! its exact command is *shown*, then a confirmation key runs it. There is no path from a keypress
//! to a command that skips the confirmation screen.
//!
//! Two keys reach the ruling a checkpoint asks for, and both of them end there:
//!
//! * `Enter` on the LIVE tab, while the card offers a ruling, opens the confirmation screen for the
//!   **default** canned ruling -- one keystroke to the confirmation, and the confirmation still has
//!   to be answered, so `Enter` alone never sends anything into a session;
//! * `e` opens the **ruling chooser**, which lists each configured ruling with its exact wording and
//!   offers free text. Its numbers are the chooser's own; `1`/`2`/`3` stay the tabs everywhere else,
//!   so a run can never be halted by a keypress meant for a screen.
//!
//! Two things constrain what either key may send. A ruling must be **written for the checkpoint the
//! card names**: `Enter` sends the first configured wording that applies there, the chooser lists
//! only those, and a checkpoint no wording covers is refused rather than answered with another
//! checkpoint's text -- a plan approval sent at release approval is refused by the run itself, which
//! is how an operator was walked into sending byte-identical text twice. And a ruling **already sent
//! into a run that has not moved** is refused: the same text, into the same session, while that
//! session's transcript reads exactly as it did at that send, is the same words into a stop that did
//! not advance, and saying so is more use than sending them again.

use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::{self, ActionKind, Command, Guards};
use crate::cache;
use crate::checkpoint::{self, Card};
use crate::collector::Collector;
use crate::config::{Config, Ruling};
use crate::iso;
use crate::state::{Probes, Snapshot};

/// The three screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Flow,
    Live,
    Inspect,
}

impl Tab {
    /// The tab's full title, which the header draws.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Flow => "1 FLOW -- the map with live lights",
            Tab::Live => "2 LIVE -- what is happening right now",
            Tab::Inspect => "3 INSPECT -- the machinery",
        }
    }

    fn next(self) -> Tab {
        match self {
            Tab::Flow => Tab::Live,
            Tab::Live => Tab::Inspect,
            Tab::Inspect => Tab::Flow,
        }
    }

    fn previous(self) -> Tab {
        match self {
            Tab::Flow => Tab::Inspect,
            Tab::Live => Tab::Flow,
            Tab::Inspect => Tab::Live,
        }
    }
}

/// What the keyboard is currently doing.
#[derive(Debug)]
pub enum Mode {
    /// Moving around.
    Normal,
    /// The action menu is open.
    Menu { selection: usize },
    /// The ruling chooser is open: the canned rulings, then free text, with `selection` the row.
    Choose { selection: usize },
    /// A value is being typed for an action.
    Input { kind: ActionKind, text: String },
    /// The exact command is on screen, waiting for a confirmation key.
    Confirm { command: Box<Command> },
    /// The key reference.
    Help,
}

/// One line of the action log pane.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub at: SystemTime,
    pub text: String,
}

/// An action that is running, with its streams being drained.
pub struct Running {
    pub kind: ActionKind,
    pub label: String,
    pub channels: Vec<Receiver<String>>,
    pub child: Child,
    pub started: SystemTime,
}

/// The last ruling the console actually sent, and how the run's transcript read at that moment.
///
/// Nothing here is written down: the record lives for the session and dies with it. It exists so the
/// console can tell a ruling that *advanced* a run from the same ruling dictated into a run that has
/// not moved -- the shape that left an operator sending byte-identical text twice, because the
/// console kept offering a ruling and the run kept refusing it.
///
/// The transcript is remembered twice over on purpose. The line count is what the probe layer read
/// from the session's own transcript tail; but that read is a *capped* tail
/// (`probe::TRANSCRIPT_TAIL_LINES` lines out of a byte-limited `tail -c`), so a run that has grown
/// past the cap reads the same number of lines however much it writes. The transcript file's own
/// byte size, which the card's session read already carries, keeps growing. Either one having
/// changed is the run having moved.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SentRuling {
    /// The id of the canned ruling whose text this was; empty for the chooser's free text.
    id: String,
    /// The exact text sent, trimmed exactly as the command builder trims it.
    text: String,
    /// The session the ruling was resumed into.
    session: String,
    /// The named session's transcript tail line count, as the probe last read it.
    lines: Option<usize>,
    /// The transcript file's own byte size from that same reading.
    size: Option<u64>,
}

/// The checkpoint number a card's canonical name carries.
///
/// Read from `checkpoint::checkpoint_name` rather than scanned out of the text: "the name mentions a
/// 2" is not the question, and the placeholder a card carries when no evidence named a checkpoint
/// ("no checkpoint named by any evidence") must name no checkpoint here either. A name that is
/// exactly a canonical checkpoint name yields its number; anything else yields `None`.
fn checkpoint_index(name: &str) -> Option<u8> {
    [1u8, 2]
        .into_iter()
        .find(|index| checkpoint::checkpoint_name(*index) == name)
}

/// How a finished action ended, as one phrase: its exit code, or the signal that killed it.
///
/// The console never guesses here: a child that reported no code is *not* read as `0`, and one that
/// reported no code and no signal says so in its own words.
pub fn exit_word(status: &std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exited with code {code}");
    }
    match status.signal() {
        Some(signal) => match signal_name(signal) {
            Some(name) => format!("was killed by signal {signal} ({name})"),
            None => format!("was killed by signal {signal}"),
        },
        None => "ended with neither an exit code nor a signal".to_string(),
    }
}

/// The usual name of a signal, when it has one.
pub fn signal_name(signal: i32) -> Option<&'static str> {
    let name = match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return None,
    };
    Some(name)
}

/// The whole application.
pub struct App {
    pub config: Config,
    pub snapshot: Snapshot,
    pub tab: Tab,
    pub selection: usize,
    pub scroll: usize,
    pub mode: Mode,
    pub log: Vec<LogLine>,
    pub show_log: bool,
    pub status: String,
    pub quit: bool,
    pub running: Vec<Running>,
    pub last_refresh: SystemTime,
    /// The worker that takes every snapshot, when the console runs interactively.
    ///
    /// `None` in `--dump`, in the action dry runs and in the tests, where the reading is taken inline
    /// and there is no UI thread to protect.
    pub collector: Option<Collector>,
    /// How many collects the worker has been asked for, and how many have come back. A read is in
    /// flight exactly when the two differ: the screen then draws the last snapshot it was given and
    /// says a read is in flight, rather than waiting for the new one.
    collects_asked: u64,
    collects_answered: u64,
    /// Whether the last ask that reached the worker was a forced one. `r` sets it; the console's own
    /// cadence does not, which is what lets each probe keep to its own TTL.
    last_request_forced: bool,
    /// How often the console asks the worker for a snapshot, from the config's cadence table.
    pub refresh_interval: Duration,
    /// The age at which the snapshot on the screen is labelled STALE, from the same table.
    pub stale_after: Duration,
    /// The last ruling sent, with the run's own state at that send: memory only, as the guard it
    /// serves is about this session and not about anything a file should remember.
    last_ruling_sent: Option<SentRuling>,
}

impl App {
    /// Read the system once, then sit in the normal mode.
    ///
    /// One synchronous reading, taken before the first frame: the non-interactive paths and the tests
    /// want a full snapshot in hand. The interactive session starts through `new_async` instead, so
    /// that its terminal is never held while a probe runs.
    pub fn new(config: Config) -> App {
        let snapshot = Snapshot::collect(&config);
        let last_refresh = snapshot.read_at;
        let mut app = App::bare(config, snapshot, last_refresh);
        app.note(format!(
            "startup: read the state of {} ({}). Nothing was written, restarted or killed.",
            app.config.repo.display(),
            iso::format_utc(last_refresh)
        ));
        app
    }

    /// The app an interactive session starts in: nothing read yet, and the first snapshot already
    /// asked for on the worker.
    ///
    /// The first frames are drawn from the honest `not probed` reading and say a read is in flight.
    /// That is the point: the console used to spend the first two seconds of every refresh inside the
    /// gate server's probe, with the terminal held.
    pub fn new_async(config: Config) -> App {
        let collector = Collector::start(config.clone());
        App::new_async_with(config, collector)
    }

    /// The same start, with the worker the caller started.
    ///
    /// `Collector::start_delayed` is a test hook; nothing else needs this, but it is the honest seam
    /// for a test that wants a known, long collect in flight while it drives the loop.
    pub fn new_async_with(config: Config, collector: Collector) -> App {
        let at = SystemTime::now();
        let snapshot = Snapshot::from_parts(config.clone(), Probes::empty(&config, at));
        let mut app = App::bare(config, snapshot, at);
        app.status = "reading the system now; press ? for the key reference".to_string();
        app.note(format!(
            "startup: asked the probe worker for the state of {} ({}). Nothing was written, restarted \
             or killed.",
            app.config.repo.display(),
            iso::format_utc(at)
        ));
        app.attach_collector(collector);
        app
    }

    /// The shared construction: the state, and the two cadences the config carries.
    fn bare(config: Config, snapshot: Snapshot, last_refresh: SystemTime) -> App {
        let refresh_interval = cache::refresh_interval(&config);
        let stale_after = cache::stale_after(&config);
        App {
            config,
            snapshot,
            tab: Tab::Flow,
            selection: 0,
            scroll: 0,
            mode: Mode::Normal,
            log: Vec::new(),
            show_log: true,
            status: "read the system once; press ? for the key reference".to_string(),
            quit: false,
            running: Vec::new(),
            last_refresh,
            collector: None,
            collects_asked: 0,
            collects_answered: 0,
            last_request_forced: false,
            refresh_interval,
            stale_after,
            last_ruling_sent: None,
        }
    }

    /// Attach the worker and ask it for the first snapshot.
    pub fn attach_collector(&mut self, collector: Collector) {
        self.collector = Some(collector);
        self.request_collect(true);
    }

    /// Ask the worker for a snapshot, and say whether the request reached it.
    fn request_collect(&mut self, force: bool) -> bool {
        let Some(collector) = &self.collector else {
            return false;
        };
        if collector.request(force) {
            self.collects_asked += 1;
            self.last_request_forced = force;
            return true;
        }
        self.note("the probe worker is not answering: no new reading was requested");
        self.status = "the probe worker is not answering".to_string();
        false
    }

    /// Take whatever the worker has finished, newest last, and draw it.
    ///
    /// This is the only way the interactive console's snapshot changes. It never blocks: a collect
    /// that has not finished is simply not here yet, and the screen keeps drawing the last one.
    pub fn drain_collector(&mut self) -> usize {
        // The collector is taken out for the drain so that the snapshots can be applied to the app
        // while it is in hand; it goes straight back, and nothing else can observe it missing.
        let Some(collector) = self.collector.take() else {
            return 0;
        };
        let mut arrived: Vec<Snapshot> = Vec::new();
        while let Some(snapshot) = collector.try_recv() {
            arrived.push(snapshot);
        }
        self.collector = Some(collector);
        let taken = arrived.len();
        for snapshot in arrived {
            self.collects_answered += 1;
            self.apply_snapshot(snapshot);
        }
        taken
    }

    /// Make a snapshot the current reading.
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.last_refresh = snapshot.read_at;
        self.snapshot = snapshot;
        if self.selection >= self.snapshot.flow.len() {
            self.selection = self.snapshot.flow.len().saturating_sub(1);
        }
    }

    /// Whether a read is in flight right now: asked for, and not yet handed back.
    pub fn collect_in_flight(&self) -> bool {
        self.collector.is_some() && self.collects_asked > self.collects_answered
    }

    /// How many snapshots the worker has handed back this session.
    pub fn collects_answered(&self) -> u64 {
        self.collects_answered
    }

    /// Whether the last ask that reached the worker was forced, so no probe could be served from its
    /// cache. `r` sets it; the console's own cadence never does.
    pub fn last_request_forced(&self) -> bool {
        self.last_request_forced
    }

    /// The age of the snapshot on the screen.
    pub fn reading_age(&self) -> Duration {
        cache::age_of(self.last_refresh, SystemTime::now())
    }

    /// Whether the snapshot on the screen is older than the config's stale-after threshold.
    pub fn is_stale(&self) -> bool {
        self.reading_age() >= self.stale_after
    }

    /// Append a line to the action log, timestamped.
    pub fn note(&mut self, text: impl Into<String>) {
        let at = SystemTime::now();
        self.log.push(LogLine {
            at,
            text: text.into(),
        });
        if self.log.len() > 2000 {
            self.log.drain(..self.log.len() - 2000);
        }
    }

    /// Re-read every reading.
    ///
    /// With a worker attached the request goes to it and the key returns at once: the answer arrives
    /// over the channel and the screen keeps drawing until it does. The request is forced, so no
    /// reading is served from its cache -- the operator asked for a reading of now.
    pub fn refresh(&mut self) {
        if self.collector.is_some() {
            if self.request_collect(true) {
                self.note(
                    "refresh asked for: the worker is re-reading every probe now (forced -- nothing \
                     is served from its cache)",
                );
                self.status =
                    "re-reading every probe now; the screen keeps drawing the last reading until it \
                     arrives"
                        .to_string();
            }
            return;
        }
        self.reread(true);
    }

    /// Re-read the probe layer when the current reading has gone stale, with no keypress at all.
    ///
    /// With a worker attached this asks it for a snapshot and returns; the snapshot is *not* forced,
    /// so each probe is re-read on its own cadence and the expensive one only when its TTL has
    /// elapsed. Without a worker it is the same `Snapshot::collect` the `r` key calls -- the card,
    /// the run line and every other reading are re-read on a timer, so they reflect reality without
    /// the operator pressing anything. Each reading still carries its source and its age, and a read
    /// that fails is still a failed reading, so nothing is invented by being refreshed.
    ///
    /// The log line `r` writes is deliberately left out here: a note every few seconds would push
    /// the action log's own lines -- a started action, an exit status -- out of the pane.
    pub fn auto_refresh(&mut self, interval: Duration) -> bool {
        let stale = self
            .last_refresh
            .elapsed()
            .map(|elapsed| elapsed >= interval)
            .unwrap_or(false);
        if !stale {
            return false;
        }
        if self.collector.is_some() {
            // One request at a time: a collect still in flight will answer with a reading at least as
            // new as this ask, so piling more on would only queue work behind a slow probe.
            if self.collect_in_flight() {
                return false;
            }
            return self.request_collect(false);
        }
        self.reread(false);
        true
    }

    /// The one re-read path without a worker: drain the children, then collect a fresh snapshot.
    fn reread(&mut self, announced: bool) {
        let ended = self.drain_running();
        self.snapshot = Snapshot::collect(&self.config);
        self.last_refresh = self.snapshot.read_at;
        if self.selection >= self.snapshot.flow.len() {
            self.selection = self.snapshot.flow.len().saturating_sub(1);
        }
        if announced {
            self.note(format!(
                "refreshed: {} readings taken at {}",
                self.snapshot.flow.len(),
                iso::format_utc(self.last_refresh)
            ));
        }
        if !ended.is_empty() {
            self.status = self.exit_status_line(&ended);
        }
    }

    /// The guards the action builder consults.
    pub fn guards(&self) -> Guards {
        Guards {
            in_flight: self.snapshot.live.run.in_flight,
            checkpoint: self.snapshot.live.checkpoint.state,
            session: self.snapshot.live.checkpoint.session.clone(),
            checkpoint_conflict: self.snapshot.live.checkpoint.checkpoint_conflict.clone(),
            evaluated: true,
        }
    }

    /// The checkpoint card, however the current snapshot reads it.
    pub fn card(&self) -> &Card {
        &self.snapshot.live.checkpoint
    }

    /// Drain every running action's streams and re-read every child.
    ///
    /// `try_wait` is what notices an exit: an action whose child has ended is taken out of
    /// `running`, so it stops being reported as running anywhere, and how it ended is returned so
    /// the caller can put the exit code (or the signal) in the log and in the status bar.
    fn drain_running(&mut self) -> Vec<(String, String)> {
        let mut actions = std::mem::take(&mut self.running);
        let mut ended: Vec<(String, String)> = Vec::new();
        let mut finished: Vec<usize> = Vec::new();
        for (index, action) in actions.iter_mut().enumerate() {
            let mut lines = Vec::new();
            for channel in &action.channels {
                while let Ok(line) = channel.try_recv() {
                    lines.push(line);
                }
            }
            for line in lines {
                self.note(line);
            }
            match action.child.try_wait() {
                Ok(Some(status)) => {
                    let label = action.label.clone();
                    let word = exit_word(&status);
                    self.note(format!("-- {label} {word}"));
                    ended.push((action.kind.id().to_string(), word));
                    finished.push(index);
                }
                Ok(None) => {}
                Err(error) => {
                    let label = action.label.clone();
                    let word = format!("could not be waited on: {error}");
                    self.note(format!("-- {label} {word}"));
                    ended.push((action.kind.id().to_string(), word));
                    finished.push(index);
                }
            }
        }
        for index in finished.into_iter().rev() {
            actions.remove(index);
        }
        self.running = actions;
        ended
    }

    /// The status line for the actions that have just ended: which one, how it ended, and whether
    /// anything is still running.
    fn exit_status_line(&self, ended: &[(String, String)]) -> String {
        let text = ended
            .iter()
            .map(|(id, word)| format!("{id} {word}"))
            .collect::<Vec<String>>()
            .join("; ");
        if self.running.is_empty() {
            format!("{text} -- no action is running now")
        } else {
            format!(
                "{text} -- {} action(s) are still running",
                self.running.len()
            )
        }
    }

    /// Re-read every child on the normal tick, so an action that has ended stops being reported as
    /// running the moment it does, and its exit status is in the log and the status bar.
    pub fn poll_running(&mut self) {
        let ended = self.drain_running();
        if ended.is_empty() {
            return;
        }
        self.status = self.exit_status_line(&ended);
    }

    /// The current mode's name, as the status bar and the tests print it.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Normal => "normal",
            Mode::Menu { .. } => "menu",
            Mode::Choose { .. } => "choose",
            Mode::Input { .. } => "input",
            Mode::Confirm { .. } => "confirm",
            Mode::Help => "help",
        }
    }

    /// The canned rulings written for the checkpoint the card names, in config order.
    ///
    /// The chooser offers exactly these: a wording scoped to another checkpoint is not a candidate at
    /// this one, and offering it would put the operator one keystroke from the text the run refuses.
    /// An unscoped ruling (`checkpoints: []`) applies everywhere, and is what keeps `halt` reachable
    /// from both checkpoints.
    ///
    /// When the card names no checkpoint this console recognises, *every* configured wording is
    /// applicable: there is no checkpoint to scope by, so nothing may be pruned, and the first of them
    /// is the default `Enter` sends -- which is what this console did before wording was scoped at all.
    /// Scoping is never widened from a name it could not read for any other reason: a card that names
    /// checkpoint 2 for real still gets checkpoint 2's wording and nothing else.
    pub fn applicable_rulings(&self) -> Vec<&Ruling> {
        let rules = self.config.rulings();
        match checkpoint_index(&self.card().which) {
            Some(index) => rules
                .iter()
                .filter(|ruling| ruling.applies_to(index))
                .collect(),
            None => rules.iter().collect(),
        }
    }

    /// The rule the rulings key list obeys: how many rows the chooser draws -- the applicable canned
    /// rulings, then one free-text row, which is always last so its number stays the next one.
    pub fn chooser_rows(&self) -> usize {
        self.applicable_rulings().len() + 1
    }

    /// Whether the card offers a ruling right now, exactly as the action builder will read it.
    ///
    /// The same question `e`, `Enter` and `actions::build` ask, asked once, so a key cannot announce
    /// an action the builder then refuses.
    pub fn ruling_offerable(&self) -> bool {
        self.guards().ruling_offerable()
    }

    /// Whether any action is running right now.
    pub fn actions_running(&self) -> usize {
        self.running.len()
    }

    /// Handle one key press.
    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.on_normal(key),
            Mode::Menu { selection } => self.on_menu(key, selection),
            Mode::Choose { selection } => self.on_choose(key, selection),
            Mode::Input { kind, text } => self.on_input(key, kind, text),
            Mode::Confirm { command } => self.on_confirm(key, *command),
            Mode::Help => {
                if key.code != KeyCode::Char('?') {
                    self.status = "closed the key reference".to_string();
                } else {
                    self.mode = Mode::Help;
                }
            }
        }
    }

    fn on_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Tab => {
                self.tab = self.tab.next();
                self.scroll = 0;
            }
            KeyCode::BackTab => {
                self.tab = self.tab.previous();
                self.scroll = 0;
            }
            KeyCode::Char('1') => self.tab = Tab::Flow,
            KeyCode::Char('2') => self.tab = Tab::Live,
            KeyCode::Char('3') => self.tab = Tab::Inspect,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('L') => self.show_log = !self.show_log,
            KeyCode::Char('a') => {
                self.mode = Mode::Menu { selection: 0 };
                self.status = "action menu: j/k to move, Enter to choose, Esc to close".to_string();
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(8),
            KeyCode::PageUp => self.move_selection(-8),
            KeyCode::Char('g') => {
                self.selection = 0;
                self.scroll = 0;
            }
            KeyCode::Char('e') => self.begin_ruling(),
            KeyCode::Enter => self.approve_with_default_ruling(),
            KeyCode::Char('t') => self.begin_input(ActionKind::Brief, String::new()),
            KeyCode::Esc => {
                if self.status.is_empty() {
                    self.mode = Mode::Help;
                } else {
                    self.status.clear();
                }
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.tab == Tab::Inspect {
            let next = self.scroll as i32 + delta;
            self.scroll = next.max(0) as usize;
            return;
        }
        let count = self.snapshot.flow.len() as i32;
        if count == 0 {
            return;
        }
        let next = (self.selection as i32 + delta).clamp(0, count - 1);
        self.selection = next as usize;
    }

    fn on_menu(&mut self, key: KeyEvent, selection: usize) {
        let kinds = actions::all_kinds();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.status = "closed the action menu; nothing was run".to_string();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = Mode::Menu {
                    selection: (selection + 1) % kinds.len(),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = Mode::Menu {
                    selection: (selection + kinds.len() - 1) % kinds.len(),
                };
            }
            KeyCode::Enter => {
                let kind = kinds[selection];
                if kind.needs_value() {
                    let prefill = if kind == ActionKind::Gate {
                        self.snapshot
                            .live
                            .gate_names
                            .first()
                            .cloned()
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    self.begin_input(kind, prefill);
                } else {
                    self.propose(kind, String::new());
                }
            }
            _ => self.mode = Mode::Menu { selection },
        }
    }

    fn begin_input(&mut self, kind: ActionKind, prefill: String) {
        self.status = format!("{} -- {}", kind.title(), kind.value_hint());
        self.mode = Mode::Input {
            kind,
            text: prefill,
        };
    }

    /// `e`: the ruling chooser.
    ///
    /// The chooser lists the canned rulings **written for the checkpoint the card names**, each drawn
    /// with its exact wording before it is chosen, and only choosing one reaches the confirmation
    /// screen -- this key sends nothing. A card that offers no ruling is refused here in the card's
    /// own words, exactly as before. With no applicable canned ruling the free-text box opens
    /// directly, which is what `e` always was.
    fn begin_ruling(&mut self) {
        let card = self.snapshot.live.checkpoint.clone();
        if !card.can_approve {
            let reason = card
                .refuse_reason
                .clone()
                .unwrap_or_else(|| "no checkpoint is open".to_string());
            self.status = reason.clone();
            self.note(format!("ruling refused: {reason}"));
            return;
        }
        let applicable = self.applicable_rulings().len();
        if applicable == 0 {
            self.begin_free_ruling();
            return;
        }
        self.status = format!(
            "ruling chooser for {}: 1-{} pick a canned ruling, c free text, Esc closes (nothing is \
             sent until the confirmation screen is answered)",
            card.which, applicable
        );
        self.mode = Mode::Choose { selection: 0 };
    }

    /// `Enter` on the LIVE tab: the confirmation screen for the ruling written for this checkpoint.
    ///
    /// One keystroke reaches the confirmation -- and stops there. The modal shows the exact command
    /// and the text about to be sent, and only `y` starts anything, so this key cannot send a ruling
    /// by itself. Off the LIVE tab it changes nothing.
    ///
    /// Which ruling that is comes from the card: the first configured wording written for the
    /// checkpoint the card names. When the card names a checkpoint no canned wording covers, the key
    /// **refuses** rather than sending a wording meant for the other checkpoint -- a plan approval at
    /// release approval is refused by the run anyway, and being handed the same default again is what
    /// walked the operator into sending byte-identical text twice. A card that names no checkpoint
    /// this console recognises (the idle and contested states) keeps the old path: the first
    /// configured ruling, which the action builder then refuses in its own words.
    fn approve_with_default_ruling(&mut self) {
        if self.tab != Tab::Live {
            return;
        }
        let card = self.snapshot.live.checkpoint.clone();
        let checkpoint = checkpoint_index(&card.which);
        let Some(ruling) = self.config.default_ruling_for(checkpoint).cloned() else {
            let reason = match checkpoint {
                Some(_) => format!(
                    "no canned ruling in console.rulings is written for {}; press e to choose or \
                     write one",
                    card.which
                ),
                None => "no canned ruling is configured in console.rulings; press e to choose or \
                         write one"
                    .to_string(),
            };
            self.note(format!("ruling refused: {reason}"));
            self.status = reason;
            return;
        };
        self.note(format!(
            "Enter: the default canned ruling '{}' ({})",
            ruling.id,
            if ruling.prefill {
                "opens the box prefilled"
            } else {
                "sent as written"
            }
        ));
        // The same call the action menu's ruling entry makes, so the same guards are consulted and
        // the same refusal is printed when the checkpoint is contended or a run is in flight.
        self.propose(ActionKind::Ruling, ruling.text);
    }

    /// One key inside the chooser.
    ///
    /// The digits are the chooser's own and mean the ruling they are numbered as -- the tabs keep
    /// their `1`/`2`/`3` everywhere else, which is why a ruling is never one keystroke away from a
    /// screen the operator meant to open.
    fn on_choose(&mut self, key: KeyEvent, selection: usize) {
        let rulings = self.applicable_rulings().len();
        let rows = self.chooser_rows();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.status = "closed the ruling chooser; nothing was run".to_string();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = Mode::Choose {
                    selection: (selection + 1) % rows,
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = Mode::Choose {
                    selection: (selection + rows - 1) % rows,
                };
            }
            KeyCode::Enter => self.pick_ruling(selection),
            KeyCode::Char('c') => self.begin_free_ruling(),
            KeyCode::Char(digit) if digit.is_ascii_digit() => {
                let index = digit.to_digit(10).unwrap_or(0) as usize;
                if index >= 1 && index <= rulings {
                    self.pick_ruling(index - 1);
                } else {
                    self.mode = Mode::Choose { selection };
                }
            }
            _ => self.mode = Mode::Choose { selection },
        }
    }

    /// Take one canned ruling, by its row in the chooser.
    ///
    /// The row is a position in the **applicable** list the chooser drew, not in `console.rulings`:
    /// a wording scoped to another checkpoint is not a row here at all, so the numbers stay
    /// contiguous over the rulings the operator can actually send. `prefill` decides where it lands:
    /// `true` puts the configured wording in the text box for amendment, `false` takes it as written
    /// and goes straight to the confirmation screen. Both end on the confirmation screen, and neither
    /// starts anything.
    fn pick_ruling(&mut self, index: usize) {
        let applicable: Vec<Ruling> = self.applicable_rulings().into_iter().cloned().collect();
        let Some(ruling) = applicable.get(index).cloned() else {
            let which = self.card().which.clone();
            self.status = format!("no canned ruling {} applies to {which}", index + 1);
            self.mode = Mode::Normal;
            return;
        };
        if ruling.prefill {
            self.status = format!(
                "{} -- the configured wording is in the box; amend it, then Enter to review the \
                 exact command",
                ruling.label
            );
            self.mode = Mode::Input {
                kind: ActionKind::Ruling,
                text: ruling.text,
            };
        } else {
            self.note(format!(
                "canned ruling '{}' ({}) is sent as written",
                ruling.id, ruling.label
            ));
            self.propose(ActionKind::Ruling, ruling.text);
        }
    }

    /// The free-text row: the empty box `e` used to open, unchanged.
    fn begin_free_ruling(&mut self) {
        let card = self.snapshot.live.checkpoint.clone();
        if !card.can_approve {
            let reason = card
                .refuse_reason
                .clone()
                .unwrap_or_else(|| "no checkpoint is open".to_string());
            self.status = reason.clone();
            self.note(format!("ruling refused: {reason}"));
            return;
        }
        self.status = format!(
            "ruling for {}: type it, then Enter to see the command",
            card.which
        );
        self.mode = Mode::Input {
            kind: ActionKind::Ruling,
            text: String::new(),
        };
    }

    fn on_input(&mut self, key: KeyEvent, kind: ActionKind, mut text: String) {
        match key.code {
            KeyCode::Esc => {
                self.status = "cancelled; nothing was run".to_string();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.propose(kind, text),
            KeyCode::Backspace => {
                text.pop();
                self.mode = Mode::Input { kind, text };
            }
            KeyCode::Char(c) => {
                text.push(c);
                self.mode = Mode::Input { kind, text };
            }
            _ => self.mode = Mode::Input { kind, text },
        }
    }

    /// Build the command and put its exact text on screen for confirmation.
    ///
    /// One send is refused before it is even built: a ruling whose exact text was already sent into
    /// this session while that session's transcript has not moved since. The refusal is here rather
    /// than on the confirmation key, so the operator never reaches a confirmation screen for words
    /// that cannot advance the run.
    fn propose(&mut self, kind: ActionKind, value: String) {
        if kind == ActionKind::Ruling {
            if let Some(reason) = self.replay_refusal(&value) {
                self.note(format!("ruling refused: {reason}"));
                self.status = format!("refused: {reason}");
                self.mode = Mode::Normal;
                return;
            }
        }
        match actions::build(&self.config, kind, &value, self.guards()) {
            Ok(command) => {
                if let Some(path) = command.writes.first().map(|(path, _)| path.clone()) {
                    self.note(format!("staged: {}", path.display()));
                }
                self.status =
                    "review the exact command, then press y (or Enter) to run it, n to cancel"
                        .to_string();
                self.mode = Mode::Confirm {
                    command: Box::new(command),
                };
            }
            Err(reason) => {
                self.note(format!("{} refused: {reason}", kind.id()));
                self.status = format!("refused: {reason}");
                self.mode = Mode::Normal;
            }
        }
    }

    /// Why this ruling must not be sent again, when it must not.
    ///
    /// The rule is the run's own diagnosis: two byte-identical messages after a run reached its stop
    /// condition means the relay is replaying rather than advancing. So the test is on the run, not on
    /// the console's intent -- the same text, into the same session, while that session's transcript
    /// reads exactly as it did at the last send. If the transcript has grown the run moved, and the
    /// ruling is allowed again; a different session or different words are somebody else's decision
    /// and are not this guard's business.
    fn replay_refusal(&self, text: &str) -> Option<String> {
        let sent = self.last_ruling_sent.as_ref()?;
        if sent.text != text.trim() {
            return None;
        }
        let session = self.guards().session.named().cloned()?;
        if sent.session != session.id {
            return None;
        }
        let lines = self.snapshot.live.session_transcript_lines;
        if lines != sent.lines || Some(session.size) != sent.size {
            return None;
        }
        let ruling = if sent.id.is_empty() {
            "the identical ruling text".to_string()
        } else {
            format!("the identical ruling '{}'", sent.id)
        };
        let reads = match (sent.lines, sent.size) {
            (Some(count), _) => format!("still reads {count} line(s) from its tail"),
            (None, Some(bytes)) => format!("still reads at {bytes} bytes"),
            (None, None) => "still reads no further than it did at that send".to_string(),
        };
        Some(format!(
            "{ruling} was already sent into session {}, and the run's stop is unchanged: its \
             transcript {reads}, exactly as at that send. The same words cannot advance a run that \
             has not moved -- press e to choose a different ruling, or type a different one.",
            checkpoint::short_id(&sent.session)
        ))
    }

    /// Remember a ruling the operator has just confirmed, with the run's own state at that instant.
    ///
    /// Recorded at the confirmation rather than at the child's exit: what the guard is about is the
    /// console having sent those words into that session, and a run that stops on them does so after
    /// this point, which is exactly the state the next send is measured against.
    fn remember_sent_ruling(&mut self, command: &Command) {
        let Some(session) = command.guards.session.named() else {
            return;
        };
        let id = self
            .config
            .rulings()
            .iter()
            .find(|ruling| ruling.text == command.value)
            .map(|ruling| ruling.id.clone())
            .unwrap_or_default();
        self.last_ruling_sent = Some(SentRuling {
            id,
            text: command.value.clone(),
            session: session.id.clone(),
            lines: self.snapshot.live.session_transcript_lines,
            size: Some(session.size),
        });
    }

    fn on_confirm(&mut self, key: KeyEvent, command: Command) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let display = command.display();
                for line in &command.note {
                    self.note(format!("note: {line}"));
                }
                self.note(format!("run: {display}"));
                if command.kind == ActionKind::Ruling {
                    self.remember_sent_ruling(&command);
                }
                match actions::start(&command) {
                    Ok(started) => {
                        self.note(format!(
                            "-- {} started (pid {})",
                            command.kind.id(),
                            started.child.id()
                        ));
                        self.running.push(Running {
                            kind: started.kind,
                            label: started.label,
                            channels: started.channels,
                            child: started.child,
                            started: SystemTime::now(),
                        });
                        self.status =
                            format!("{} is running; its output streams below", command.kind.id());
                    }
                    Err(error) => {
                        self.note(format!("could not start: {error}"));
                        self.status = format!("could not start: {error}");
                    }
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.note(format!("cancelled: {}", command.display()));
                self.status = "cancelled; nothing was run".to_string();
                self.mode = Mode::Normal;
            }
            _ => {
                self.mode = Mode::Confirm {
                    command: Box::new(command),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkpoint_number_is_read_from_the_canonical_name_only() {
        assert_eq!(
            checkpoint_index("HUMAN CHECKPOINT 1 (plan approval)"),
            Some(1)
        );
        assert_eq!(
            checkpoint_index("HUMAN CHECKPOINT 2 (release approval)"),
            Some(2)
        );
        // The card's own placeholder names no checkpoint, and neither does any other text: the name is
        // matched against `checkpoint::checkpoint_name`, not scanned for a digit, so neither the words
        // "no checkpoint named by any evidence" nor a bare "HUMAN CHECKPOINT 2" resolves to a number.
        assert_eq!(
            checkpoint_index("no checkpoint named by any evidence"),
            None
        );
        assert_eq!(checkpoint_index("HUMAN CHECKPOINT 2"), None);
        assert_eq!(checkpoint_index("checkpoint 2 (release approval)"), None);
        assert_eq!(checkpoint_index(""), None);
    }
}
