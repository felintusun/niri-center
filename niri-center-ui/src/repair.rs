//! When niri cannot read the config after an edit somewhere else: what
//! niri says, where, a fix when the cause is a misspelled name, and the way
//! back to the editor the edit came from.
//!
//! niri keeps running the last config that loaded, so nothing is broken
//! yet; saving here waits until the file reads again.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Length};
use niri_center::typo::Typo;

use crate::rows::SEMIBOLD;

use crate::app::App;
use crate::message::Message;
use crate::{El, highlight, theme};

/// The panel at the top of the page while niri cannot read the config.
pub fn panel(state: &App) -> Option<El<'_>> {
    let health = state.session.health()?;
    let (details, typo) = (&health.details, &health.typo);
    let file = state.session.store.as_ref().map_or_else(
        || "the config".to_string(),
        |store| store.file_label(store.path()),
    );
    let mut body = column![
        text(format!("niri can't read {file}"))
            .size(20)
            .font(SEMIBOLD),
    ]
    .spacing(10)
    .width(Length::Fill);
    body = body.push(text(health.sentence()).size(14));
    if let Some(details) = details {
        body = body.push(
            column![
                text("What niri validate says").size(12).style(theme::muted),
                crate::controls::diagnostics(details),
            ]
            .spacing(4),
        );
    }
    body = body.push(
        text("niri still runs the last config that worked. Saving here waits until the file reads again.")
            .size(13)
            .style(theme::muted),
    );
    let mut actions = row![].spacing(8).align_y(Alignment::Center);
    if let Some(Typo {
        path,
        line,
        name,
        meant: Some(meant),
    }) = &typo
    {
        actions = actions.push(
            button(text(format!("Change to {meant}")).size(13))
                .style(theme::primary)
                .on_press(Message::FixTypo {
                    path: path.clone(),
                    line: *line,
                    name: name.clone(),
                    meant: meant.clone(),
                }),
        );
    }
    let editor = state
        .running
        .as_ref()
        .zip(state.store())
        .and_then(|(running, store)| running.editor_of(store));
    if let Some(window) = editor {
        actions = actions.push(
            button(text("Go to your editor").size(13))
                .style(theme::secondary)
                .on_press(Message::FocusWindow(window.id)),
        );
    }
    if let Some(typo) = &typo {
        actions = actions.push(
            button(
                text(if state.broken_line_shown {
                    "Hide the line".to_string()
                } else {
                    format!("Show line {} here", typo.line)
                })
                .size(13),
            )
            .style(theme::secondary)
            .on_press(Message::ShowBrokenLine(!state.broken_line_shown)),
        );
    }
    actions = actions.push(
        button(text("Dismiss").size(13))
            .style(theme::secondary)
            .on_press(Message::DismissHealth),
    );
    body = body.push(actions);
    if let Some(window) = editor {
        body = body.push(
            text(format!(
                "Your editor: {}, window “{}”.",
                window.app_id, window.title
            ))
            .size(12)
            .style(theme::muted),
        );
    }
    if state.broken_line_shown
        && let Some(typo) = &typo
        && let Some(source) = state
            .session
            .store
            .as_ref()
            .and_then(|s| s.text_of(&typo.path))
    {
        let colors = state.colors();
        let mut lines = column![];
        let first = typo.line.saturating_sub(2).max(1);
        for (index, line) in source.lines().enumerate().skip(first - 1).take(5) {
            let number = index + 1;
            let mark = if number == typo.line {
                highlight::Mark::Removed
            } else {
                highlight::Mark::Plain
            };
            lines = lines.push(highlight::numbered(number, line, mark, None, colors));
        }
        body = body.push(highlight::lines_block(lines));
    }
    Some(
        container(body)
            .padding(16)
            .width(Length::Fill)
            .style(theme::problem)
            .into(),
    )
}
