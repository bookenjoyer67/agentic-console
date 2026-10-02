//! The RUNTIME screen: is this machine actually ready?
//!
//! Every row is a prerequisite and the state the machine actually reported for it. A container is
//! named `running`, `exited` or `absent` -- never a boolean, because a container that exists and does
//! nothing is not the same finding as one that was never created. A row the console cannot read says
//! so in words rather than drawing a green tick.
//!
//! A row that is missing carries the exact command a human would run to fix it. That command is
//! printed and never executed: this panel reads and does not act, nothing here starts, stops, creates
//! or removes anything, and the decision stays with the person reading it. A wizard, not this screen,
//! is where a fix would be run with a confirmation.
//!
//! Width: the panel is built for 80-100 columns and widens above 160, where each row's own source
//! moves onto the row instead of a line beneath it. Every row is clipped to one line, so a row never
//! wraps into a second one and a row is never dropped silently -- what does not fit below is counted
//! in the title and reachable with `j`/`k`.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::state::{Light, RuntimeRow};

use super::style;

/// Draw the RUNTIME screen, scrolled by `app.scroll` rows.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    // The block's borders are not content columns.
    let width = usize::from(area.width).saturating_sub(2).max(1);
    let wide = area.width >= 160;
    let runtime = &app.snapshot.runtime;
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(style::clip_row(
        vec![
            Span::styled("  ", style::dim()),
            Span::styled(
                runtime.summary.clone(),
                summary_style(&runtime.summary).add_modifier(Modifier::BOLD),
            ),
        ],
        width,
    )));
    lines.push(Line::from(style::clip_row(
        vec![Span::styled(
            "  every row below names the read it rests on and the age of that read; a command shown is \
             a command this panel never runs",
            style::dim(),
        )],
        width,
    )));
    lines.push(Line::from(""));
    for row in &runtime.rows {
        push_row(&mut lines, row, width, wide);
    }

    // Each row is one line per span set (clipped, never wrapped), so the line count IS the row count:
    // nothing has to be measured, and a scroll can be clamped against a real number.
    let visible = usize::from(area.height).saturating_sub(2);
    let total = lines.len();
    let max_top = total.saturating_sub(visible);
    let top = app.scroll.min(max_top);
    let below = total.saturating_sub(top + visible);
    let title = format!(
        " RUNTIME -- is this machine ready? -- read-only, nothing here is run{} ",
        if below > 0 {
            format!(" -- {below} more rows below (j/k)")
        } else {
            String::new()
        }
    );
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(title, style::section())),
            )
            .scroll((top.min(u16::MAX as usize) as u16, 0)),
        area,
    );
}

/// The summary line's colour, from the summary's own opening word.
fn summary_style(summary: &str) -> Style {
    if summary.starts_with("READY") {
        style::light_style(Light::Ok)
    } else if summary.starts_with("NOT READY") {
        style::failure()
    } else {
        style::warning()
    }
}

/// Append one prerequisite's lines: the row, its detail, its read, and -- when something is missing
/// -- the exact command that would fix it.
fn push_row(lines: &mut Vec<Line<'static>>, row: &RuntimeRow, width: usize, wide: bool) {
    let mut head = vec![
        Span::styled(
            format!(" {} ", row.light.glyph()),
            style::light_style(row.light),
        ),
        Span::styled(format!("{:<24}", row.label), Style::default()),
        Span::styled(format!("{:<14}", row.state), style::light_style(row.light)),
    ];
    if !row.key.is_empty() {
        head.push(Span::styled(format!("  {}", row.key), style::dim()));
    }
    // Above 160 columns there is room for the read on the same line, and the panel widens rather
    // than leaving the row short.
    if wide {
        head.push(Span::styled(
            format!("   source {}", row.source),
            style::dim(),
        ));
    }
    lines.push(Line::from(style::clip_row(head, width)));
    lines.push(Line::from(style::clip_row(
        vec![Span::styled(format!("      {}", row.detail), style::dim())],
        width,
    )));
    if !wide {
        lines.push(Line::from(style::clip_row(
            vec![Span::styled(
                format!("      source {} ({})", row.source, row.age),
                style::dim(),
            )],
            width,
        )));
    }
    if !row.fix.is_empty() {
        lines.push(Line::from(style::clip_row(
            vec![
                Span::styled("      fix    ", style::warning()),
                Span::styled(row.fix.clone(), style::warning()),
                Span::styled("   (shown, never run from here)", style::dim()),
            ],
            width,
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::state::{Probes, Snapshot};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::Path;
    use std::time::SystemTime;

    fn text(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw(frame, area, app);
            })
            .expect("draw the runtime panel");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .map(|cell| cell.symbol().to_string())
                            .unwrap_or_default()
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_panel_draws_every_row_and_says_a_shown_command_is_never_run() {
        let config = Config::load(
            Path::new("/nonexistent/agentic-console-runtime-panel"),
            None,
        );
        let at = SystemTime::now();
        let snapshot = Snapshot::from_parts(config.clone(), Probes::empty(&config, at));
        let mut app = App::new(config);
        app.snapshot = snapshot;

        // Every docker read failed (`not probed`), so this is the panel's hardest case: it must still
        // draw every row, name the unread state in words, and never claim the fix was run.
        let wide = text(&app, 200, 50);
        assert!(
            wide.contains("RUNTIME"),
            "the panel draws its own title\n{wide}"
        );
        assert!(wide.contains("docker daemon"), "{wide}");
        assert!(wide.contains("MCP server gate"), "{wide}");
        assert!(
            wide.contains("nothing here is run") && wide.contains("never run from here"),
            "the read-only contract is on the panel\n{wide}"
        );

        // The narrow target: same panel, no panic, still one bordered block.
        let narrow = text(&app, 80, 24);
        assert!(narrow.contains("RUNTIME"), "{narrow}");
    }
}
