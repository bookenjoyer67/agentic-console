//! The INSPECT screen: the machinery, as tables.
//!
//! Config seams, the role x mount matrix, the grant grid, the suites, the conversion candidates and
//! the ADRs. Each section names the file it was parsed from and the age of that reading, so a stale
//! table is visibly stale.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
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

    // Last, because this document is scrolled and every section above it was here first: a new
    // section goes below the ones an operator already finds where they were.
    lines.push(blank());
    lines.push(section(
        "SCORECARD (scripts/redteam_scorecard.py, scored by Azathoth)",
    ));
    lines.push(dim(&format!(
        "  source: {} ({})",
        inspect.scorecard_source, inspect.scorecard_age
    )));
    if let Some(note) = &inspect.scorecard_note {
        lines.push(warn(&format!("  {}", truncate(note, 96))));
    }
    for row in &inspect.scorecards {
        lines.push(Line::from(vec![
            Span::raw(format!("  #{} ", row.rank)),
            Span::styled(
                format!("{:<12} ", truncate(&row.revision, 12)),
                style::section(),
            ),
            Span::raw(format!("overall {:<6} ", row.overall)),
            Span::styled(format!("quality {:<6} ", row.quality), style::dim()),
            Span::raw(format!("cases {}", row.cases)),
        ]));
    }
    if !inspect.scorecard_winner.is_empty() {
        lines.push(dim(&format!("  winner: {}", inspect.scorecard_winner)));
    }
    if !inspect.scorecard_axes.is_empty() {
        // The artifact's own sentence, printed verbatim: which axes carry no measurement is the
        // artifact's claim about its own scoring, and the console does not restate it.
        lines.push(dim(&format!("  {}", truncate(&inspect.scorecard_axes, 96))));
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

/// A marker line appended to a measurement render: its row index is the document's row count.
///
/// Not content, and never drawn: it exists only inside the measuring buffer, where the block's own
/// borders are rendered too, so its row already accounts for them. The MATCH below is deliberately
/// short, because a panel narrower than the marker wraps it and only its head reaches the first row --
/// a longer match silently measured nothing (and a silent nothing is how the row count went wrong).
const BUDGET_SENTINEL: &str = "@@budget@@";

/// The head the scan looks for. Short enough to survive the wrap in any usable panel width.
const BUDGET_MARK: &str = "@@bud";

/// The block: one bordered panel with the row position in its title.
fn panel(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, style::section()))
}

/// The whole screen as a widget: one scrolled document, with its title in the block.
fn document(body: Vec<Line<'static>>, title: String) -> Paragraph<'static> {
    Paragraph::new(Text::from(body))
        .wrap(Wrap { trim: false })
        .block(panel(title))
}

/// How many rows a body needs at this width, AS THE RENDERER COUNTS THEM, or `None` if it cannot be
/// measured at all.
///
/// Measured, not calculated: ratatui's own exact count (`Paragraph::line_count`) sits behind the
/// unstable `rendered-line-info` feature, and this crate does not take unstable dependencies. So the
/// body is rendered for real into a scratch buffer with one sentinel line appended, and the sentinel's
/// row index is the answer -- wrapping and the block's borders included, decided by the same code that
/// will draw the panel. Nothing here predicts where the text will break.
///
/// `None` is the honest failure. A caller that cannot learn the row count must not clamp against a
/// guess: an OVER-estimate merely lets a scroll run past the end, while an under-estimate puts the
/// tail of the document permanently out of reach. Prefer the harmless failure.
fn measured_rows(body: &[Line<'static>], width: u16) -> Option<usize> {
    let inner = usize::from(width).saturating_sub(2).max(1);
    // The buffer must be tall enough to draw the sentinel, or the sentinel is clipped and this measures
    // nothing -- and a clipped sentinel silently returns a count that is TOO SMALL, which is the
    // failure that hides content. So the bound has to be a real over-estimate, and "a line wraps to
    // `width / inner + 1` rows" is not one: word wrapping breaks early whenever the next word does not
    // fit, so a row can end up holding a single short word and a line can need one row per word. Each
    // break is caused either by a space the next word could not follow, or by a word too long for a
    // whole row. Counting both bounds the rows from above.
    let bound: usize = 3 + body
        .iter()
        .map(|line| {
            let spaces: usize = line
                .spans
                .iter()
                .map(|span| span.content.matches(' ').count())
                .sum();
            1 + spaces + line.width() / inner
        })
        .sum::<usize>();
    let height = u16::try_from(bound).unwrap_or(u16::MAX);
    let mut measured = body.to_vec();
    measured.push(Line::from(BUDGET_SENTINEL));
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    document(measured, String::new()).render(buffer.area, &mut buffer);
    for y in 0..height {
        let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        // `contains`, not `starts_with`: the block draws its own left border into column 0, so the row
        // reads `|@@budget@@ ...|`. Matching at the start measured nothing at every width, and the
        // caller's fallback then hid the failure wherever no line wrapped.
        if row.contains(BUDGET_MARK) {
            // The sentinel renders below the block's top border, so the content rows are above it.
            return Some(usize::from(y).saturating_sub(1));
        }
    }
    None
}

/// The measured row count for this screen's document, or `None` when it could not be measured.
fn document_rows(app: &App, width: u16) -> Option<usize> {
    measured_rows(&lines(app), width)
}

/// Draw the screen, scrolled by `app.scroll` lines.
///
/// The panel has a row budget, and it is the row count the renderer reports for this width, never an
/// arithmetic guess about how the text will fall. Three things follow from having the real number: the
/// scroll is clamped so the panel cannot be scrolled past its last row into a blank screen, the title
/// says which row of how many is at the top, and when rows remain below the window the title says how
/// many. Content that does not fit is therefore ANNOUNCED and still reachable -- never silently cut,
/// and never silently absent. When the count cannot be measured at all, the panel drops the numbers
/// AND the clamp rather than inventing either: a scroll past the end is harmless, a hidden tail is not.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    // The block's own top and bottom borders are not content rows.
    let visible = usize::from(area.height).saturating_sub(2);

    let Some(rows) = document_rows(app, area.width) else {
        frame.render_widget(
            document(
                lines(app),
                " INSPECT -- j/k to scroll, end not measured ".to_string(),
            )
            .scroll((app.scroll.min(u16::MAX as usize) as u16, 0)),
            area,
        );
        return;
    };

    let max_top = rows.saturating_sub(visible);
    let top = app.scroll.min(max_top);
    // What remains below the window that is actually on the glass, not below the fold in general:
    // at the end of the document this is 0 and the title stops saying there is more.
    let below = rows.saturating_sub(top + visible);
    // The clause that must never be lost goes FIRST: a bordered title clips at its END, so the row
    // position and the count of rows below the window precede the scroll hint. The config path is not
    // in the title at all -- the footer line carries it, and so does this panel's own CONFIG SEAMS
    // section, so a long path cannot eat the one clause the operator needs at a narrow width.
    let title = format!(
        " INSPECT -- row {}/{} -- j/k to scroll{} ",
        if rows == 0 { 0 } else { top + 1 },
        rows,
        if below > 0 {
            format!(" -- {below} more rows below")
        } else {
            String::new()
        }
    );
    let widget = document(lines(app), title).scroll((top.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(widget, area);
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    /// The regression this guards: the measurement buffer was sized by `width / inner + 1` per line,
    /// which is not an upper bound once word wrapping breaks early. The sentinel was then clipped
    /// below the buffer, no count came back, and the caller's fallback UNDER-counted the rows -- so the
    /// clamp stopped short and the tail of the real INSPECT document became unreachable.
    #[test]
    fn the_row_count_allows_for_the_breaks_word_wrapping_forces() {
        // 12 columns wide leaves 10 content columns. Ten six-character words separated by spaces: no
        // two of them fit on a row together (6 + 1 + 6 > 10), so the renderer needs one row per word.
        // The naive bound allows 69 / 10 + 1 = 7, which is exactly how the sentinel got clipped.
        let body = [Line::from(["aaaaaa"; 10].join(" "))];
        assert_eq!(
            measured_rows(&body, 12),
            Some(10),
            "one row per word, not the width-based estimate"
        );
    }

    #[test]
    fn a_document_of_plain_lines_is_counted_exactly() {
        let body = vec![Line::from("one"), Line::from("two"), blank()];
        assert_eq!(measured_rows(&body, 40), Some(3));
    }

    #[test]
    fn an_empty_document_needs_no_rows() {
        assert_eq!(measured_rows(&[], 40), Some(0));
    }

    /// Long unbroken text cannot break at a space, so it fills each row completely -- the case the
    /// naive bound was correct for. Both shapes have to be counted right.
    #[test]
    fn a_row_that_fills_completely_is_counted_once() {
        let body = vec![Line::from("x".repeat(25))];
        // 12 columns wide leaves 10 content columns: three rows for 25 unbroken characters.
        assert_eq!(measured_rows(&body, 12), Some(3));
    }
}
