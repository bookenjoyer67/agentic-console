//! The INSPECT screen: the machinery, as tables.
//!
//! Config seams, the role x mount matrix, the grant grid, the suites, the conversion candidates and
//! the ADRs. Each section names the file it was parsed from and the age of that reading, so a stale
//! table is visibly stale.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::state::SeamClass;

use super::style;

/// Build the whole screen as lines, so both the renderer and `--dump` can use them.
pub fn lines(app: &App) -> Vec<Line<'static>> {
    let inspect = &app.snapshot.inspect;
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(section("CONFIG SEAMS (agentic.config.json)"));
    lines.push(dim(&format!(
        "  config: {}   ({})   container {}",
        inspect.config_source, inspect.config_age, inspect.container
    )));
    if inspect.container_overridden {
        lines.push(warn(
            "  the container name was overridden on the command line (--container)",
        ));
    }
    for seam in &inspect.seams {
        let word = style::seam_word(seam.class);
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<52} ", seam.key), style::dim()),
            Span::styled(
                format!("{:<34} ", truncate(&seam.value, 34)),
                style::seam_style(seam.class),
            ),
            Span::styled(word.to_string(), style::seam_style(seam.class)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  class column: ", style::dim()),
        Span::styled(
            "still a Komun default",
            style::seam_style(SeamClass::RepoSpecific),
        ),
        Span::styled("  vs  ", style::dim()),
        Span::styled(
            "changed for this repo / generic",
            style::seam_style(SeamClass::Generic),
        ),
    ]));

    lines.push(blank());
    lines.push(section("ROLE x MOUNT MATRIX (roles.mounts)"));
    lines.push(dim("  role              workspace  memory     build-cache"));
    for mount in &inspect.mounts {
        lines.push(Line::from(format!(
            "  {:<17} {:<10} {:<10} {}",
            mount.role, mount.workspace, mount.memory, mount.build_cache
        )));
    }

    lines.push(blank());
    lines.push(section("GRANT GRID (who may do what)"));
    lines.push(dim(&format!(
        "  source: {} ({})",
        inspect.grant_source, inspect.grant_age
    )));
    lines.push(dim(&format!(
        "  project {}   servers: {}",
        inspect.grant_project,
        inspect.grant_servers.join(", ")
    )));
    for row in &inspect.grants {
        let tools: Vec<String> = row
            .groups
            .iter()
            .map(|(server, names)| format!("{server}: {}", names.join(", ")))
            .collect();
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<17} ", row.role), style::section()),
            Span::styled(format!("ceiling {:<9} ", row.ceiling), style::dim()),
            Span::raw(if tools.is_empty() {
                "no grant".to_string()
            } else {
                tools.join("  |  ")
            }),
        ]));
    }

    lines.push(blank());
    lines.push(section(
        "SUITES AND GATES (toolchain.commands + the gate journal)",
    ));
    for suite in &inspect.suites {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", suite.light.glyph()),
                style::light_style(suite.light),
            ),
            Span::styled(format!("{:<14} ", suite.gate), style::section()),
            Span::raw(suite.last.clone()),
        ]));
        lines.push(dim(&format!("      argv: {}", suite.argv)));
        lines.push(dim(&format!("      file: {}", suite.path)));
    }

    lines.push(blank());
    lines.push(section(
        "CONVERSION CANDIDATES (docs/step-classification.md)",
    ));
    lines.push(dim(&format!(
        "  source: {} ({})",
        inspect.conversion_source, inspect.conversion_age
    )));
    for row in &inspect.conversions {
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<52} ", truncate(&row.step, 52))),
            Span::styled(
                format!("{:<46} ", truncate(&row.status, 46)),
                style::warning(),
            ),
            Span::styled(format!("next review {}", row.next_review), style::dim()),
        ]));
        lines.push(dim(&format!("      {}", row.source)));
    }
    if inspect.conversions.is_empty() {
        lines.push(warn("  no `## Step:` section was found in that document"));
    }

    lines.push(blank());
    lines.push(section("ADRs"));
    if inspect.adrs.is_empty() {
        lines.push(warn(
            "  no .md file under the ADR directory the grant map cites",
        ));
    }
    for adr in &inspect.adrs {
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<64} ", truncate(&adr.file, 64))),
            Span::styled(adr.title.clone(), style::section()),
        ]));
        lines.push(dim(&format!("      {}", adr.meta)));
    }
    lines
}

fn section(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), style::section()))
}

fn dim(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), style::dim()))
}

fn warn(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), style::warning()))
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        format!(
            "{}~",
            text.chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        )
    }
}

/// Draw the screen, scrolled by `app.scroll` lines.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let body = lines(app);
    let title = format!(
        " INSPECT -- {} (scroll {}) -- j/k to scroll ",
        app.snapshot.inspect.config_source, app.scroll
    );
    let widget = Paragraph::new(Text::from(body))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll.min(u16::MAX as usize) as u16, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, style::section())),
        );
    frame.render_widget(widget, area);
}
