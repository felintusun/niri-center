//! The History page: every version of the config by day, and what each
//! change did, with the way back to it. The history itself is
//! `niri_center::session`'s, and its words `niri_center::summary`'s.

use iced::widget::{Column, button, column, container, row, space, text};
use iced::{Length, Task};
use niri_center::diff::{self, Line};
use niri_center::disk::actor::{Command, Purpose};
use niri_center::history::{Entry, Source};
use niri_center::session::Version;
use niri_center::summary::{Today, clock, day, details, files_line, moment, title};

use crate::app::App;
use crate::highlight::{self, Mark};
use crate::message::Message as AppMessage;
use crate::rows::{self, SEMIBOLD};
use crate::{El, theme};
use niri_center::page::Page;

/// The version shown, and the diffs worked out for it.
#[derive(Debug, Default)]
pub struct State {
    /// The version open; `None` shows the latest, or the list in a narrow
    /// column.
    pub selected: Option<Version>,
    /// What the version shown changed, by the time of its entry.
    pub diff: Option<(u128, Vec<diff::FileDiff>)>,
    /// What restoring the start of the history would change.
    pub start_preview: Option<Vec<diff::FileDiff>>,
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    /// Open a version (`None`: back to the list).
    Pick(Option<Version>),
    /// Write the config back as it was at a version.
    Restore(Version),
    /// Copy a change as a diff.
    Copy(u128),
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Pick(version) => state.history_view.selected = version,
        Message::Restore(version) => {
            state.session.restore_version(version);
            // the list shows the new current version, not the old one again
            state.history_view.selected = None;
        }
        Message::Copy(time) => state.session.ask(Command::Texts {
            time,
            purpose: Purpose::Copy,
        }),
    }
    Task::none()
}

/// The History page shows one version: ask for its texts when they are not
/// the ones at hand.
pub fn want_texts(state: &mut App) {
    if state.page != Page::History {
        return;
    }
    let history = &state.session.history;
    let shown = state
        .history_view
        .selected
        .or_else(|| history.last().map(|entry| Version::After(entry.time)));
    match shown {
        Some(Version::After(time))
            if state.history_view.diff.as_ref().map(|(at, _)| *at) != Some(time) =>
        {
            state.history_view.diff = Some((time, Vec::new()));
            state.session.ask(Command::Texts {
                time,
                purpose: Purpose::Show,
            });
        }
        Some(Version::Start) if state.history_view.start_preview.is_none() => {
            if let Some(oldest) = history.first() {
                state.history_view.start_preview = Some(Vec::new());
                state.session.ask(Command::Before {
                    time: oldest.time,
                    purpose: Purpose::Preview,
                });
            }
        }
        Some(Version::After(_) | Version::Start) | None => {}
    }
}

/// History: versions of the config by day on the left, the chosen one on
/// the right. A narrow column shows one at a time, and Esc goes back to
/// the list.
pub fn page(state: &App) -> El<'_> {
    let history = &state.session.history;
    if history.is_empty() {
        return text(
            "No changes yet. Every change you make here, and every edit another program makes to your config, shows up in this list.",
        )
        .size(14)
        .style(theme::muted)
        .into();
    }
    let selected = state.history_view.selected.filter(|version| match version {
        Version::Start => true,
        Version::After(time) => history.iter().any(|e| e.time == *time),
    });
    // the day it is, for every heading and time on the page
    let today = niri_center::summary::today();
    if state.narrow() {
        return match selected {
            Some(version) => column![
                button(text("All versions").size(13))
                    .style(theme::link)
                    .padding([4, 0])
                    .on_press(AppMessage::History(Message::Pick(None))),
                detail(state, version, today),
            ]
            .spacing(8)
            .into(),
            None => list(state, None, today).into(),
        };
    }
    let selected = selected.or_else(|| history.last().map(|e| Version::After(e.time)));
    row![
        container(list(state, selected, today)).width(380),
        selected.map_or_else(|| space().into(), |version| detail(state, version, today)),
    ]
    .spacing(28)
    .into()
}

/// One version in the list: its time, its name, a muted line under it.
fn list_item<'a>(
    time: String,
    name: String,
    under: String,
    current: bool,
    chosen: bool,
    version: Version,
) -> El<'a> {
    let mut name_line = row![text(name).size(14).width(Length::Fill)].spacing(8);
    if current {
        name_line = name_line.push(text("Current").size(12).style(theme::accent));
    }
    button(
        row![
            text(time).size(12).style(theme::muted).width(44),
            column![name_line, text(under).size(12).style(theme::muted)]
                .spacing(2)
                .width(Length::Fill),
        ]
        .spacing(10),
    )
    .padding([8, 12])
    .width(Length::Fill)
    .style(theme::history_item(chosen))
    .on_press(AppMessage::History(Message::Pick(Some(version))))
    .into()
}

fn list(state: &App, selected: Option<Version>, today: Today) -> Column<'_, AppMessage> {
    let history = &state.session.history;
    let mut days: Vec<(String, Vec<El<'_>>)> = Vec::new();
    let newest = history.last().map(|e| e.time);
    for entry in history.iter().rev() {
        let version = Version::After(entry.time);
        let item = list_item(
            clock(entry.time),
            title(entry),
            files_line(entry, |path| state.session.file_label(path)),
            newest == Some(entry.time),
            selected == Some(version),
            version,
        );
        let heading = day(entry.time, today);
        match days.last_mut() {
            Some((last, items)) if *last == heading => items.push(item),
            _ => days.push((heading, vec![item])),
        }
    }
    if let (Some(oldest), Some((_, items))) = (history.first(), days.last_mut()) {
        items.push(list_item(
            String::new(),
            "Before these changes".to_string(),
            format!(
                "Your config as it was before {}",
                moment(oldest.time, today)
            ),
            false,
            selected == Some(Version::Start),
            Version::Start,
        ));
    }
    let mut list = column![].spacing(16).width(Length::Fill);
    for (heading, items) in days {
        list = list.push(rows::group(heading, items));
    }
    list
}

fn detail(state: &App, version: Version, today: Today) -> El<'_> {
    match version {
        Version::Start => start_detail(state, today),
        Version::After(time) => state
            .session
            .history
            .iter()
            .find(|e| e.time == time)
            .map_or_else(
                || space().into(),
                |entry| change_detail(state, entry, today),
            ),
    }
}

/// A version after a change: what the change did, and restoring it.
fn change_detail<'a>(state: &'a App, entry: &'a Entry, today: Today) -> El<'a> {
    let when = format!(
        "{} at {}, {}",
        day(entry.time, today),
        clock(entry.time),
        entry
            .paths
            .iter()
            .map(|path| state.session.file_label(path))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut body = column![
        column![
            text(title(entry)).size(20).font(SEMIBOLD),
            text(when).size(12).style(theme::muted),
        ]
        .spacing(2),
    ]
    .spacing(14)
    .width(Length::Fill);
    let details = details(entry);
    if details.len() > 1 || entry.source == Source::Outside {
        body = body.push(rows::group(
            String::new(),
            details
                .iter()
                .map(|line| {
                    container(text(line.clone()).size(14))
                        .padding([8, 12])
                        .into()
                })
                .collect(),
        ));
    }
    let colors = state.colors();
    let files = state
        .history_view
        .diff
        .as_ref()
        .filter(|(time, _)| *time == entry.time)
        .map_or(&[][..], |(_, files)| files.as_slice());
    for file in files {
        if files.len() > 1 {
            body = body.push(text(file.path.as_str()).size(13).font(theme::MONO));
        }
        body = body.push(diff_view(&file.hunks, colors));
    }
    let current = state
        .session
        .history
        .last()
        .is_some_and(|last| last.time == entry.time);
    let copy = button(text("Copy as a diff").size(13))
        .style(theme::secondary)
        .on_press(AppMessage::History(Message::Copy(entry.time)));
    if current {
        body = body.push(copy);
    } else {
        body = body.push(
            row![
                button(text("Restore this version").size(13))
                    .style(theme::primary)
                    .on_press(AppMessage::History(Message::Restore(Version::After(
                        entry.time
                    )))),
                copy,
            ]
            .spacing(8),
        );
    }
    container(body).width(Length::Fill).into()
}

/// The start of the history: what restoring it would change now.
fn start_detail(state: &App, today: Today) -> El<'_> {
    let Some(oldest) = state.session.history.first() else {
        return space().into();
    };
    let mut body = column![
        column![
            text("Before these changes").size(20).font(SEMIBOLD),
            text(format!(
                "Your config as it was before {}",
                moment(oldest.time, today)
            ))
            .size(12)
            .style(theme::muted),
        ]
        .spacing(2),
        text("Restoring it changes this:").size(13),
    ]
    .spacing(14)
    .width(Length::Fill);
    let colors = state.colors();
    // each file as the first change found it, against the file now
    for file in state.history_view.start_preview.iter().flatten() {
        body = body.push(text(file.path.as_str()).size(13).font(theme::MONO));
        body = body.push(diff_view(&file.hunks, colors));
    }
    body = body.push(
        button(text("Restore this version").size(13))
            .style(theme::primary)
            .on_press(AppMessage::History(Message::Restore(Version::Start))),
    );
    container(body).width(Length::Fill).into()
}

/// A diff with line numbers, signs and colored KDL.
pub fn diff_view<'a>(hunks: &[diff::Hunk], colors: &theme::Colors) -> El<'a> {
    let mut lines = column![].width(Length::Fill);
    for (index, hunk) in hunks.iter().enumerate() {
        if index > 0 {
            lines = lines.push(space().height(10));
        }
        for (old, new, line) in hunk.numbered() {
            let mark = match line {
                Line::Context(_) => Mark::Plain,
                Line::Removed(_) => Mark::Removed,
                Line::Added(_) => Mark::Added,
            };
            lines = lines.push(highlight::numbered(
                new.or(old).unwrap_or_default(),
                line.text(),
                mark,
                Some(line.prefix()),
                colors,
            ));
        }
    }
    highlight::lines_block(lines)
}
