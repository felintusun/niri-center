//! The Environment page's own part: the variables niri sets for the
//! programs it starts.

use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::forms;
use niri_center::settings;
use niri_center::store::Block;

use super::pad;
use niri_center::store::Store;

use crate::app::{App, error};
use crate::controls;
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
                && let Err(failure) = store.set_environment(&name, Some(&value))
            {
                state.status = error(failure.to_string());
            }
        }
        Message::Remove(name) => {
            if let Some(store) = &mut state.session.store
                && let Err(failure) = store.remove_environment(&name)
            {
                state.status = error(failure.to_string());
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
                    Err(failure) => state.status = error(failure.to_string()),
                }
            }
        }
    }
    Task::none()
}

/// The variables niri sets for the programs it starts.
pub fn view(state: &App, store: &Store) -> El<'static> {
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
    // a name is checked as it is typed, under the field, and Add waits
    // for one niri takes
    let name = state.environment.name.trim();
    let problem = (!name.is_empty())
        .then(|| forms::environment_name_problem(name))
        .flatten();
    let ready = !name.is_empty() && problem.is_none();
    let mut value = text_input("value", &state.environment.value)
        .on_input(|value| AppMessage::Environment(Message::NewValue(value)))
        .font(theme::MONO)
        .width(Length::FillPortion(3));
    let mut add = button(text("Add").size(12))
        .padding([3, 10])
        .style(theme::secondary);
    if ready {
        value = value.on_submit(AppMessage::Environment(Message::Add));
        add = add.on_press(AppMessage::Environment(Message::Add));
    }
    let mut adding = column![
        row![
            text_input("NAME", &state.environment.name)
                .on_input(|name| AppMessage::Environment(Message::NewName(name)))
                .font(theme::MONO)
                .width(Length::FillPortion(2)),
            value,
            add,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    ]
    .spacing(4);
    if let Some(problem) = problem {
        adding = adding.push(text(problem).size(12).style(theme::error));
    }
    rows.push(pad(adding.into()));
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
