//! The Environment page's own part: the variables niri sets for the
//! programs it starts.

use iced::widget::{column, container, row, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::forms;
use niri_center::settings;
use niri_center::store::Block;

use super::pad;
use niri_center::store::Store;

use crate::app::{App, error};
use crate::controls::{self, small};
use crate::message::Message as AppMessage;
use crate::rows;
use crate::{El, theme};

/// The variable being added.
#[derive(Debug, Default)]
pub struct State {
    pub name: String,
    pub value: String,
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    /// A variable's new value.
    Value(String, String),
    Remove(String),
    NewName(String),
    NewValue(String),
    Add,
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Value(name, value) => {
            if let Some(store) = &mut state.session.store
                && let Err(e) = store.set_environment(&name, Some(&value))
            {
                state.status = error(e.to_string());
            }
        }
        Message::Remove(name) => {
            if let Some(store) = &mut state.session.store
                && let Err(e) = store.remove_environment(&name)
            {
                state.status = error(e.to_string());
            }
        }
        Message::NewName(name) => state.environment.name = name,
        Message::NewValue(value) => state.environment.value = value,
        Message::Add => {
            let name = state.environment.name.trim().to_string();
            if let Some(problem) = forms::environment_name_problem(&name) {
                state.status = error(problem);
                return Task::none();
            }
            if let Some(store) = &mut state.session.store {
                match store.set_environment(&name, Some(&state.environment.value)) {
                    Ok(()) => state.environment = State::default(),
                    Err(e) => state.status = error(e.to_string()),
                }
            }
        }
    }
    Task::none()
}

/// The variables niri sets for the programs it starts.
pub fn view<'a>(state: &'a App, store: &'a Store) -> El<'a> {
    let mut rows: Vec<El<'_>> = Vec::new();
    for (name, value) in store.environment() {
        let path = settings::line_link(store, &[Block::named("environment")], &name);
        let typed = name.clone();
        let field = text_input(
            if value.is_none() {
                "Taken away (null)"
            } else {
                "Value"
            },
            value.as_deref().unwrap_or_default(),
        )
        .on_input(move |input| AppMessage::Environment(Message::Value(typed.clone(), input)))
        .font(theme::MONO)
        .width(Length::Fill);
        rows.push(pad(row![
            column![
                text(name.clone()).size(14).font(theme::MONO),
                text(path).size(12).font(theme::MONO).style(theme::muted),
            ]
            .spacing(2)
            .width(Length::FillPortion(2)),
            container(field).width(Length::FillPortion(3)),
            controls::remove_button(
                format!("Remove {name}"),
                AppMessage::Environment(Message::Remove(name.clone()))
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()));
    }
    rows.push(pad(row![
        text_input("NAME", &state.environment.name)
            .on_input(|name| AppMessage::Environment(Message::NewName(name)))
            .font(theme::MONO)
            .width(Length::FillPortion(2)),
        text_input("value", &state.environment.value)
            .on_input(|value| AppMessage::Environment(Message::NewValue(value)))
            .on_submit(AppMessage::Environment(Message::Add))
            .font(theme::MONO)
            .width(Length::FillPortion(3)),
        small("Add", AppMessage::Environment(Message::Add)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()));
    column![
        text("Environment variables").size(14).font(rows::SEMIBOLD),
        text("niri sets these for every program it starts. Programs that are already running keep what they had.")
            .size(12)
            .style(theme::muted),
        rows::group(String::new(), rows),
    ]
    .spacing(6)
    .into()
}
