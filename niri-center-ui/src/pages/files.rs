//! The Files page: the config's files, backups, snapshots and how saving
//! works.

use iced::widget::{button, column, row, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::disk::actor::Command;
use niri_center::session::SaveWhen;
use niri_center::settings;

use super::{pad, tag};
use niri_center::store::Store;

use crate::app::{App, error, info};
use crate::controls::small;
use crate::message::Message as AppMessage;
use crate::rows;
use crate::{El, theme};

/// The Files page's backups and snapshots: read from disk when the page
/// opens and after what changes them, not as it draws.
#[derive(Debug, Default)]
pub struct Listing {
    /// The latest backup of each file, with when it was made.
    pub backups: Vec<(String, u128)>,
    pub snapshots: Vec<String>,
}

#[derive(Debug, Default)]
pub struct State {
    pub listing: Listing,
    /// The name typed for a new snapshot.
    pub snapshot_name: String,
    /// What the sandbox allows, in a sentence.
    pub sandbox: Option<String>,
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    RestoreBackup(String),
    SnapshotName(String),
    CreateSnapshot,
    RestoreSnapshot(String),
    DeleteSnapshot(String),
    SaveWhen(SaveWhen),
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::RestoreBackup(name) => state.session.ask(Command::Backup { name }),
        Message::SnapshotName(name) => state.files.snapshot_name = name,
        Message::CreateSnapshot => {
            if state
                .desktop_only("Snapshots are saved in the desktop app. This demo writes no files.")
            {
                return Task::none();
            }
            if let Some(store) = state.store() {
                let name = state.files.snapshot_name.trim().to_string();
                if name.is_empty() {
                    state.status = error("Enter a name for the snapshot.");
                    return Task::none();
                }
                let files = store.snapshot_files();
                state.session.ask(Command::CreateSnapshot {
                    name: name.clone(),
                    files,
                });
                state.files.snapshot_name.clear();
                state.status = info(format!("Snapshot {name} saved."));
            }
        }
        Message::RestoreSnapshot(name) => state.session.ask(Command::Snapshot { name }),
        Message::DeleteSnapshot(name) => {
            state.status = info(format!("Snapshot {name} deleted."));
            state.session.ask(Command::DeleteSnapshot { name });
        }
        Message::SaveWhen(when) => {
            state.session.save_when = when;
            // kept for the next start; tests and the demo keep nothing
            if state.session.platform.writes_files()
                && let Err(e) = niri_center::start::set_save_when(when)
            {
                state.status = error(format!("The preference could not be kept: {e}"));
            }
            // back to saving as changes are made: what waits goes now
            state.session.rearm();
        }
    }
    Task::none()
}

pub fn page<'a>(state: &'a App, store: &'a Store) -> El<'a> {
    let mut file_rows = vec![pad(row![
        text(niri_center::store::tilde(store.path()))
            .size(13)
            .font(theme::MONO)
            .width(Length::Fill),
        tag("Main config"),
    ]
    .into())];
    for included in store.included_files() {
        let mut line = column![row![
            text(niri_center::store::tilde(&included.path))
                .size(13)
                .font(theme::MONO)
                .width(Length::Fill),
            tag("Included"),
        ]]
        .spacing(4);
        if let Some(owner) = settings::managed_by(store, &included.path) {
            line = line.push(
                text(format!(
                    "{owner} rewrites this file, so changes made here may not last. Change those settings in {owner} instead."
                ))
                .size(12)
                .style(theme::pending),
            );
        }
        file_rows.push(pad(line.into()));
    }
    if let Some(sandbox) = &state.files.sandbox {
        file_rows.push(pad(text(sandbox.clone())
            .size(12)
            .style(theme::muted)
            .into()));
    }
    let mut col = column![rows::group("Config files".to_string(), file_rows)].spacing(24);

    let backups = &state.files.listing.backups;
    let backup_rows: Vec<El<'_>> = if backups.is_empty() {
        vec![pad(text(
            "No backups yet. Every save keeps a copy of each file first.",
        )
        .size(13)
        .into())]
    } else {
        backups
            .iter()
            .map(|(name, nanos)| {
                let when = niri_center::summary::stamp(*nanos)
                    .map(|stamp| format!(", from {stamp}"))
                    .unwrap_or_default();
                pad(row![
                    text(format!("Latest backup of {name}{when}"))
                        .size(13)
                        .width(Length::Fill),
                    small(
                        "Restore",
                        AppMessage::Files(Message::RestoreBackup(name.clone()))
                    ),
                ]
                .align_y(Alignment::Center)
                .into())
            })
            .collect()
    };
    col = col.push(rows::group("Backups".to_string(), backup_rows));

    let mut snapshot_rows = vec![pad(column![
        row![
            text_input(
                "Name, for example before the new monitor",
                &state.files.snapshot_name
            )
            .on_input(move |value| AppMessage::Files(Message::SnapshotName(value)))
            .on_submit(AppMessage::Files(Message::CreateSnapshot))
            .width(Length::Fill),
            button(text("Save snapshot").size(13))
                .style(theme::secondary)
                .on_press(AppMessage::Files(Message::CreateSnapshot)),
        ]
        .spacing(10),
        text("A snapshot keeps every config file as it is now.")
            .size(12)
            .style(theme::muted),
    ]
    .spacing(6)
    .into())];
    for name in &state.files.listing.snapshots {
        snapshot_rows.push(pad(row![
            text(name.as_str()).size(13).width(Length::Fill),
            small(
                "Restore",
                AppMessage::Files(Message::RestoreSnapshot(name.clone()))
            ),
            small(
                "Delete",
                AppMessage::Files(Message::DeleteSnapshot(name.clone()))
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()));
    }
    col = col.push(rows::group("Snapshots".to_string(), snapshot_rows));
    col.push(rows::group(
        "Saving".to_string(),
        vec![pad(column![
            iced::widget::radio(
                "Save each change a moment after I make it",
                SaveWhen::AfterChange,
                Some(state.session.save_when),
                move |value| AppMessage::Files(Message::SaveWhen(value)),
            )
            .text_size(14),
            iced::widget::radio(
                "Save when I press Save or Ctrl+S, after a look at the diff",
                SaveWhen::OnRequest,
                Some(state.session.save_when),
                move |value| AppMessage::Files(Message::SaveWhen(value)),
            )
            .text_size(14),
        ]
        .spacing(8)
        .into())],
    ))
    .into()
}
