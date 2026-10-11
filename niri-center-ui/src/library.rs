//! The rule library, a tab of the Window rules page: the rules for apps
//! installed here, offered to mark and import, and suggested on an app's
//! page. The rules themselves are `niri_center::library`.

use iced::widget::{Column, button, checkbox, column, container, radio, row, text};
use iced::{Alignment, Length};
use niri_center::library::{self, FILE, LIBRARY};
use niri_center::words::count;

use crate::app::App;
use crate::message::Message;
use crate::pages::rules;
use crate::rows::{self, SEMIBOLD};
use crate::{El, controls, highlight, theme};

/// Rules the library has for a window's app that the config lacks,
/// offered beside it, each with Add. `None` when there are none.
pub fn suggestions<'a>(state: &'a App, app_id: &str) -> Option<El<'a>> {
    let found = niri_center::library::missing(in_config(state), app_id);
    let app = LIBRARY.get(*found.first()?)?.app?.name();
    let colors = state.colors();
    let rows: Vec<El<'a>> = found
        .into_iter()
        .map(|index| {
            let rule = &LIBRARY[index];
            let action: El<'a> = controls::small(
                "Add this rule",
                Message::Rules(rules::Message::AddLibraryRule(index)),
            )
            .into();
            column![
                row![text(rule.name).size(14).width(Length::Fill), action]
                    .spacing(8)
                    .align_y(Alignment::Center),
                text(rule.why).size(12).style(theme::muted),
                highlight::block(rule.kdl, colors, 12.0),
            ]
            .spacing(6)
            .padding([10, 12])
            .into()
        })
        .collect();
    Some(
        column![
            text(format!("Suggested for {app}")).size(14).font(SEMIBOLD),
            rows::group(String::new(), rows),
        ]
        .spacing(6)
        .into(),
    )
}

/// Which of the library's rules the config has active, by index.
pub fn in_config(state: &App) -> &[bool] {
    &state.rows.library_have
}

/// The library, in place of the app or rule open on the Rules page.
pub fn view(state: &App) -> El<'_> {
    let colors = state.colors();
    let apps = library::apps(&state.system, state.rules.library.all_apps);
    // an app not offered here any more shows every window's rules
    let picked = state
        .rules
        .library
        .app
        .filter(|app| apps.contains(&Some(*app)));
    let mut list = Vec::new();
    for &app in &apps {
        let rules: Vec<usize> = (0..LIBRARY.len())
            .filter(|&index| LIBRARY[index].app == app)
            .collect();
        let marked = rules
            .iter()
            .filter(|index| state.rules.library.marked.contains(index))
            .count();
        let mut count = count(rules.len(), "rule", "rules");
        if marked > 0 {
            count = format!("{count}, {marked} marked");
        }
        list.push(
            button(
                row![
                    text(app.map_or("Every window", |app| app.name()))
                        .size(14)
                        .width(Length::Fill),
                    text(count).size(12).style(theme::muted),
                ]
                .spacing(8),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::history_item(app == picked))
            .on_press(Message::Rules(rules::Message::LibraryApp(app)))
            .into(),
        );
    }
    let mut rules: Column<'_, Message> = column![].spacing(10);
    let in_config = in_config(state);
    for (index, rule) in LIBRARY.iter().enumerate() {
        if rule.app != picked {
            continue;
        }
        let have = in_config[index];
        let mut mark = checkbox(have || state.rules.library.marked.contains(&index))
            .label(rule.name)
            .text_size(14);
        if !have {
            mark = mark.on_toggle(move |on| Message::Rules(rules::Message::LibraryMark(index, on)));
        }
        rules = rules.push(
            container(
                column![
                    mark,
                    text(rule.why).size(12).style(theme::muted),
                    text(if have {
                        format!("Already in your config. From {}.", rule.source)
                    } else {
                        format!("From {}.", rule.source)
                    })
                    .size(12)
                    .style(theme::muted),
                    highlight::block(rule.kdl, colors, 12.0),
                ]
                .spacing(6),
            )
            .padding([10, 12])
            .width(Length::Fill)
            .style(theme::group),
        );
    }
    let marked: Vec<usize> = state.rules.library.marked.iter().copied().collect();
    let mut chosen = column![].spacing(4);
    for &index in &marked {
        let rule = &LIBRARY[index];
        chosen = chosen.push(
            row![
                text(rule.app.map_or_else(
                    || rule.name.to_string(),
                    |app| format!("{}: {}", app.name(), rule.name),
                ))
                .size(13)
                .width(Length::Fill),
                controls::remove_button(
                    format!("Unmark {}", rule.name),
                    Message::Rules(rules::Message::LibraryMark(index, false)),
                ),
            ]
            .align_y(Alignment::Center),
        );
    }
    if marked.is_empty() {
        chosen = chosen.push(text("Nothing marked yet.").size(13).style(theme::muted));
    }
    // the file import writes to: beside the config, as add_to_include puts it
    let file_exists = state
        .store()
        .is_some_and(|store| store.text_of(&store.config_dir().join(FILE)).is_some());
    let to_file = format!("{FILE}{}", if file_exists { "" } else { " (new file)" });
    let mut import =
        button(text(format!("Import {}", count(marked.len(), "rule", "rules"))).size(13))
            .style(theme::primary);
    if !marked.is_empty() {
        import = import.on_press(Message::Rules(rules::Message::ImportLibrary));
    }
    column![
        text("From niri's docs and issues, and from configs people share.")
            .size(12)
            .style(theme::muted),
        checkbox(!state.rules.library.all_apps)
            .label("Only apps installed here")
            .text_size(13)
            .on_toggle(|only| Message::Rules(rules::Message::LibraryAllApps(!only))),
        rows::group(String::new(), list),
        rules,
        text("Marked for import").size(14).font(SEMIBOLD),
        container(chosen)
            .padding([10, 12])
            .width(Length::Fill)
            .style(theme::group),
        text("Write them to").size(13),
        radio(
            to_file,
            false,
            Some(state.rules.library.into_config),
            move |value| Message::Rules(rules::Message::LibraryIntoConfig(value)),
        )
        .text_size(13),
        radio(
            "config.kdl",
            true,
            Some(state.rules.library.into_config),
            move |value| Message::Rules(rules::Message::LibraryIntoConfig(value)),
        )
        .text_size(13),
        text(if !state.rules.library.into_config && !file_exists {
            "Adds one include line to config.kdl."
        } else {
            ""
        })
        .size(12)
        .style(theme::muted),
        import,
    ]
    .spacing(10)
    .into()
}

/// Add the marked rules, as one change.
pub fn import(state: &mut App) {
    let Some(store) = &mut state.session.store else {
        return;
    };
    let marked: Vec<usize> = state.rules.library.marked.iter().copied().collect();
    if let Err(e) = library::import(store, &marked, state.rules.library.into_config) {
        state.status = crate::app::error(e.to_string());
        return;
    }
    state.rules.library.marked.clear();
    state.status = crate::app::info(format!(
        "Imported {}.",
        count(marked.len(), "rule", "rules")
    ));
}
