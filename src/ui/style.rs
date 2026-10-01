//! Colours and styles, in one place, so the three screens agree on what a light means.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::checkpoint::CardState;
use crate::state::{Light, SeamClass};

/// The colour of a status light.
pub fn light_color(light: Light) -> Color {
    match light {
        Light::Ok => Color::Green,
        Light::Warn => Color::Yellow,
        Light::Fail => Color::Red,
        Light::Missing => Color::Magenta,
        Light::Unknown => Color::DarkGray,
        Light::Human => Color::Cyan,
    }
}

/// The style of a status light.
pub fn light_style(light: Light) -> Style {
    Style::default().fg(light_color(light))
}

/// The style of a tab title.
pub fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

/// The style of a config-seam class.
pub fn seam_style(class: SeamClass) -> Style {
    match class {
        SeamClass::RepoSpecific => Style::default().fg(Color::Yellow),
        SeamClass::Generic => Style::default().fg(Color::Green),
    }
}

/// The word printed for a seam class.
pub fn seam_word(class: SeamClass) -> &'static str {
    match class {
        SeamClass::RepoSpecific => "still a Komun default",
        SeamClass::Generic => "changed for this repo / generic",
    }
}

/// The style of the checkpoint card's state.
pub fn card_style(state: CardState) -> Style {
    match state {
        // The stopped-at-a-checkpoint state is the one the operator must act on, so it carries the
        // filled band: "the run stopped here, the ruling resumes it".
        CardState::Stopped => Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        // A run in flight is work in progress, not a caution: plain cyan text, no yellow and no
        // filled "act now" band, because nothing is waiting on a decision.
        CardState::Running => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        CardState::Idle => Style::default().fg(Color::Gray),
    }
}

/// A dim style for provenance.
pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// A section header inside INSPECT.
pub fn section() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

/// A selected box.
pub fn selected() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::White)
        .add_modifier(Modifier::BOLD)
}

/// A failure.
pub fn failure() -> Style {
    Style::default().fg(Color::Red)
}

/// A warning.
pub fn warning() -> Style {
    Style::default().fg(Color::Yellow)
}

/// The operator's own words in the transcript.
pub fn you() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

/// The run's own words in the transcript.
pub fn agent() -> Style {
    Style::default()
}

/// A tool call the run made, as its own stream named it.
pub fn tool() -> Style {
    Style::default().fg(Color::Yellow)
}

/// What a tool returned.
pub fn result() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// A line this console wrote itself: an argv, a count, an envelope it does not read.
pub fn console() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// A refusal, in the console's own words.
pub fn refused() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

/// The streaming cursor: the run is writing this item now.
pub fn streaming() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

/// Clip a composed row to `width` columns, ending in an ellipsis when anything was cut.
///
/// One row stays one row. A row left free to wrap silently drops whatever falls past the panel's
/// height, and a dropped row reads as a fact that does not exist. That is how a check went missing
/// from the CHECKS panel on a machine whose paths were longer: the third row wrapped out of the
/// area and the panel looked like a panel with two checks.
///
/// The spans keep their own styles, so a clipped row still carries its light colour and its dim
/// source. The caller passes the width inside the block's borders.
pub fn clip_row<'a>(spans: Vec<Span<'a>>, width: usize) -> Vec<Span<'a>> {
    let total: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    if total <= width {
        return spans;
    }
    let mut out: Vec<Span<'a>> = Vec::new();
    let mut used = 0usize;
    for span in spans {
        let text = span.content.to_string();
        let len = text.chars().count();
        if used + len <= width {
            used += len;
            out.push(span);
            continue;
        }
        // One column goes to the ellipsis, so the result never exceeds the width it was given.
        let room = width.saturating_sub(used).saturating_sub(1);
        if room > 0 {
            let head: String = text.chars().take(room).collect();
            out.push(Span::styled(format!("{head}…"), span.style));
        }
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(spans: &[Span]) -> usize {
        spans.iter().map(|span| span.content.chars().count()).sum()
    }

    #[test]
    fn a_row_that_fits_is_untouched() {
        let row = vec![Span::raw("abc"), Span::raw("def")];
        let clipped = clip_row(row, 6);
        assert_eq!(width(&clipped), 6);
        assert_eq!(clipped[0].content, "abc");
        assert_eq!(clipped[1].content, "def");
    }

    #[test]
    fn an_overlong_row_is_one_row_with_an_ellipsis() {
        let row = vec![Span::raw("port-self-test"), Span::raw("abcdefghij")];
        let clipped = clip_row(row, 12);
        assert_eq!(width(&clipped), 12);
        assert!(clipped.last().unwrap().content.ends_with('…'));
    }

    #[test]
    fn an_exact_fit_is_not_clipped() {
        let row = vec![Span::raw("12345"), Span::raw("67890")];
        let clipped = clip_row(row, 10);
        assert_eq!(width(&clipped), 10);
        assert!(!clipped.last().unwrap().content.contains('…'));
    }
}
