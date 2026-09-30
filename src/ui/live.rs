//! The LIVE screen: what is happening right now, and the checkpoint card.
//!
//! Every panel here names its source and the age of its reading. The checkpoint card is the point of
//! the screen: when the evidence says a human decision is waiting, the card names that checkpoint,
//! shows the evidence for it, shows the exact command a ruling would run, and refuses to offer a
//! ruling when there is nothing to resume.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::iso;
use crate::state::head_chars;

use super::style;

/// Draw the LIVE screen.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            // The card's own rows: header, checkpoint, decision, three evidence lines, the session it
            // would resume and the read that named it, one line for what that read says about the
            // checkpoint, the command, and its refusal or offer line -- and the evidence, the command
            // and the refusal can each wrap, so the box has to be tall enough that the line the
            // operator acts on (or is refused by) is drawn. Three evidence lines rather than two
            // because the attribution is one of them: which transcript was read, and why the others
            // in the evidence directory were not.
            Constraint::Length(17),
            Constraint::Min(6),
        ])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[0]);
    draw_run(frame, top[0], app);
    draw_checks(frame, top[1], app);
    draw_checkpoint(frame, rows[1], app);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(rows[2]);
    draw_gate_table(frame, bottom[0], app);
    draw_storage(frame, bottom[1], app);
}

fn block(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, style::section()))
}

fn draw_run(frame: &mut Frame, area: Rect, app: &App) {
    let run = &app.snapshot.live.run;
    let mut lines: Vec<Line> = Vec::new();
    let (word, styled) = if run.in_flight {
        ("IN FLIGHT", style::warning())
    } else {
        ("no agent process", style::dim())
    };
    lines.push(Line::from(vec![
        Span::styled("run       : ", style::dim()),
        Span::styled(word.to_string(), styled.add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                "  container {} ({})",
                run.container,
                if run.container_running {
                    "running"
                } else {
                    "NOT running"
                }
            ),
            style::dim(),
        ),
    ]));
    if run.in_flight {
        lines.push(Line::from(vec![
            Span::styled("process   : ", style::dim()),
            Span::raw(format!("pid {} el {}", run.pid, run.elapsed)),
            Span::styled(format!("  mode {}", run.mode), style::dim()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("brief     : ", style::dim()),
            Span::raw(format!(
                "{} characters in the -p argument",
                run.prompt_chars
            )),
        ]));
        if let Some(hint) = &run.checkpoint_hint {
            lines.push(Line::from(vec![
                Span::styled("phase     : ", style::dim()),
                Span::styled(format!("the brief names {hint}"), style::warning()),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("phase     : ", style::dim()),
                Span::raw("the brief names no checkpoint".to_string()),
            ]));
        }
    }
    lines.push(Line::from(vec![
        Span::styled("gates     : ", style::dim()),
        Span::raw(app.snapshot.live.gate_names.join(", ")),
        Span::styled(
            format!("   ports: {}", app.snapshot.live.port_line),
            style::dim(),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("reading   : ", style::dim()),
        Span::raw(format!("{} ({})", run.source, run.source_age)),
    ]));
    if let Some(error) = &run.error {
        lines.push(Line::from(Span::styled(
            format!("error     : {error}"),
            style::failure(),
        )));
    }
    match &app.snapshot.live.gate_allowlist_error {
        Some(error) => lines.push(Line::from(vec![
            Span::styled("allowlist : ", style::dim()),
            Span::styled(
                format!("gate server could not be asked -- {error}"),
                style::failure(),
            ),
        ])),
        None => lines.push(Line::from(vec![
            Span::styled("allowlist : ", style::dim()),
            Span::raw(app.snapshot.live.gate_allowlist.join(", ")),
            Span::styled(
                format!(
                    "   (the server's own list_gates, {})",
                    app.snapshot.live.gate_allowlist_age
                ),
                style::dim(),
            ),
        ])),
    }
    let title = format!(" RUN -- {} ", iso::format_utc(app.snapshot.read_at));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block(title)),
        area,
    );
}

fn draw_checks(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "the three repository checks, last known result",
        style::dim(),
    )));
    for check in &app.snapshot.live.checks {
        // One line per check: three of them must fit even in a short pane, and the full text with
        // its source is in `--dump`.
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", check.light.glyph()),
                style::light_style(check.light),
            ),
            Span::styled(
                format!("{:<32} ", check.name),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{} ", check.last), Style::default()),
            Span::styled(format!("[{} {}]", check.source, check.age), style::dim()),
        ]));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block(" CHECKS ".to_string())),
        area,
    );
}

/// The checkpoint card: the most important element on the screen.
pub fn draw_checkpoint(frame: &mut Frame, area: Rect, app: &App) {
    let card = app.card();
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("CHECKPOINT CARD  ", style::section()),
        Span::styled(format!(" {} ", card.word()), style::card_style(card.state)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("checkpoint : ", style::dim()),
        Span::styled(
            card.which.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("decision   : ", style::dim()),
        Span::raw(card.question.clone()),
    ]));
    // Three evidence lines: the liveness reading, the session's own transcript (the primary
    // evidence, or why there is none), and the attribution -- which transcript in the evidence
    // directory was read and why the others were not. `--dump` carries all of them.
    for line in card.evidence.iter().take(3) {
        lines.push(Line::from(vec![
            Span::styled("evidence   : ", style::dim()),
            Span::raw(line.clone()),
        ]));
    }
    // The session the ruling would resume, right beside the checkpoint it has to belong to: the
    // whole point of the card is that the evidence names the conversation, so the operator can see
    // the id, the path and the age of the one the command would use -- and read the line under it
    // when the checkpoint and the session came from different reads, or when the container's newest
    // session is a different one.
    let session_style = match card.session.named() {
        Some(_) => Style::default().add_modifier(Modifier::BOLD),
        None => style::failure(),
    };
    lines.push(Line::from(vec![
        Span::styled("session    : ", style::dim()),
        Span::styled(head_chars(&card.session.line(), 170), session_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("session src: ", style::dim()),
        Span::raw(head_chars(&card.session.source_line(), 170)),
    ]));
    for note in card.session.notes().iter().take(1) {
        lines.push(Line::from(vec![
            Span::styled("session    : ", style::dim()),
            Span::styled(head_chars(note, 170), style::warning()),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("command    : ", style::dim()),
        Span::styled(head_chars(&card.command_preview, 170), style::warning()),
    ]));
    match (&card.refuse_reason, card.can_approve) {
        (_, true) => lines.push(Line::from(vec![
            Span::styled("ruling     : ", style::dim()),
            Span::raw(
                "Enter sends the default canned ruling, e opens the chooser (its 1/2/3 pick a \
                 ruling there) -- the confirmation screen shows the exact command either way"
                    .to_string(),
            ),
        ])),
        (Some(reason), false) => {
            // The refusal is bounded here -- the full sentence is in `--dump` and on the status
            // line -- so the box always has room for it and for the line above it.
            lines.push(Line::from(Span::styled(
                format!("no ruling  : {}", head_chars(reason, 320)),
                style::failure(),
            )));
        }
        _ => {}
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block(" CHECKPOINT ".to_string())),
        area,
    );
}

fn draw_gate_table(frame: &mut Frame, area: Rect, app: &App) {
    let live = &app.snapshot.live;
    let header = Row::new(vec![
        Cell::from("gate"),
        Cell::from("exit"),
        Cell::from("role"),
        Cell::from("dur s"),
        Cell::from("guard"),
        Cell::from("timestamp"),
    ])
    .style(style::section());
    let rows: Vec<Row> = live
        .gates
        .iter()
        .map(|entry| {
            let colour = if entry.passed {
                style::light_style(crate::state::Light::Ok)
            } else {
                style::failure()
            };
            Row::new(vec![
                Cell::from(entry.gate.clone()),
                Cell::from(entry.exit_code.to_string()),
                Cell::from(entry.role.clone()),
                Cell::from(format!("{:.1}", entry.duration)),
                Cell::from(entry.guard_text()),
                Cell::from(entry.timestamp.chars().take(19).collect::<String>()),
            ])
            .style(colour)
        })
        .collect();
    let title = format!(
        " GATE JOURNAL (last 20 of {}) -- {} {} ",
        live.gate_total, live.gate_source, live.gate_age
    );
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(16),
            Constraint::Percentage(8),
            Constraint::Percentage(16),
            Constraint::Percentage(9),
            Constraint::Percentage(21),
            Constraint::Percentage(30),
        ],
    )
    .header(header)
    .block(block(title));
    frame.render_widget(table, area);
}

fn draw_storage(frame: &mut Frame, area: Rect, app: &App) {
    let live = &app.snapshot.live;
    let header = Row::new(vec![
        Cell::from("role"),
        Cell::from("operation"),
        Cell::from("entry"),
        Cell::from("class"),
        Cell::from("time"),
    ])
    .style(style::section());
    let rows: Vec<Row> = live
        .stores
        .iter()
        .map(|entry| {
            Row::new(vec![
                Cell::from(entry.role.clone()),
                Cell::from(entry.operation.clone()),
                Cell::from(entry.short_id()),
                Cell::from(entry.classification.clone()),
                Cell::from(entry.timestamp.chars().take(19).collect::<String>()),
            ])
        })
        .collect();
    let title = format!(
        " STORAGE JOURNAL -- {} {} ",
        live.store_source, live.store_age
    );
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(24),
            Constraint::Percentage(16),
            Constraint::Percentage(16),
            Constraint::Percentage(24),
        ],
    )
    .header(header)
    .block(block(title));
    frame.render_widget(table, area);
}
