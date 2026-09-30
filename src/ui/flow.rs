//! The FLOW screen: the map with live lights, four lanes, and the provenance of the selected box.
//!
//! Nothing is drawn from a table baked into the renderer. Lane A comes from the parsed workflow file,
//! lane B from the config's ordered sequence plus the journals, lane C from the grant map and the
//! classification document, and lane D from `roles.mounts` plus `docker ps`.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::state::Lane;

use super::style;

/// Draw the whole screen: the four lanes in two columns.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(8)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(11)])
        .split(columns[1]);
    let slots = [left[0], left[1], right[0], right[1]];
    for (index, lane) in app.snapshot.flow.lanes.iter().enumerate() {
        if let Some(slot) = slots.get(index) {
            draw_lane(frame, *slot, lane, app, index);
        }
    }
}

/// The flat selection index of a lane's node, matching `FlowView::selection`.
fn flat_index(app: &App, lane_index: usize, node_index: usize) -> Option<usize> {
    app.snapshot
        .flow
        .selection
        .iter()
        .position(|(lane, node)| *lane == lane_index && *node == node_index)
}

fn draw_lane(frame: &mut Frame, area: Rect, lane: &Lane, app: &App, lane_index: usize) {
    let title = format!(" LANE {} -- {} ", lane.key, lane.title);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, style::section()));
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(lane.blurb.clone(), style::dim())));
    if lane.key == 'A' && !lane.edges.is_empty() {
        lines.push(Line::from(Span::styled(edges_text(lane), style::dim())));
    }
    let label_width = label_width(area.width);
    for (node_index, node) in lane.nodes.iter().enumerate() {
        let selected = flat_index(app, lane_index, node_index) == Some(app.selection);
        let marker = if selected { ">" } else { " " };
        let line = Line::from(vec![
            Span::styled(
                format!("{marker} {} ", node.light.glyph()),
                style::light_style(node.light),
            ),
            Span::styled(
                format!(
                    "{:<width$} ",
                    truncate(&node.label, label_width),
                    width = label_width
                ),
                Style::default(),
            ),
            Span::styled(node.status.clone(), style::dim()),
        ]);
        lines.push(if selected {
            line.style(style::selected())
        } else {
            line
        });
    }
    for note in &lane.footnote {
        lines.push(Line::from(Span::styled(
            format!("  . {note}"),
            style::dim(),
        )));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// The needs chain of a lane, spelled out from the parsed edges.
fn edges_text(lane: &Lane) -> String {
    let mut parts = Vec::new();
    for (parent, child) in &lane.edges {
        if let (Some(from), Some(to)) = (lane.nodes.get(*parent), lane.nodes.get(*child)) {
            parts.push(format!("{} <- {}", to.label, from.label));
        }
    }
    format!("  chain: {}", parts.join("; "))
}

/// How wide the label column is: as wide as the pane allows, within reading limits.
fn label_width(area_width: u16) -> usize {
    let available = area_width.saturating_sub(46) as usize;
    available.clamp(28, 48)
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        let kept: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{kept}~")
    }
}

/// The provenance pane the FLOW screen shows for the selected box.
pub fn selected_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    match app.snapshot.flow.selected(app.selection) {
        Some((lane, node)) => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", node.light.glyph()),
                    style::light_style(node.light),
                ),
                Span::styled(
                    format!("LANE {} / {}", lane.key, node.label),
                    style::section(),
                ),
                Span::styled(format!("  [{}]", node.light.word()), style::dim()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  status    : ", style::dim()),
                Span::raw(node.status.clone()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  artifact  : ", style::dim()),
                Span::styled(node.artifact.clone(), style::warning()),
            ]));
            for line in node.provenance.iter().take(3) {
                lines.push(Line::from(vec![
                    Span::styled("  source    : ", style::dim()),
                    Span::raw(line.clone()),
                ]));
            }
        }
        None => lines.push(Line::from(Span::styled(
            "no box is selected (the map is empty)",
            style::dim(),
        ))),
    }
    lines
}
