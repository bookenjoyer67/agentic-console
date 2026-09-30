//! The terminal shell: the header, the three screens, the action log pane, the status bar and the
//! modal states (action menu, value input, command confirmation, key reference).

pub mod flow;
pub mod inspect;
pub mod live;
pub mod style;

use std::io::stdout;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::actions;
use crate::app::{App, Mode, Tab};
use crate::config::Config;
use crate::iso;

/// The key reference, drawn by the help modal and printed by `--help`.
pub const KEY_REFERENCE: &[(&str, &str)] = &[
    ("q / Ctrl-C", "quit"),
    (
        "1 / 2 / 3, Tab, Shift-Tab",
        "the FLOW, LIVE and INSPECT screens",
    ),
    (
        "j / k, Up / Down",
        "move the selected box (or scroll INSPECT)",
    ),
    ("PgUp / PgDn", "jump 8 boxes"),
    ("g", "back to the first box"),
    (
        "r",
        "re-read every reading now, forcing every probe (the console also re-reads on its own, on \
         console_probe_cadence.refresh_seconds; a probe is not re-run inside its own TTL)",
    ),
    (
        "a",
        "the action menu: choose an action, see its exact command, confirm",
    ),
    (
        "Enter",
        "on LIVE, at a checkpoint the card offers: the confirmation screen for the default canned \
         ruling (`console.rulings[0]`). One keystroke to the confirmation, and nothing is sent until \
         its own key is pressed",
    ),
    (
        "e",
        "the ruling chooser: the numbered canned rulings with their exact wording, or c for free \
         text (refused while a run is in flight, or when the evidence cannot name one session)",
    ),
    (
        "chooser",
        "1-9 or Enter pick a canned ruling, c the empty box, j/k move, Esc closes",
    ),
    ("t", "start a brief (same as the action menu's brief entry)"),
    ("L", "show or hide the action log pane"),
    ("? / Esc", "this key reference"),
    (
        "Enter then y",
        "run the command on the confirmation screen; n cancels",
    ),
];

/// How long one turn of the loop waits for a key before drawing again.
///
/// This is the console's whole latency budget: input is read, handled and answered within one tick,
/// whatever a probe is doing, because the probes are not on this thread at all. It is also how
/// promptly a snapshot the worker has finished reaches the screen.
const TICK: Duration = Duration::from_millis(50);

/// Run the interactive TUI until the operator quits.
pub fn run(config: Config) -> Result<(), String> {
    enable_raw_mode().map_err(|error| format!("raw mode: {error}"))?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).map_err(|error| format!("alternate screen: {error}"))?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(|error| format!("terminal: {error}"))?;
    // The first snapshot is asked for on the worker, not taken here: the terminal is never held while
    // a probe runs, and the first frame says which readings are still in flight.
    let mut app = App::new_async(config);
    let result = event_loop(&mut terminal, &mut app);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<(), String> {
    while !app.quit {
        step(terminal, app, TICK, &mut keyboard)?;
    }
    Ok(())
}

/// One key from the terminal, or `None` when the tick expires with no key pressed.
fn keyboard(tick: Duration) -> Result<Option<KeyEvent>, String> {
    if !event::poll(tick).map_err(|error| format!("poll: {error}"))? {
        return Ok(None);
    }
    match event::read().map_err(|error| format!("read: {error}"))? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(key)),
        Event::Resize(_, _) => Ok(None),
        _ => Ok(None),
    }
}

/// One turn of the loop, with the input side injected so a test can drive it.
///
/// The order is the whole design:
///
/// 1. take whatever snapshot the worker has finished -- never wait for one;
/// 2. re-read the running actions' own children, which is a `try_wait` and not a probe;
/// 3. draw the frame belonging to the reading in hand;
/// 4. wait at most one tick for a key, and handle it immediately;
/// 5. ask for a new snapshot when the reading has gone stale.
///
/// Step 4 is the only place this function waits, and its bound is `tick`. Nothing here reads a file
/// or runs a command: the expensive reads belong to the worker thread. A test passes its own
/// `next_key` and so drives the real loop body, with a deliberately slow probe in flight.
pub fn step<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    tick: Duration,
    next_key: &mut dyn FnMut(Duration) -> Result<Option<KeyEvent>, String>,
) -> Result<(), String> {
    app.drain_collector();
    app.poll_running();
    terminal
        .draw(|frame| draw(frame, app))
        .map_err(|error| format!("draw: {error}"))?;
    if let Some(key) = next_key(tick)? {
        app.on_key(key);
    }
    app.poll_running();
    // The card and the run line must not need a keypress: ask for a fresh reading on the config's
    // cadence. The ask is a channel send -- the collect happens on the worker.
    app.auto_refresh(app.refresh_interval);
    Ok(())
}

/// Draw one frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let log_lines = app.log.len().min(9) as u16;
    let log_height = if app.show_log && area.height > 22 {
        log_lines + 2
    } else {
        0
    };
    let detail_height = if app.tab == Tab::Flow && area.height > 24 {
        8
    } else {
        0
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(detail_height),
            Constraint::Length(log_height),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, chunks[0], app);
    match app.tab {
        Tab::Flow => flow::draw(frame, chunks[1], app),
        Tab::Live => live::draw(frame, chunks[1], app),
        Tab::Inspect => inspect::draw(frame, chunks[1], app),
    }
    if detail_height > 0 {
        draw_detail(frame, chunks[2], app);
    }
    if log_height > 0 {
        draw_log(frame, chunks[3], app);
    }
    draw_footer(frame, chunks[4], app);
    draw_modal(frame, area, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();
    for tab in [Tab::Flow, Tab::Live, Tab::Inspect] {
        let title = format!(" {} ", tab.title());
        if tab == app.tab {
            spans.push(Span::styled(title, style::tab_style(true)));
        } else {
            spans.push(Span::styled(title, style::tab_style(false)));
        }
        spans.push(Span::raw(" "));
    }
    // Whether the reading in hand is a reading of now sits on the tab line, which is short, and not
    // only on the context line below it, which carries the repo, the config and the container and can
    // run off the end of a narrow terminal. The age itself is on the line below; these two say the
    // rest: that the snapshot is older than the cadence allows (STALE), and that a fresh one is on
    // its way -- so nothing drawn here passes itself off as current.
    if app.is_stale() {
        spans.push(Span::styled(
            " STALE ",
            style::failure().add_modifier(Modifier::BOLD),
        ));
    }
    if app.collect_in_flight() {
        spans.push(Span::styled(
            " A READ IS IN FLIGHT ",
            style::warning().add_modifier(Modifier::BOLD),
        ));
    }
    let mut lines = vec![Line::from(spans)];
    let mut context = vec![
        Span::styled("repo ", style::dim()),
        Span::raw(app.config.repo.display().to_string()),
        Span::styled("   config ", style::dim()),
        Span::raw(app.snapshot.inspect.config_source.clone()),
        Span::styled("   container ", style::dim()),
        Span::raw(app.config.console.container.clone()),
        Span::styled("   read ", style::dim()),
        Span::raw(iso::age_text(
            app.last_refresh,
            std::time::SystemTime::now(),
        )),
    ];
    if app.snapshot.live.run.in_flight {
        context.push(Span::styled(
            format!("   RUN IN FLIGHT (el {})", app.snapshot.live.run.elapsed),
            style::warning().add_modifier(Modifier::BOLD),
        ));
    }
    if !app.snapshot.warnings.is_empty() {
        context.push(Span::styled(
            format!("   warnings: {}", app.snapshot.warnings.len()),
            style::failure(),
        ));
    }
    lines.push(Line::from(context));
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

/// The selected box's provenance, under the FLOW map.
fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let lines = flow::selected_lines(app);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(" SELECTED BOX ", style::section())),
            ),
        area,
    );
}

fn draw_log(frame: &mut Frame, area: Rect, app: &App) {
    let height = area.height.saturating_sub(2) as usize;
    let start = app.log.len().saturating_sub(height);
    let lines: Vec<Line> = app.log[start..]
        .iter()
        .map(|line| {
            let stamp = iso::format_utc(line.at);
            let clock = stamp.chars().skip(11).take(8).collect::<String>();
            Line::from(vec![
                Span::styled(format!("{clock} "), style::dim()),
                Span::raw(line.text.clone()),
            ])
        })
        .collect();
    let running = app.actions_running();
    let title = if running > 0 {
        format!(" ACTION LOG -- {running} action(s) running ")
    } else {
        " ACTION LOG ".to_string()
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(title, style::section())),
            ),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let mut status = if app.status.is_empty() {
        "ready".to_string()
    } else {
        app.status.clone()
    };
    // The header says a read is in flight beside a long line of repo, config and container; this says
    // it too, on the line that has room for the sentence -- so the one thing the operator must not
    // misread, that these readings are the last completed read, is on the screen wherever it fits.
    if app.collect_in_flight() {
        status = format!(
            "{status}   [a read is in flight: the readings on screen are the last completed read]"
        );
    }
    let keys =
        "q quit  1/2/3 tabs  j/k select  r refresh  a actions  Enter approve  e chooser  t brief  L log  ? help";
    // The status bar carries the checkpoint card's state, not only the card: the state word alone
    // would repeat the overclaim, so the state's own name and what it rests on are printed here,
    // in the state's own colour, whatever screen is open.
    let card = app.card();
    let checkpoint = format!(
        "  checkpoint: {}{}",
        card.state.basis_line(),
        match &card.checkpoint_conflict {
            Some(_) => " -- the reads that named it disagree, so no ruling is offered",
            None => "",
        }
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(checkpoint, style::card_style(card.state)),
                Span::styled(format!("   {status}"), style::warning()),
            ]),
            Line::from(Span::styled(format!(" {keys}"), style::dim())),
        ]))
        .block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn centered(area: Rect, percent_x: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height.min(area.height)),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn draw_modal(frame: &mut Frame, area: Rect, app: &App) {
    match &app.mode {
        Mode::Normal => {}
        Mode::Help => {
            let popup = centered(area, 84, 26);
            frame.render_widget(Clear, popup);
            let mut lines = vec![Line::from(Span::styled(
                " agentic-console -- a read-only window onto this repository's agentic gate",
                style::section(),
            ))];
            lines.push(Line::from(""));
            for (key, what) in KEY_REFERENCE {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {key:<28}"), style::warning()),
                    Span::raw((*what).to_string()),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  nothing here writes inside the repository, restarts a container or kills a process",
                style::dim(),
            )));
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(Span::styled(
                        " KEY REFERENCE -- any other key closes ",
                        style::section(),
                    ))),
                popup,
            );
        }
        Mode::Menu { selection } => {
            let kinds = actions::all_kinds();
            let popup = centered(area, 78, (kinds.len() as u16) + 7);
            frame.render_widget(Clear, popup);
            let mut lines = vec![Line::from(Span::styled(
                "  choose an action; its exact command is shown before anything runs",
                style::dim(),
            ))];
            for (index, kind) in kinds.iter().enumerate() {
                let line = Line::from(vec![
                    Span::raw(if index == *selection { " > " } else { "   " }),
                    Span::styled(format!("{:<40}", kind.title()), Style::default()),
                    Span::styled(kind.value_hint().to_string(), style::dim()),
                ]);
                lines.push(if index == *selection {
                    line.style(style::selected())
                } else {
                    line
                });
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  no destructive action is offered: no rm, no restart, no config write, no git",
                style::dim(),
            )));
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(Span::styled(
                        " ACTION MENU -- j/k move, Enter choose, Esc close ",
                        style::section(),
                    ))),
                popup,
            );
        }
        Mode::Choose { selection } => {
            // The applicable rulings, not every configured one: the rows drawn here are the rows the
            // digits pick, so a wording scoped to another checkpoint must not be given a number.
            let rulings = app.applicable_rulings();
            let rows = rulings.len() + 1;
            let height = ((rows as u16) * 3 + 8).min(area.height);
            let popup = centered(area, 88, height);
            frame.render_widget(Clear, popup);
            let mut lines = vec![Line::from(Span::styled(
                "  the exact wording of every ruling is below; choosing one only reaches the \
                 confirmation screen",
                style::dim(),
            ))];
            for (index, ruling) in rulings.iter().enumerate() {
                let selected = index == *selection;
                let head = Line::from(vec![
                    Span::raw(if selected { " > " } else { "   " }),
                    Span::styled(format!("{} ", index + 1), style::warning()),
                    Span::styled(format!("{:<34}", ruling.label), Style::default()),
                    Span::styled(
                        if ruling.prefill {
                            "opens the box prefilled, for amendment"
                        } else {
                            "sent as written (straight to the confirmation)"
                        },
                        style::dim(),
                    ),
                ]);
                lines.push(if selected {
                    head.style(style::selected())
                } else {
                    head
                });
                lines.push(Line::from(vec![
                    Span::styled("      sends: ", style::dim()),
                    Span::raw(ruling.text.clone()),
                ]));
            }
            let index = rulings.len();
            let selected = index == *selection;
            let head = Line::from(vec![
                Span::raw(if selected { " > " } else { "   " }),
                Span::styled("c ", style::warning()),
                Span::styled(format!("{:<34}", "free text"), Style::default()),
                Span::styled("opens an empty box, as e always did", style::dim()),
            ]);
            lines.push(if selected {
                head.style(style::selected())
            } else {
                head
            });
            if let Some(default) = rulings.first() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  Enter on LIVE, without this chooser, sends the default ({}): {}",
                        default.label, default.text
                    ),
                    style::dim(),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  nothing is sent from this screen: the confirmation screen shows the exact \
                 command and asks its own key",
                style::dim(),
            )));
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(Span::styled(
                        " RULING CHOOSER -- 1-9 or Enter pick, j/k move, c free text, Esc close ",
                        style::section(),
                    ))),
                popup,
            );
        }
        Mode::Input { kind, text } => {
            let popup = centered(area, 84, 9);
            frame.render_widget(Clear, popup);
            let hint = if kind.value_hint().is_empty() {
                "value".to_string()
            } else {
                kind.value_hint().to_string()
            };
            let shown: String = if text.chars().count() > 400 {
                format!(
                    "...{}",
                    text.chars()
                        .skip(text.chars().count() - 400)
                        .collect::<String>()
                )
            } else {
                text.clone()
            };
            let lines = vec![
                Line::from(vec![
                    Span::styled("prompt : ", style::dim()),
                    Span::raw(hint),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("> ", style::warning()), Span::raw(shown)]),
                Line::from(""),
                Line::from(Span::styled(
                    "Enter reviews the exact command; Esc cancels. Nothing runs from this screen.",
                    style::dim(),
                )),
            ];
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(Span::styled(
                        format!(" {} ", kind.title()),
                        style::section(),
                    ))),
                popup,
            );
        }
        Mode::Confirm { command } => {
            let height =
                (command.note.len() as u16 + command.writes.len() as u16 + 12).min(area.height);
            let popup = centered(area, 90, height);
            frame.render_widget(Clear, popup);
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("about to run: ", style::dim()),
                    Span::styled(command.kind.title().to_string(), style::section()),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    format!("cwd     : {}", command.cwd.display()),
                    style::dim(),
                )),
                Line::from(vec![
                    Span::styled("command : ", style::warning()),
                    Span::styled(
                        command.display(),
                        style::warning().add_modifier(Modifier::BOLD),
                    ),
                ]),
            ];
            lines.push(Line::from(Span::styled(
                "          executed as argv, element by element; no shell parses it",
                style::dim(),
            )));
            // The state this command was built under, carried on the command itself: what the
            // guards found, in the card's own words, so "no process is running" is read as the
            // stopped-at-a-checkpoint state the ruling resumes, not as a decision being awaited.
            lines.push(Line::from(Span::styled(
                format!("guards  : {}", command.guards.summary()),
                style::dim(),
            )));
            if !command.env.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "env     : {}",
                        command
                            .env
                            .iter()
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect::<Vec<String>>()
                            .join(" ")
                    ),
                    style::dim(),
                )));
            }
            for line in command.write_lines() {
                lines.push(Line::from(Span::styled(
                    format!("side    : {line}"),
                    style::warning(),
                )));
            }
            lines.push(Line::from(""));
            for note in &command.note {
                lines.push(Line::from(Span::styled(
                    format!("note    : {note}"),
                    style::dim(),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "press y (or Enter) to run it, n (or Esc) to cancel",
                style::warning().add_modifier(Modifier::BOLD),
            )));
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(Span::styled(
                        format!(" CONFIRM -- {} ", command.kind.id()),
                        style::section(),
                    ))),
                popup,
            );
        }
    }
}
