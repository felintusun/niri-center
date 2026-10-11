//! The Windows and Events pages: what niri runs right now, and what it
//! reports as it happens.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Length};

use super::tag;
use crate::app::App;
use crate::controls::small;
use crate::message::Message;
use crate::rows;
use crate::{El, theme};

pub fn windows(state: &App) -> El<'_> {
    let mut col = column![small("Refresh now", Message::RunningTick)].spacing(18);
    if let Some(e) = &state.running_error {
        col = col.push(
            text(format!("niri did not answer: {e}"))
                .size(13)
                .style(theme::error),
        );
    }
    let Some(data) = &state.running else {
        return col.push(text("Waiting for niri.").size(13)).into();
    };
    let lines = |lines: &[String]| -> Vec<El<'_>> {
        lines
            .iter()
            .map(|line| {
                container(text(line.clone()).size(13).font(theme::MONO))
                    .padding([6, 14])
                    .into()
            })
            .collect()
    };
    let window_rows = data
        .windows
        .iter()
        .map(|window| {
            let mut line = row![
                text(window.app_id.clone())
                    .size(13)
                    .font(theme::MONO)
                    .style(theme::code)
                    .width(240),
                text(window.title.clone()).size(13).width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            if window.focused {
                line = line.push(tag("Focused"));
            }
            if window.floating {
                line = line.push(tag("Floating"));
            }
            container(line.push(small(
                "Make a rule",
                Message::Rules(crate::pages::rules::Message::NewForWindow(
                    window.app_id.clone(),
                    window.title.clone(),
                )),
            )))
            .padding([6, 14])
            .into()
        })
        .collect();
    col = col
        .push(rows::group("Windows".to_string(), window_rows))
        .push(rows::group(
            "Outputs".to_string(),
            lines(
                &data
                    .outputs
                    .iter()
                    .map(niri_center::outputs::summary)
                    .collect::<Vec<_>>(),
            ),
        ));
    if !data.layers.is_empty() {
        col = col.push(rows::group(
            "Layer surfaces".to_string(),
            lines(&data.layers),
        ));
    }
    col.into()
}

// ---- events (live) ----

pub fn events(state: &App) -> El<'_> {
    let mut col = column![
        row![
            small("Clear", Message::ClearLive),
            text(format!("{} events", state.live_events.len()))
                .size(13)
                .style(theme::muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(14);
    if state.live_events.is_empty() {
        return col.push(text("No events yet.").size(13)).into();
    }
    let mut list = column![].spacing(4);
    for (kind, line) in &state.live_events {
        list = list.push(
            row![
                text(kind.word())
                    .size(12)
                    .font(theme::MONO)
                    .style(theme::accent)
                    .width(100),
                text(line.clone()).size(13).width(Length::Fill),
            ]
            .spacing(8),
        );
    }
    col = col.push(list);
    col.into()
}
