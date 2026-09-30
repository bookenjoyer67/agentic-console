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

use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::{self, ActionKind, Command, Guards};
use crate::cache;
use crate::checkpoint::Card;
use crate::collector::Collector;
use crate::config::Config;
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

    /// The rule the rulings key list obeys: how many rows the chooser draws, the canned rulings plus
    /// one free-text row. Zero when nothing is configured, which is also when `e` skips the chooser.
    pub fn chooser_rows(&self) -> usize {
        self.config.rulings().len() + 1
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
    /// The canned rulings come from `console.rulings`, each drawn with its exact wording before it
    /// is chosen, and only choosing one reaches the confirmation screen -- this key sends nothing.
    /// A card that offers no ruling is refused here in the card's own words, exactly as before. A
    /// config with no canned ruling falls back to the free-text box, which is what `e` always was.
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
        if self.config.rulings().is_empty() {
            self.begin_free_ruling();
            return;
        }
        self.status = format!(
            "ruling chooser for {}: 1-{} pick a canned ruling, c free text, Esc closes (nothing is \
             sent until the confirmation screen is answered)",
            card.which,
            self.config.rulings().len()
        );
        self.mode = Mode::Choose { selection: 0 };
    }

    /// `Enter` on the LIVE tab: the confirmation screen for the default canned ruling.
    ///
    /// One keystroke reaches the confirmation -- and stops there. The modal shows the exact command
    /// and the text about to be sent, and only `y` starts anything, so this key cannot send a ruling
    /// by itself. Off the LIVE tab, or with no ruling offered, it changes nothing: the refusal is
    /// the action builder's own words, so the operator reads the same sentence here as in `--dump`.
    fn approve_with_default_ruling(&mut self) {
        if self.tab != Tab::Live {
            return;
        }
        let Some(ruling) = self.config.default_ruling().cloned() else {
            self.status =
                "no canned ruling is configured in console.rulings; press c in the chooser to write \
                 one"
                    .to_string();
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
        let rulings = self.config.rulings().len();
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

    /// Take one canned ruling, by its position in `console.rulings`.
    ///
    /// `prefill` decides where it lands: `true` puts the configured wording in the text box for
    /// amendment, `false` takes it as written and goes straight to the confirmation screen. Both
    /// end on the confirmation screen, and neither starts anything.
    fn pick_ruling(&mut self, index: usize) {
        let Some(ruling) = self.config.rulings().get(index).cloned() else {
            self.status = format!("no canned ruling {} is configured", index + 1);
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
    fn propose(&mut self, kind: ActionKind, value: String) {
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

    fn on_confirm(&mut self, key: KeyEvent, command: Command) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let display = command.display();
                for line in &command.note {
                    self.note(format!("note: {line}"));
                }
                self.note(format!("run: {display}"));
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
