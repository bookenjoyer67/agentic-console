//! The CONVERSATION screen: this console's own transcript, and the composer under it.
//!
//! Everything here is the console's own. The transcript is the one thing this console *owns* --
//! every other pane is a reading of something else, which is why every other pane prints its source
//! and its age. This one prints neither, and instead says what it is: the title names the buffer, the
//! item count, how many items the run's own transcript has confirmed, and how many are still only
//! this console's word. A pane that printed a source age for its own buffer would be claiming its own
//! memory arrived from somewhere.
//!
//! The four ideas the drawing holds:
//!
//! * **the gutter names the kind, the marker names the state.** `·` is live (the run's own words, not
//!   yet found in the session transcript), `✓` is confirmed (the session transcript carries that
//!   uuid), `!` is failed, and a blank marker is a line this console wrote itself. A line is never
//!   drawn as more certain than it is;
//! * **the transcript reads from the bottom**, like a terminal: the newest item is at the foot, and
//!   what does not fit is dropped from the top with a counted notice rather than clipped silently;
//! * **the composer is one line at every width.** It never gains a second field, and the confirmation
//!   for a prompt happens in it -- the argv, the guards, the notes and the question -- because that is
//!   where the operator is already looking;
//! * **the ring's drop notice is an item**, not a truncation: `… 240 earlier item(s) dropped` is drawn
//!   in the transcript's own first row, in the console's own voice.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode, Screen};
use crate::conversation::{Conversation, Item, ItemKind, ItemState};
use crate::ui::style;

/// The columns the kind label and the state marker occupy, on every row of an item.
///
/// Fifteen rather than twelve, and that is deliberate: the widest label this pane draws is
/// `orchestrator` (twelve characters), and a label truncated to fit a twelve-column gutter would
/// rename the run. The design target is 80 columns, which leaves 65 for text.
const GUTTER: usize = 15;

/// The columns one run of rows is indented by, so a wrapped item stays visibly one item.
const INDENT: &str = "               ";

/// Draw the CONVERSATION screen into `area`: header, transcript, optional action log, composer,
/// footer.
///
/// The header and the footer are the console's own, drawn by the same functions every other screen
/// uses, so the checkpoint card's state and the console's status say the same thing here as there.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let composer_rows = composer_rows(app, area.height);
    let log_rows = log_rows(app, area.height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(log_rows),
            Constraint::Length(composer_rows),
            // Three rows: the footer's top border, the checkpoint line and the key line. Two rows
            // clipped the key line for as long as this console has existed.
            Constraint::Length(3),
        ])
        .split(area);

    super::draw_header(frame, rows[0], app, Screen::Conversation);
    draw_transcript(frame, rows[1], app);
    if log_rows > 0 {
        super::draw_log(frame, rows[2], app);
    }
    draw_composer(frame, rows[3], app);
    super::draw_footer(frame, rows[4], app);
}

/// The action log pane's height, under the same rule the console has always used for `L`.
fn log_rows(app: &App, height: u16) -> u16 {
    if app.show_log && height > 22 {
        app.log.len().min(9) as u16 + 2
    } else {
        0
    }
}

/// How many rows the composer takes: three when it is a line, and as many as its confirmation needs
/// when a prompt is waiting for its key.
///
/// **The transcript keeps its eight rows whatever the confirmation wants.** A confirmation that ate
/// the transcript would hide the very thing the operator is deciding about, and the transcript is the
/// one pane in this console that cannot be reconstructed from a probe.
fn composer_rows(app: &App, height: u16) -> u16 {
    let wanted = match &app.mode {
        // Measured with the very lines the box will draw, so the box can never be one row shorter
        // than its confirmation: a "press y" that fell off the bottom would be the operator answering
        // a question they cannot read.
        Mode::PromptConfirm { command } => confirm_lines(app, command).len() as u16 + 2,
        _ => 3,
    };
    let room = height.saturating_sub(2 + 8 + 3);
    wanted.min(room.max(3))
}

/// The transcript pane: the buffer's own title line, the drop notice, and the items that fit.
fn draw_transcript(frame: &mut Frame, area: Rect, app: &App) {
    let inner = area.width.saturating_sub(2) as usize;
    let title = buffer_line(&app.conversation);
    let lines = transcript_lines(
        &app.conversation,
        inner,
        area.height.saturating_sub(2) as usize,
    );
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(title, style::section())),
            )
            .style(Style::default()),
        area,
    );
}

/// What the transcript's own title says: what this buffer is, and how much of it is corroborated.
///
/// The counts are the doctrine on one line. `confirmed` is only ever raised by finding an item's uuid
/// in the session's own transcript; `live` is everything the run said that nothing has corroborated
/// yet; and `own` is what this console wrote, which is not a claim about the run at all and so cannot
/// be confirmed or unconfirmed.
fn buffer_line(chat: &Conversation) -> String {
    let session = match &chat.session {
        Some(id) => format!(
            ", {} confirmed against {}",
            chat.confirmed(),
            id.chars().take(8).collect::<String>()
        ),
        None => format!(", {} confirmed", chat.confirmed()),
    };
    let model = match &chat.model {
        Some(model) => format!(" -- {model}"),
        None => String::new(),
    };
    format!(
        " TRANSCRIPT -- this console's own buffer, not a reading: {} item(s){session}, {} live, {} \
         this console's own{model} ",
        chat.items.len(),
        chat.live(),
        chat.own()
    )
}

/// The rows the transcript draws: the drop notice, then as many items as fit from the bottom up.
fn transcript_lines(chat: &Conversation, width: usize, height: usize) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }
    let mut head: Vec<Line<'static>> = Vec::new();
    if chat.dropped > 0 {
        // A truncated pane reads as "there is nothing more", which is a lie the operator cannot
        // detect; this is the row that makes the drop visible instead of silent.
        head.push(Line::from(Span::styled(
            format!(
                "… {} earlier item(s) dropped (the {}-item ring)",
                chat.dropped, chat.max_items
            ),
            style::warning(),
        )));
    }
    let room = height.saturating_sub(head.len());
    let all: Vec<&Item> = chat.items.iter().collect();
    let streaming = chat.turns.last().is_some_and(|turn| turn.ended.is_none());
    // Walk from the newest item backwards until the room runs out, then draw the run forwards. The
    // newest item is the one the operator is watching, so it is the one that must never be the row
    // that got cut.
    let mut chosen: Vec<Vec<Line<'static>>> = Vec::new();
    let mut used = 0usize;
    for (index, item) in all.iter().enumerate().rev() {
        let last = index + 1 == all.len();
        let rows = item_lines(item, width, streaming && last);
        if used + rows.len() > room && !chosen.is_empty() {
            break;
        }
        used += rows.len();
        chosen.push(rows);
        if used >= room {
            break;
        }
    }
    chosen.reverse();
    head.extend(chosen.into_iter().flatten());
    head.truncate(height);
    head
}

/// The rows one item occupies: its kind and state in the gutter, its text wrapped beside it.
fn item_lines(item: &Item, width: usize, cursor: bool) -> Vec<Line<'static>> {
    let body_style = match (item.kind, item.state) {
        (ItemKind::You, _) => style::you(),
        (ItemKind::Agent, _) => style::agent(),
        (ItemKind::Tool, _) => style::tool(),
        (ItemKind::Result, ItemState::Failed) => style::failure(),
        (ItemKind::Result, _) => style::result(),
        (ItemKind::Console, _) => style::console(),
        (ItemKind::Refused, _) => style::refused(),
    };
    let marker = match item.state {
        ItemState::Own => " ",
        ItemState::Live => "·",
        ItemState::Confirmed => "✓",
        ItemState::Failed => "!",
    };
    let label = match item.kind {
        ItemKind::You => "you",
        ItemKind::Agent => "orchestrator",
        ItemKind::Tool => "tool",
        ItemKind::Result => "result",
        ItemKind::Console => "console",
        ItemKind::Refused => "refused",
    };
    let marker_style = match item.state {
        ItemState::Failed => style::failure(),
        ItemState::Live => style::warning(),
        ItemState::Confirmed => style::section(),
        ItemState::Own => style::dim(),
    };
    let gutter = format!("{marker} {label:<13}");
    // A tool's result is documented as its first line only: one tool output can be thousands of
    // rows, and a transcript that pastes them is a transcript with one item in it.
    let text = match item.kind {
        ItemKind::Result => item.text.lines().next().unwrap_or_default().to_string(),
        _ => item.text.clone(),
    };
    let body_width = width.saturating_sub(GUTTER).max(1);
    let wrapped = match item.kind {
        ItemKind::Result => vec![clip(&text, body_width)],
        _ => wrap(&text, body_width),
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (index, row) in wrapped.iter().enumerate() {
        let (gutter, gutter_style) = if index == 0 {
            (gutter.clone(), marker_style)
        } else {
            (INDENT.to_string(), style::dim())
        };
        let mut spans = vec![
            Span::styled(gutter, gutter_style),
            Span::styled(row.clone(), body_style),
        ];
        // The cursor rides the last row of the newest item while a turn is open: it is the console
        // saying "the run is writing here", which is a claim it can always support -- the item is
        // `Live` or it would not be the newest.
        if cursor && index + 1 == wrapped.len() {
            spans.push(Span::styled("▌", style::streaming()));
        }
        lines.push(Line::from(spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(gutter, marker_style)));
    }
    lines
}

/// The composer box: the line being written, or the confirmation of the command it becomes.
fn draw_composer(frame: &mut Frame, area: Rect, app: &App) {
    let lines = match &app.mode {
        Mode::PromptConfirm { command } => confirm_lines(app, command),
        _ => composer_line(app),
    };
    let title = match &app.mode {
        Mode::PromptConfirm { .. } => {
            " CONFIRM -- y (or Enter) runs it, n (or Esc) cancels ".to_string()
        }
        Mode::Composer { .. } => {
            " COMPOSER -- Enter reviews the exact command, Esc closes it, Ctrl-U clears "
                .to_string()
        }
        _ => format!(
            " COMPOSER -- {} ",
            if app.composer.is_empty() {
                "press i to write a prompt"
            } else {
                "press i to continue this prompt"
            }
        ),
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, style::section())),
        ),
        area,
    );
}

/// The composer's own line, with the caret where the next character lands.
fn composer_line(app: &App) -> Vec<Line<'static>> {
    let budget = 240usize;
    let (before, at, after, caret) = match &app.mode {
        Mode::Composer { cursor } => {
            let chars: Vec<char> = app.composer.chars().collect();
            let cursor = (*cursor).min(chars.len());
            (
                chars[..cursor].iter().collect::<String>(),
                chars.get(cursor).copied(),
                chars[cursor.saturating_add(1).min(chars.len())..]
                    .iter()
                    .collect::<String>(),
                true,
            )
        }
        _ => (app.composer.clone(), None, String::new(), false),
    };
    // The tail, not the head: what the operator is typing now is what must be on screen. The same
    // rule the ruling box has always used for a long text.
    let shown: String = if before.chars().count() + 1 > budget {
        before
            .chars()
            .skip(before.chars().count() + 1 - budget)
            .collect()
    } else {
        before.clone()
    };
    let mut spans = vec![
        Span::styled("> ", style::warning()),
        Span::styled(shown, style::you()),
    ];
    if caret {
        let at = at.unwrap_or(' ');
        spans.push(Span::styled(
            at.to_string(),
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(ratatui::style::Color::Cyan),
        ));
        spans.push(Span::styled(after, style::you()));
    }
    vec![
        Line::from(spans),
        Line::from(Span::styled(
            if app.composer.is_empty() && !caret {
                "nothing written yet: what you type here becomes the next turn of this conversation"
            } else {
                "this text is the whole prompt; it is handed to the CLI as one argv element, never \
                 through a shell"
            },
            style::dim(),
        )),
    ]
}

/// The four things a prompt's confirmation shows, in the composer box: the exact argv, the guards,
/// the notes, and the question. The same four the modal shows for every other action.
fn confirm_lines(app: &App, command: &crate::actions::Command) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("about to run: ", style::dim()),
            Span::styled(command.kind.title().to_string(), style::section()),
        ]),
        Line::from(vec![
            Span::styled("command : ", style::warning()),
            Span::styled(command.display(), style::warning()),
        ]),
        Line::from(Span::styled(
            format!("guards  : {}", command.guards.summary()),
            style::dim(),
        )),
    ];
    for note in &command.note {
        lines.push(Line::from(Span::styled(
            format!("note    : {note}"),
            style::dim(),
        )));
    }
    // The operator's own words stay on screen while they are being confirmed: a prompt is a
    // sentence, and confirming a sentence you cannot see is how a wrong one gets sent.
    lines.push(Line::from(vec![
        Span::styled("prompt  : ", style::dim()),
        Span::styled(app.composer.clone(), style::you()),
    ]));
    lines.push(Line::from(Span::styled(
        "press y (or Enter) to run it, n (or Esc) to go back to the composer",
        style::warning(),
    )));
    lines
}

/// Wrap text at `width` columns, breaking between words.
///
/// A word longer than the pane is clipped with an ellipsis rather than broken: this pane carries tool
/// calls whose arguments are exactly the tokens an operator copies, and a token cut in half reads as
/// a different token. It is `style::clip_row`'s rule, applied to one word instead of one row.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        let size = word.chars().count();
        if row.is_empty() {
            if size <= width {
                row.push_str(word);
            } else {
                out.push(clip(word, width));
            }
            continue;
        }
        if row.chars().count() + 1 + size <= width {
            row.push(' ');
            row.push_str(word);
        } else {
            out.push(std::mem::take(&mut row));
            if size <= width {
                row.push_str(word);
            } else {
                out.push(clip(word, width));
            }
        }
    }
    if !row.is_empty() {
        out.push(row);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Clip one word (or one row) to `width` columns with an ellipsis when anything was cut.
fn clip(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    let room = width.saturating_sub(1);
    let head: String = text.chars().take(room).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrapped_row_never_exceeds_the_width_it_was_given() {
        let text = "the quick brown fox jumps over the lazy dog";
        for width in [1usize, 7, 20, 65] {
            let rows = wrap(text, width);
            assert!(!rows.is_empty());
            for row in &rows {
                assert!(
                    row.chars().count() <= width,
                    "row {row:?} is wider than {width}"
                );
            }
        }
        // Where the words fit, none is dropped: wrapping breaks between words, never inside one.
        for width in [7usize, 20, 65] {
            let joined = wrap(text, width).join(" ");
            for word in text.split_whitespace() {
                assert!(
                    joined.contains(word),
                    "{word} was dropped at {width} columns: {joined:?}"
                );
            }
        }
    }

    #[test]
    fn a_word_wider_than_the_pane_is_clipped_rather_than_broken() {
        let rows = wrap("toolu_01AAAAAAAAAAAAAAAAAAAAAAAAAAAA", 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].chars().count(), 10);
        assert!(rows[0].ends_with('…'));
    }

    #[test]
    fn a_wrapped_item_keeps_its_gutter_and_indents_its_continuation() {
        let item = Item {
            key: None,
            uuid: None,
            kind: ItemKind::Agent,
            state: ItemState::Live,
            text: "one two three four five six seven eight nine ten eleven twelve".to_string(),
            at: std::time::SystemTime::now(),
        };
        let lines = item_lines(&item, 40, false);
        assert!(lines.len() > 1, "the item wrapped");
        let first: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.to_string())
            .collect();
        let second: String = lines[1]
            .spans
            .iter()
            .map(|span| span.content.to_string())
            .collect();
        assert!(first.starts_with("· orchestrator"), "{first:?}");
        assert!(second.starts_with(INDENT), "{second:?}");
        for line in &lines {
            let width: usize = line
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum();
            assert!(width <= 40, "an item row ran past the pane: {width}");
        }
    }

    #[test]
    fn the_buffer_line_counts_the_three_classes_separately() {
        let mut chat = Conversation::new(400);
        chat.session = Some("22222222-3333-4444-8555-666666666666".to_string());
        chat.note("the console ran: docker exec …");
        let line = buffer_line(&chat);
        assert!(line.contains("1 item(s)"), "{line}");
        assert!(line.contains("0 confirmed against 22222222"), "{line}");
        assert!(line.contains("0 live"), "{line}");
        assert!(line.contains("1 this console's own"), "{line}");
    }

    #[test]
    fn a_dropped_item_is_announced_rather_than_clipped() {
        let mut chat = Conversation::new(2);
        for index in 0..5 {
            chat.note(format!("line {index}"));
        }
        assert_eq!(chat.dropped, 3);
        let lines = transcript_lines(&chat, 60, 10);
        let first: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.to_string())
            .collect();
        assert!(first.contains("3 earlier item(s) dropped"), "{first:?}");
        assert!(first.contains("the 2-item ring"), "{first:?}");
    }

    #[test]
    fn the_newest_item_is_the_one_that_survives_a_short_pane() {
        let mut chat = Conversation::new(400);
        chat.note("the oldest line in this buffer");
        for index in 0..40 {
            chat.note(format!("line {index}"));
        }
        let lines = transcript_lines(&chat, 40, 5);
        assert_eq!(lines.len(), 5);
        let last: String = lines[4]
            .spans
            .iter()
            .map(|span| span.content.to_string())
            .collect();
        assert!(
            last.contains("line 39"),
            "the newest item is what is on screen: {last:?}"
        );
    }
}
