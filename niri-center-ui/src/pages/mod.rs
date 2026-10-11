//! The pages: each page's module draws it, and this one picks the page
//! and gives every page its heading.

pub mod animations;
pub mod displays;
pub mod environment;
pub mod files;
pub mod keybinds;
pub mod rules;
pub mod startup;
mod windows;

use iced::widget::{Column, Row, column, container, row, text};
use iced::{Alignment, Length};
use niri_center::page::Page;

use crate::app::App;
use crate::controls::{self, small};
use crate::message::Message;
use crate::rows;
use crate::{El, theme};

/// The current page: title, one-line description, content.
pub fn page(state: &App) -> Column<'_, Message> {
    let body: El<'_> = match (state.page, state.store()) {
        (Page::Windows, _) => windows::windows(state),
        (Page::Events, _) => windows::events(state),
        (_, None) => no_config(state),
        (Page::Keybinds, Some(store)) => keybinds::page(state, store),
        (Page::Rules, Some(_)) => rules::page(state),
        (Page::Startup, Some(store)) => startup::page(state, store),
        (Page::Displays, Some(_)) => displays::page(state),
        (Page::Environment, Some(store)) => column![
            environment::view(state, store),
            rows::sections(state, state.page)
        ]
        .spacing(16)
        .into(),
        (
            Page::Layout
            | Page::Workspaces
            | Page::Overview
            | Page::Input
            | Page::Gestures
            | Page::Cursor
            | Page::Screenshots,
            Some(_),
        ) => rows::sections(state, state.page),
        (Page::Animations, Some(store)) => animations::page(state, store),
        (Page::History, Some(_)) => crate::history::page(state),
        (Page::Files, Some(store)) => files::page(state, store),
        (Page::Source, Some(_)) => crate::source::whole(state),
    };
    column![heading(state), body]
        .spacing(16)
        .width(Length::Fill)
        .height(if state.page_scrolls_itself() {
            Length::Fill
        } else {
            Length::Shrink
        })
}

/// What the page is, in a title and a sentence, and the button that shows
/// the config beside it: in the same place on every page.
fn heading(state: &App) -> El<'_> {
    let mut line = row![
        text(state.page.label())
            .size(22)
            .font(rows::SEMIBOLD)
            .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    if state.shows_source() {
        let open = state.source_shown;
        line = line.push(iced::widget::tooltip(
            small(
                if open {
                    "Hide config.kdl"
                } else {
                    "Show config.kdl"
                },
                Message::ShowSource(!open),
            ),
            controls::tip("Ctrl+."),
            iced::widget::tooltip::Position::Left,
        ));
    }
    line.into()
}

/// One choice of a few, side by side in one outlined strip: the chosen one
/// filled.
fn segments<'a>(choices: Row<'a, Message>) -> El<'a> {
    container(choices)
        .padding(1)
        .clip(true)
        .style(theme::segments)
        .into()
}

fn no_config(state: &App) -> El<'_> {
    let mut col = column![text("No config loaded yet.").size(14)].spacing(6);
    if let Some(e) = &state.load_error {
        col = col.push(text(e.clone()).size(13).style(theme::error));
    }
    col.into()
}

/// Content of one row inside a group.
fn pad(content: El<'_>) -> El<'_> {
    container(content).padding([9, 14]).into()
}

/// A muted tag such as "Off" or "keybinds.kdl".
fn tag<'a>(label: impl text::IntoFragment<'a>) -> El<'a> {
    text(label).size(12).style(theme::muted).into()
}

fn include_warnings(state: &App) -> Option<El<'_>> {
    let store = state.session.store.as_ref()?;
    let warnings = store.include_warnings();
    if warnings.is_empty() {
        return None;
    }
    let mut col = column![].spacing(4);
    for warning in warnings {
        col = col.push(text(warning.clone()).size(12).style(theme::pending));
    }
    Some(col.into())
}

/// The button that turns an entry off, or back on when it is off.
const fn on_off_label(off: bool) -> &'static str {
    if off { "Turn on" } else { "Turn off" }
}
