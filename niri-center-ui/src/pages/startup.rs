//! The Startup page: programs niri starts when you log in.

use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::system::InstalledApp;

use super::{include_warnings, on_off_label, tag};
use niri_center::store::Store;

use crate::app::{App, error, info, outcome, turned};
use crate::controls::small;
use crate::message::Message as AppMessage;
use crate::rows;
use crate::{El, theme};

/// The program being added.
#[derive(Debug, Default)]
pub struct State {
    /// The command typed.
    pub command: String,
    /// The installed apps to pick from.
    pub app_box: iced::widget::combo_box::State<InstalledApp>,
    /// What the list of programs is narrowed to.
    pub search: String,
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    /// The command typed so far.
    New(String),
    Add,
    /// Add an installed app's command.
    AddApp(String),
    Remove(usize),
    On(usize, bool),
    Search(String),
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::New(command) => state.startup.command = command,
        Message::Add => {
            let Some(store) = &mut state.session.store else {
                return Task::none();
            };
            let command = state.startup.command.trim().to_string();
            if command.is_empty() {
                state.status = error("Enter a command.");
                return Task::none();
            }
            match store.add_autostart(&command) {
                Ok(()) => {
                    state.startup.command.clear();
                    state.status = info(format!("Added {command}."));
                }
                Err(e) => state.status = error(e.to_string()),
            }
        }
        Message::AddApp(command) => {
            state.startup.command = command;
            return update(state, Message::Add);
        }
        Message::Remove(index) => {
            if let Some(store) = &mut state.session.store {
                state.status = outcome(store.remove_autostart(index), "Entry removed.");
            }
        }
        Message::On(index, on) => {
            if let Some(store) = &mut state.session.store {
                state.status = outcome(
                    store.set_autostart_disabled(index, !on),
                    turned("Entry", on),
                );
            }
        }
        Message::Search(query) => state.startup.search = query,
    }
    Task::none()
}

pub fn page<'a>(state: &'a App, store: &'a Store) -> El<'a> {
    let rows = store.autostart_rows();
    let query = state.startup.search.trim().to_lowercase();
    let mut entries: Vec<El<'_>> = Vec::new();
    for (index, entry) in rows.iter().enumerate() {
        if !query.is_empty() && !entry.display.to_lowercase().contains(&query) {
            continue;
        }
        let command = text(entry.display.clone()).size(14).font(theme::MONO);
        let command = if entry.disabled {
            command.style(theme::muted)
        } else {
            command
        };
        let mut line = row![container(command).width(Length::Fill)]
            .spacing(10)
            .align_y(Alignment::Center);
        if entry.disabled {
            line = line.push(tag("Off"));
        }
        if entry.included {
            line = line.push(tag(entry.file.clone()));
        }

        line = line
            .push(small(
                on_off_label(entry.disabled),
                AppMessage::Startup(Message::On(index, entry.disabled)),
            ))
            .push(small("Remove", AppMessage::Startup(Message::Remove(index))));
        entries.push(container(line).padding([8, 14]).into());
    }
    let mut col = column![
        text_input("Search autostart", &state.startup.search)
            .on_input(|query| AppMessage::Startup(Message::Search(query)))
            .width(320),
    ]
    .spacing(18);
    if rows.is_empty() {
        col = col.push(text("niri starts nothing extra yet.").size(13));
    } else if entries.is_empty() {
        col = col.push(text("Nothing matches the search.").size(13));
    } else {
        col = col.push(rows::group("Programs".to_string(), entries));
    }
    if let Some(warnings) = include_warnings(state) {
        col = col.push(warnings);
    }
    col.push(rows::group(
        "Add a program".to_string(),
        vec![
            container(
                column![
                    iced::widget::combo_box(
                        &state.startup.app_box,
                        "Pick an installed app: type to search",
                        None,
                        |app: InstalledApp| AppMessage::Startup(Message::AddApp(app.command)),
                    )
                    .width(Length::Fill),
                    text("Or type a command:").size(12).style(theme::muted),
                    row![
                        text_input("A command, for example waybar", &state.startup.command)
                            .on_input(|command| AppMessage::Startup(Message::New(command)))
                            .on_submit(AppMessage::Startup(Message::Add))
                            .width(Length::Fill),
                        small(
                            "Browse",
                            AppMessage::Browse {
                                target: Box::new(AppMessage::Startup(Message::New(
                                    crate::controls::HOLE.to_string()
                                ))),
                                folder: false,
                            }
                        ),
                        button(text("Add").size(13))
                            .style(theme::primary)
                            .on_press(AppMessage::Startup(Message::Add)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    text("A command with arguments runs through the shell (spawn-sh-at-startup).")
                        .size(12)
                        .style(theme::muted),
                ]
                .spacing(6),
            )
            .padding([10, 14])
            .into(),
        ],
    ))
    .into()
}
