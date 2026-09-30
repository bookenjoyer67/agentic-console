//! Colours and styles, in one place, so the three screens agree on what a light means.

use ratatui::style::{Color, Modifier, Style};

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
