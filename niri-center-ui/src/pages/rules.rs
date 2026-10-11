//! The Window rules page: an index of apps, each with its open windows and
//! the config's rules for it, the form for a new rule, and the rule
//! library as a second tab.
//!
//! The page goes three levels in: the index, an app, one rule. With room,
//! the index stays beside the level open; in a narrow window only that
//! level shows, under the way back up.

use iced::widget::{
    Column, button, checkbox, column, container, pick_list, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Length, Task};
use std::collections::BTreeSet;

use niri_center::by_app::{self, Entry, Who};
use niri_center::forms::RuleDraft;
use niri_center::library::KnownApp;
use niri_center::niri::{self, OpenWindow};
use niri_center::page::Page;
use niri_center::rules::When;
use niri_center::store::{RuleProperty, TextMatch};

use super::{on_off_label, pad};
use crate::app::{App, error, info, outcome, room, turned};
use crate::controls::{self, small};
use crate::message::Message as AppMessage;
use crate::rows;
use crate::view::by_width;
use crate::{El, theme};

/// The rule library's picks.
#[derive(Debug, Default)]
pub struct Library {
    /// It shows in place of the app or rule open.
    pub open: bool,
    /// The app whose rules show; `None` is every window.
    pub app: Option<KnownApp>,
    /// The rules marked for import, by index.
    pub marked: BTreeSet<usize>,
    /// Apps not installed here show too.
    pub all_apps: bool,
    /// Imported rules go into config.kdl itself, not a file of their own.
    pub into_config: bool,
}

/// The rule form, the rule or window open, and the library.
#[derive(Debug, Default)]
pub struct State {
    /// The new rule, or the config's rule being changed in the form.
    pub draft: RuleDraft,
    /// niri waits for a click on the window a rule is for.
    pub picking: bool,
    /// The app the page is about, by its entry in the index.
    pub app: Option<Who>,
    /// The open window a new rule is made from: its title can be added.
    pub window: Option<OpenWindow>,
    /// The config's rule the page shows, by index.
    pub open: Option<usize>,
    /// The rule form shows over the page.
    pub dialog: bool,
    /// A rule being edited as text, over the page: its index and the text.
    pub editor: Option<(usize, iced::widget::text_editor::Content)>,
    pub library: Library,
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    /// Which windows the new rule is for: app, title, state.
    App(String),
    AppHow(TextMatch),
    Title(String),
    TitleHow(TextMatch),
    When(Option<When>),
    /// Show an app's windows and rules (`None`: the index).
    Show(Option<Who>),
    /// Show one of the config's rules.
    Open(usize),
    /// Back up one level: from a rule to its app, from an app to the index.
    Up,
    /// Show the app after (or before) the one shown in the index.
    Step(bool),
    Property(RuleProperty, bool),
    /// Show the open window with this app ID and title.
    FromWindow(String, String),
    /// Show that window and open the form for a rule for its app.
    NewForWindow(String, String),
    /// Open the form for a new rule, for this app ID if given.
    New(Option<String>),
    /// Change a rule: in the form when it can say all of the rule, else
    /// as text.
    Edit(usize),
    /// Add the picked window's title to the rule being made.
    TitleFromWindow,
    PickWindow,
    WindowPicked(Result<Option<(String, String)>, String>),
    Add,
    On(usize, bool),
    Remove(usize),
    /// Edit a rule as text.
    EditText(usize),
    /// Load the config's rule at this index into the form.
    EditInForm(usize),
    Editor(iced::widget::text_editor::Action),
    SaveText,
    CancelEdit,
    /// Add one library rule to the config, as suggested for an app.
    AddLibraryRule(usize),
    /// Show the rule library (or go back to the rules).
    OpenLibrary(bool),
    LibraryApp(Option<KnownApp>),
    LibraryMark(usize, bool),
    LibraryAllApps(bool),
    LibraryIntoConfig(bool),
    ImportLibrary,
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::App(app) => state.rules.draft.app = app,
        Message::AppHow(how) => state.rules.draft.app_how = how,
        Message::Title(title) => state.rules.draft.title = title,
        Message::TitleHow(how) => state.rules.draft.title_how = how,
        Message::When(when) => state.rules.draft.when = when,
        Message::Open(index) => {
            // an edit belongs to its rule; going elsewhere leaves it
            if state
                .rules
                .draft
                .editing
                .is_some_and(|editing| editing != index)
            {
                state.rules.draft = RuleDraft::default();
            }
            state.rules.open = Some(index);
            state.rules.library.open = false;
        }
        Message::Show(who) => {
            state.rules.window = who
                .as_ref()
                .and_then(|who| state.app_index().iter().find(|entry| &entry.who == who))
                .and_then(|entry| entry.windows.first())
                .and_then(|&at| windows(state).get(at).cloned());
            state.rules.app = who;
            state.rules.open = None;
            state.rules.library.open = false;
        }
        Message::Up => {
            if let Some(at) = state.rules.open {
                // the app the way back up showed
                state.rules.app = shown_with(state, at);
                state.rules.open = None;
            } else if state.rules.library.open {
                state.rules.library.open = false;
            } else {
                state.rules.app = None;
                state.rules.window = None;
            }
        }
        Message::Step(down) => {
            let index = state.app_index();
            // beside the index, the first app shows until another is picked
            let at = match &state.rules.app {
                Some(who) => index.iter().position(|entry| &entry.who == who),
                None => state.rules_beside_list().then_some(0),
            };
            let next = match (at, down) {
                (None, true) => Some(0),
                (None, false) => index.len().checked_sub(1),
                (Some(at), true) => Some((at + 1).min(index.len().saturating_sub(1))),
                (Some(at), false) => Some(at.saturating_sub(1)),
            };
            if let Some(entry) = next.and_then(|next| index.get(next)) {
                let who = entry.who.clone();
                return update(state, Message::Show(Some(who)));
            }
        }
        Message::Property(property, on) => {
            if on {
                state.rules.draft.properties.insert(property);
            } else {
                state.rules.draft.properties.remove(&property);
            }
        }
        Message::FromWindow(app_id, title) => {
            let _ = update(
                state,
                Message::Show(Some(by_app::who(&state.system, &app_id))),
            );
            state.rules.window = windows(state)
                .iter()
                .find(|w| w.app_id == app_id && w.title == title)
                .cloned();
            state.page = Page::Rules;
        }
        Message::NewForWindow(app_id, title) => {
            let _ = update(state, Message::FromWindow(app_id.clone(), title));
            return update(state, Message::New(Some(app_id)));
        }
        Message::New(app_id) => {
            // titles change (a terminal's shows its folder): the app alone,
            // and the title only when asked for
            state.rules.draft = RuleDraft {
                app: app_id.unwrap_or_default(),
                app_how: TextMatch::Exact,
                ..RuleDraft::default()
            };
            state.rules.editor = None;
            state.rules.dialog = true;
            // the app is what a new rule needs first
            return iced::widget::operation::focus(APP_INPUT);
        }
        Message::Edit(index) => {
            let sayable = state
                .rule_rows()
                .get(index)
                .is_some_and(|rule| RuleDraft::from_rule(index, rule).is_some());
            return update(
                state,
                if sayable {
                    Message::EditInForm(index)
                } else {
                    Message::EditText(index)
                },
            );
        }
        Message::PickWindow => {
            state.rules.picking = true;
            state.status = info("Click the window the rule is for. Escape cancels.");
            return state.ask_niri(niri::Command::PickWindow);
        }
        Message::WindowPicked(result) => {
            state.rules.picking = false;
            match result {
                Ok(Some((app_id, title))) => {
                    state.status = info(format!("Picked {app_id}."));
                    return update(state, Message::FromWindow(app_id, title));
                }
                Ok(None) => state.status = info("No window picked."),
                Err(e) => state.status = error(format!("Could not ask niri for a window: {e}")),
            }
        }
        Message::Add => {
            let Some(store) = &mut state.session.store else {
                return Task::none();
            };
            match state.rules.draft.apply(store) {
                Ok((at, done)) => {
                    // what happens as it opens waits for the next opening
                    let at_open = state
                        .rules
                        .draft
                        .properties
                        .iter()
                        .any(|property| property.at_open());
                    // changed on its page, it stays there; else its app
                    // shows it, the app shown when the rule is one of its
                    if state.rules.open.is_some() {
                        state.rules.open = Some(at);
                    } else {
                        // the index has the new rule once the rows follow
                        state.refresh_rows();
                        state.rules.app = shown_with(state, at);
                    }
                    state.rules.draft = RuleDraft::default();
                    state.rules.dialog = false;
                    state.status = info(if at_open {
                        format!(
                            "{done} What it does when a window opens starts with the next one that opens."
                        )
                    } else {
                        done.to_string()
                    });
                }
                Err(e) => state.status = error(e),
            }
        }
        Message::On(index, on) => {
            if let Some(store) = &mut state.session.store {
                state.status = outcome(
                    store.set_window_rule_disabled(index, !on),
                    turned("Rule", on),
                );
            }
        }
        Message::Remove(index) => {
            if let Some(store) = &mut state.session.store {
                // what pointed at the rules after it follows them up
                // (`refresh_rows`)
                state.status = outcome(store.remove_window_rule(index), "Rule removed.");
            }
        }
        Message::EditText(index) => {
            state.rules.dialog = false;
            if let Some(text) = state.store().and_then(|s| s.window_rule_source(index)) {
                state.rules.editor =
                    Some((index, iced::widget::text_editor::Content::with_text(&text)));
            }
        }
        Message::Editor(action) => {
            if let Some((_, content)) = &mut state.rules.editor {
                content.perform(action);
            }
        }
        Message::SaveText => {
            let (Some((index, content)), Some(store)) =
                (&state.rules.editor, &mut state.session.store)
            else {
                return Task::none();
            };
            match store.replace_window_rule(*index, &content.text()) {
                Ok(at) => {
                    if state.rules.open.is_some() {
                        state.rules.open = Some(at);
                    }
                    state.rules.editor = None;
                    state.status = info("Rule updated.");
                }
                Err(e) => state.status = error(e.to_string()),
            }
        }
        Message::CancelEdit => {
            state.rules.dialog = false;
            state.rules.editor = None;
            state.rules.draft = RuleDraft::default();
        }
        Message::EditInForm(index) => {
            let rule = state.rule_rows().get(index).cloned();
            if let Some(draft) = rule.and_then(|rule| RuleDraft::from_rule(index, &rule)) {
                state.rules.draft = draft;
                state.rules.editor = None;
                state.rules.dialog = true;
                return iced::widget::operation::focus(APP_INPUT);
            }
        }
        Message::OpenLibrary(open) => {
            state.rules.library.open = open;
            if open {
                state.rules.open = None;
            }
        }
        Message::LibraryApp(app) => state.rules.library.app = app,
        Message::LibraryMark(index, on) => {
            if on {
                state.rules.library.marked.insert(index);
            } else {
                state.rules.library.marked.remove(&index);
            }
        }
        Message::LibraryAllApps(all) => state.rules.library.all_apps = all,
        Message::LibraryIntoConfig(into_config) => state.rules.library.into_config = into_config,
        Message::ImportLibrary => crate::library::import(state),
        Message::AddLibraryRule(index) => {
            if let (Some(store), Some(rule)) = (
                &mut state.session.store,
                niri_center::library::LIBRARY.get(index),
            ) {
                match store.add_window_rule_text(rule.kdl) {
                    Ok(_) => state.status = info(format!("Added: {}.", rule.name)),
                    Err(e) => state.status = error(e.to_string()),
                }
            }
        }
        Message::TitleFromWindow => {
            if let Some(window) = &state.rules.window {
                state.rules.draft.title = window.title.clone();
                state.rules.draft.title_how = TextMatch::Contains;
            }
        }
    }
    Task::none()
}

/// The open windows niri reports.
fn windows(state: &App) -> &[OpenWindow] {
    state
        .running
        .as_ref()
        .map_or(&[], |data| data.windows.as_slice())
}

/// The app a rule shows under: the one shown when the rule is one of its.
fn shown_with(state: &App, rule: usize) -> Option<Who> {
    by_app::card(state.app_index(), state.rules.app.as_ref(), rule).map(|entry| entry.who.clone())
}

/// Whether there is a level above the one shown, for Escape.
pub fn can_go_up(state: &App) -> bool {
    state.rules.open.is_some() || state.rules.library.open || state.rules.app.is_some()
}

/// A match on this page for the header search: an app or a rule.
pub struct Found {
    pub label: String,
    pub detail: String,
    pub message: Message,
}

/// The apps and rules on this page with every word of `query`: an app by
/// its name or an app ID, a rule by its name or any of its lines.
pub fn found(state: &App, query: &str) -> Vec<Found> {
    let words = niri_center::search::Words::of(query);
    let rules = state.rule_rows();
    let apps = state.app_index().iter().filter_map(|entry| {
        let ids: Vec<&str> = entry.app_ids.iter().map(String::as_str).collect();
        words
            .all_in(&format!("{} {}", entry.name, ids.join(" ")).to_lowercase())
            .then(|| Found {
                label: entry.name.clone(),
                detail: entry.counts(rules),
                message: Message::Show(Some(entry.who.clone())),
            })
    });
    let found_rules = rules.iter().enumerate().filter_map(|(at, rule)| {
        let name = niri_center::rules::name(rule);
        let lines: Vec<&str> = rule
            .criteria
            .iter()
            .chain(&rule.properties)
            .map(String::as_str)
            .collect();
        words
            .all_in(&format!("{name} {}", lines.join(" ")).to_lowercase())
            .then(|| Found {
                label: name,
                detail: format!("{}:{}", rule.file, rule.line),
                message: Message::Open(at),
            })
    });
    apps.chain(found_rules).take(FOUND_MOST).collect()
}

/// The index of apps, and the level open: beside it when there is room,
/// else that level alone under the way back up.
pub fn page(state: &App) -> El<'_> {
    let beside = state.rules_beside_list();
    let body = if state.rules.library.open {
        let library = crate::library::view(state);
        if beside {
            scrollable(library)
                .spacing(8)
                .height(Length::Fill)
                .style(theme::scrollbar)
                .into()
        } else {
            library
        }
    } else {
        your_rules(state, beside)
    };
    let mut page = column![tabs(state.rules.library.open), body]
        .spacing(16)
        .width(Length::Fill);
    if beside {
        page = page.height(Length::Fill);
    }
    page.into()
}

/// The config's rules, or the library's: the page shows one at a time.
fn tabs<'a>(library: bool) -> El<'a> {
    let tab = |label: &'static str, on: bool, open_library: bool| {
        button(text(label).size(13))
            .padding([5, 14])
            .style(theme::history_item(on))
            .on_press(AppMessage::Rules(Message::OpenLibrary(open_library)))
    };
    row![
        tab("Your rules", !library, false),
        tab("Rule library", library, true),
    ]
    .spacing(4)
    .into()
}

/// The index of apps and the level open: beside it when there is room,
/// else that level alone under the way back up.
fn your_rules(state: &App, beside: bool) -> El<'_> {
    let rules = state.rule_rows();
    let windows = windows(state);
    let index = state.app_index();
    // beside the index, the first app shows until another is picked,
    // rather than an empty column
    let entry = match &state.rules.app {
        Some(who) => index.iter().find(|entry| &entry.who == who),
        None if beside && state.rules.open.is_none() => index.first(),
        None => None,
    };
    let rule = state
        .rules
        .open
        .and_then(|at| rules.get(at).map(|rule| (at, rule)));
    let (detail, above): (Option<El<'_>>, Option<&Entry>) = match rule {
        Some((at, rule)) => (
            Some(rule_detail(state, rule, at, windows)),
            by_app::card(index, state.rules.app.as_ref(), at),
        ),
        None => (
            entry.map(|entry| app_card(state, entry, rules, windows)),
            None,
        ),
    };
    // beside the index the way up is the index itself, but from a rule
    let detail = detail.map(|detail| -> El<'_> {
        let mut level = column![].spacing(12).width(Length::Fill);
        if !beside || above.is_some() {
            level = level.push(crumbs(above));
        }
        level.push(detail).into()
    });
    let shown = above.or(entry).map(|entry| &entry.who);
    if beside {
        let column = |content| {
            scrollable(content)
                .spacing(8)
                .height(Length::Fill)
                .style(theme::scrollbar)
        };
        row![
            container(column(index_view(state, index, rules, shown)))
                .width(index_width(state.content_width())),
            container(column(detail.unwrap_or_else(|| space().into()))).width(Length::Fill),
        ]
        .spacing(24)
        .height(Length::Fill)
        .into()
    } else {
        detail.unwrap_or_else(|| index_view(state, index, rules, shown))
    }
}

/// The way back up from the level open: all apps, then the app a rule is
/// shown under.
fn crumbs<'a>(app: Option<&Entry>) -> El<'a> {
    let link = |label: String, message: Message| {
        button(text(label).size(13))
            .padding(0)
            .style(theme::link)
            .on_press(AppMessage::Rules(message))
    };
    let mut line = row![link("All apps".to_string(), Message::Show(None))]
        .spacing(6)
        .align_y(Alignment::Center);
    if let Some(app) = app {
        line = line
            .push(crate::icons::icon(crate::icons::Icon::Chevron, 12.0))
            .push(link(app.name.clone(), Message::Show(Some(app.who.clone()))));
    }
    line.into()
}

/// Every app with windows open or rules, then the rules for every window
/// and those by title or state: one row each, with how many of each.
fn index_view<'a>(
    state: &'a App,
    index: &[Entry],
    rules: &[niri_center::store::WindowRuleRow],
    shown: Option<&Who>,
) -> El<'a> {
    let mut pick: button::Button<'_, AppMessage> = button(
        text(if state.rules.picking {
            "Waiting for a click"
        } else {
            "Pick a window"
        })
        .size(12),
    )
    .padding([3, 10])
    .style(theme::secondary);
    if !state.rules.picking {
        pick = pick.on_press(AppMessage::Rules(Message::PickWindow));
    }
    let have = crate::library::in_config(state);
    let listed: Vec<El<'a>> = index
        .iter()
        .map(|entry| {
            // one line each: the name, then how much it has
            let mut line = row![
                text(entry.name.clone()).size(14).width(Length::Fill),
                text(entry.counts(rules)).size(12).style(theme::muted),
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            // the library knows this app: say so, and its card shows how
            let suggested = entry
                .app_ids
                .iter()
                .map(|id| niri_center::library::missing_for(have, id))
                .max()
                .unwrap_or(0);
            if suggested > 0 {
                line = line.push(
                    text(format!("{suggested} suggested"))
                        .size(12)
                        .style(theme::accent),
                );
            }
            button(line)
                .padding([7, 12])
                .width(Length::Fill)
                .style(theme::history_item(shown == Some(&entry.who)))
                .on_press(AppMessage::Rules(Message::Show(Some(entry.who.clone()))))
                .into()
        })
        .collect();
    column![
        row![
            text("Apps")
                .size(14)
                .font(rows::SEMIBOLD)
                .width(Length::Fill),
            pick,
            add("New rule".to_string(), None),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
        if index.is_empty() {
            El::from(
                text("No window rules yet, and niri reports no open windows.")
                    .size(13)
                    .style(theme::muted),
            )
        } else {
            rows::group(String::new(), listed)
        },
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}

/// A rule's name and where it is written. Within an app, who it is for
/// goes without saying, so it says what it does and what else picks its
/// windows (a title, a state).
fn rule_heading<'a>(
    rule: &niri_center::store::WindowRuleRow,
    within_app: bool,
) -> Column<'a, AppMessage> {
    let name = text(if within_app {
        niri_center::rules::what(rule)
    } else {
        niri_center::rules::name(rule)
    })
    .size(14);
    column![
        if rule.disabled {
            name.style(theme::muted)
        } else {
            name
        },
        text(place(rule, within_app))
            .size(12)
            .font(theme::MONO)
            .style(theme::muted),
    ]
}

/// A rule in a list, to open it.
fn rule_row<'a>(state: &App, rule: &niri_center::store::WindowRuleRow, at: usize) -> El<'a> {
    button(rule_heading(rule, false))
        .padding([8, 12])
        .width(Length::Fill)
        .style(theme::history_item(state.rules.open == Some(at)))
        .on_press(AppMessage::Rules(Message::Open(at)))
        .into()
}

/// A rule on its app's page: its name, its lines, and what can be done to
/// it, so changing it is one click away.
fn rule_block<'a>(
    state: &'a App,
    rule: &niri_center::store::WindowRuleRow,
    at: usize,
    within_app: bool,
) -> El<'a> {
    rule_heading(rule, within_app)
        .push(rule_text(state, rule))
        .push(rule_actions(rule, at))
        .spacing(6)
        .padding([10, 12])
        .width(Length::Fill)
        .into()
}

/// Under a rule's name: what else picks its windows within an app, where
/// it is written, and whether it is off.
fn place(rule: &niri_center::store::WindowRuleRow, within_app: bool) -> String {
    let mut words = vec![format!("{}:{}", rule.file, rule.line)];
    if within_app && let Some(condition) = niri_center::rules::condition(rule) {
        words.insert(0, condition);
    }
    if rule.disabled {
        words.push("off".to_string());
    }
    words.join(", ")
}

/// One app: what it is, its open windows, its rules in the order niri
/// applies them, the rules from elsewhere that reach its windows, and
/// what the library suggests for it.
fn app_card<'a>(
    state: &'a App,
    entry: &Entry,
    rules: &[niri_center::store::WindowRuleRow],
    windows: &[OpenWindow],
) -> El<'a> {
    // who a rule is for goes without saying, but for those by title
    let within_app = matches!(entry.who, Who::App(_) | Who::Every);
    let mut heading = column![text(entry.name.clone()).size(20).font(rows::SEMIBOLD)].spacing(2);
    let ids: Vec<&str> = entry
        .app_ids
        .iter()
        .map(String::as_str)
        .filter(|id| !id.eq_ignore_ascii_case(&entry.name))
        .collect();
    if !ids.is_empty() {
        heading = heading.push(
            text(ids.join(", "))
                .size(12)
                .font(theme::MONO)
                .style(theme::muted),
        );
    }
    let mut body = column![heading].spacing(12).width(Length::Fill);
    let alike = entry.alike_windows(windows);
    if !alike.is_empty() {
        let mut lines = column![].spacing(2);
        for (window, count) in alike {
            let state_word = if window.floating { "floating" } else { "tiled" };
            lines = lines.push(
                text(if count == 1 {
                    format!("{}, {state_word}", window.title)
                } else {
                    format!("{}, {state_word}, {count} windows", window.title)
                })
                .size(13),
            );
        }
        body = body.push(section("Open windows", lines.into()));
    }
    let listed: Vec<El<'a>> = entry
        .rules
        .iter()
        .filter_map(|&at| {
            rules
                .get(at)
                .map(|rule| rule_block(state, rule, at, within_app))
        })
        .collect();
    let app_id = entry
        .windows
        .first()
        .and_then(|&at| windows.get(at))
        .map(|window| window.app_id.clone())
        .or_else(|| entry.app_ids.iter().next().cloned());
    let new = match (&entry.who, app_id) {
        (Who::App(_), Some(app_id)) => add(format!("New rule for {}", entry.name), Some(app_id)),
        (Who::App(_) | Who::Every | Who::Others, _) => add("New rule".to_string(), None),
    };
    body = body.push(
        column![
            row![
                text("Rules")
                    .size(14)
                    .font(rows::SEMIBOLD)
                    .width(Length::Fill),
                new
            ]
            .align_y(Alignment::Center),
            if listed.is_empty() {
                El::from(text("No rules for it yet.").size(13).style(theme::muted))
            } else {
                rows::group(String::new(), listed)
            },
        ]
        .spacing(2),
    );
    // what else reaches its windows, and what nearly does
    let elsewhere = entry.elsewhere(rules, windows);
    if !elsewhere.applies.is_empty() {
        body = body.push(rows::group(
            "Other rules that apply to its windows".to_string(),
            elsewhere
                .applies
                .iter()
                .filter_map(|&at| rules.get(at).map(|rule| rule_row(state, rule, at)))
                .collect(),
        ));
    }
    let mut notes = column![].spacing(4);
    for note in elsewhere.notes {
        notes = notes.push(text(note).size(12).style(theme::muted));
    }
    body = body.push(notes);
    if let Some(suggested) = entry
        .app_ids
        .iter()
        .find_map(|id| crate::library::suggestions(state, id))
    {
        body = body.push(suggested);
    }
    body.into()
}

/// The plus that opens the form for a new rule, for `app_id` if given;
/// `what` names it on hover.
fn add<'a>(what: String, app_id: Option<String>) -> El<'a> {
    controls::icon_button(
        crate::icons::Icon::Add,
        14.0,
        what,
        AppMessage::Rules(Message::New(app_id)),
    )
}

/// A part of a card: a small heading over its content.
fn section<'a>(heading: &'static str, content: El<'a>) -> El<'a> {
    column![text(heading).size(14).font(rows::SEMIBOLD), content]
        .spacing(6)
        .into()
}

/// One of the config's rules: its text, what it reaches, and what can be
/// done with it.
fn rule_detail<'a>(
    state: &'a App,
    rule: &niri_center::store::WindowRuleRow,
    index: usize,
    windows: &[niri_center::niri::OpenWindow],
) -> El<'a> {
    let mut body = column![
        text(niri_center::rules::name(rule))
            .size(20)
            .font(rows::SEMIBOLD),
    ]
    .spacing(10)
    .width(Length::Fill);
    body = body.push(rule_text(state, rule));
    let reached: Vec<String> = windows
        .iter()
        .filter(|window| {
            niri_center::rules::reach(rule, window) == niri_center::rules::Reach::Reaches
        })
        .map(|window| format!("{} ({})", window.title, window.app_id))
        .collect();
    body = body.push(
        text(match reached.len() {
            0 => "It reaches none of the open windows.".to_string(),
            1 => format!("It reaches 1 open window: {}.", reached[0]),
            n => format!("It reaches {n} open windows: {}.", reached.join(", ")),
        })
        .size(12)
        .style(theme::muted),
    );

    body.push(rule_actions(rule, index)).into()
}

/// A rule's lines as written, colored.
fn rule_text<'a>(state: &App, rule: &niri_center::store::WindowRuleRow) -> El<'a> {
    let lines: Vec<String> = rule
        .criteria
        .iter()
        .chain(&rule.properties)
        .map(|written| niri_center::words::dedent(written))
        .collect();
    crate::highlight::block(&lines.join("\n"), state.colors(), 13.0)
}

/// What can be done to a rule: change it, turn it off or on, remove it.
fn rule_actions<'a>(rule: &niri_center::store::WindowRuleRow, index: usize) -> El<'a> {
    row![
        // the form where it can say all of the rule, else the text
        small("Edit", AppMessage::Rules(Message::Edit(index))),
        niri_center::forms::RuleDraft::from_rule(index, rule)
            .map(|_| small("Edit as text", AppMessage::Rules(Message::EditText(index)))),
        small(
            on_off_label(rule.disabled),
            AppMessage::Rules(Message::On(index, rule.disabled))
        ),
        small("Remove", AppMessage::Rules(Message::Remove(index))),
    ]
    .spacing(8)
    .into()
}

fn rule_editor(content: &iced::widget::text_editor::Content) -> El<'_> {
    container(
        column![
            iced::widget::text_editor(content)
                .on_action(move |value| AppMessage::Rules(Message::Editor(value)))
                .font(theme::MONO)
                .size(13)
                .height(Length::Shrink),
            row![
                button(text("Save changes").size(13))
                    .style(theme::primary)
                    .on_press(AppMessage::Rules(Message::SaveText)),
                small("Cancel", AppMessage::Rules(Message::CancelEdit)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(8),
    )
    .padding([8, 14])
    .into()
}

/// How the form's app ID or title matches, as a pick list shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct How(niri_center::store::TextMatch);

impl std::fmt::Display for How {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use niri_center::store::TextMatch;
        f.write_str(match self.0 {
            TextMatch::Exact => "is exactly",
            TextMatch::Contains => "contains",
            TextMatch::Pattern => "matches pattern",
        })
    }
}

const HOWS: [How; 3] = [
    How(niri_center::store::TextMatch::Exact),
    How(niri_center::store::TextMatch::Contains),
    How(niri_center::store::TextMatch::Pattern),
];

fn rule_builder<'a>(state: &'a App, windows: &[niri_center::niri::OpenWindow]) -> El<'a> {
    let draft = &state.rules.draft;
    // a row of the "which windows" card: a word, then the control
    let labeled = |label: &'static str, control: El<'a>| -> El<'a> {
        pad(row![text(label).size(14).width(64), control]
            .spacing(8)
            .align_y(Alignment::Center)
            .into())
    };
    let text_match = |how: niri_center::store::TextMatch,
                      on_how: fn(niri_center::store::TextMatch) -> AppMessage,
                      value: &'a str,
                      placeholder: &'static str,
                      on_input: fn(String) -> AppMessage,
                      id: &'static str|
     -> El<'a> {
        by_width(move |width| {
            let how =
                pick_list(HOWS, Some(How(how)), move |picked: How| on_how(picked.0)).width(150);
            let field = text_input(placeholder, value)
                .id(id)
                .on_input(on_input)
                .on_submit(AppMessage::Rules(Message::Add))
                .font(theme::MONO)
                .width(Length::Fill);
            // a narrow column puts the text under how it matches
            if width < room::MATCH_BESIDE_TEXT {
                column![how, field].spacing(6).into()
            } else {
                row![how, field]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into()
            }
        })
    };
    let when = pick_list(niri_center::rules::When::ALL, draft.when, |picked| {
        AppMessage::Rules(Message::When(Some(picked)))
    })
    .placeholder("is in any state");
    let mut when_line = row![when.width(Length::Fill)]
        .spacing(8)
        .align_y(Alignment::Center);
    if draft.when.is_some() {
        when_line = when_line.push(controls::remove_button(
            "Any state".to_string(),
            AppMessage::Rules(Message::When(None)),
        ));
    }
    let which = rows::group(
        String::new(),
        vec![
            labeled(
                "App",
                text_match(
                    draft.app_how,
                    move |value| AppMessage::Rules(Message::AppHow(value)),
                    &draft.app,
                    "any app, or for example org.gnome.Calculator",
                    move |value| AppMessage::Rules(Message::App(value)),
                    APP_INPUT,
                ),
            ),
            labeled(
                "Title",
                text_match(
                    draft.title_how,
                    move |value| AppMessage::Rules(Message::TitleHow(value)),
                    &draft.title,
                    "any title",
                    move |value| AppMessage::Rules(Message::Title(value)),
                    TITLE_INPUT,
                ),
            ),
            labeled("While it", when_line.into()),
        ],
    );
    // what happens: once as the window opens, or for as long as it matches
    let effects = |at_open: bool| {
        let mut list = column![].spacing(6);
        for property in RuleProperty::ALL
            .into_iter()
            .filter(|property| property.at_open() == at_open)
        {
            list = list.push(
                checkbox(draft.properties.contains(&property))
                    .label(property.label())
                    .text_size(14)
                    .on_toggle(move |on| AppMessage::Rules(Message::Property(property, on))),
            );
        }
        list
    };
    let matcher = draft.rule().matcher();
    let reaches = if matcher.props.is_empty() {
        String::new()
    } else {
        let matchers = [matcher];
        match windows
            .iter()
            .filter(|w| {
                niri_center::rules::reach_when_on(&matchers, w)
                    == niri_center::rules::Reach::Reaches
            })
            .count()
        {
            0 => "No open window matches it now".to_string(),
            1 => "1 open window matches it now".to_string(),
            n => format!("{n} open windows match it now"),
        }
    };
    column![
        text(if draft.editing.is_some() {
            "Change the rule"
        } else {
            "New rule"
        })
        .size(18)
        .font(rows::SEMIBOLD),
        text("Which windows").size(13).style(theme::muted),
        which,
        title_from_window(state),
        text("What happens").size(13).style(theme::muted),
        rows::group(
            String::new(),
            vec![
                pad(column![
                    text("When it opens").size(13).font(rows::SEMIBOLD),
                    effects(true),
                ]
                .spacing(8)
                .into()),
                pad(column![
                    text("For as long as it matches")
                        .size(13)
                        .font(rows::SEMIBOLD),
                    effects(false),
                ]
                .spacing(8)
                .into()),
            ],
        ),
        if draft.editing.is_some() {
            row![
                button(text("Save changes").size(13))
                    .style(theme::primary)
                    .on_press(AppMessage::Rules(Message::Add)),
                small("Cancel", AppMessage::Rules(Message::CancelEdit)),
                text(reaches).size(12).style(theme::muted),
            ]
        } else {
            row![
                button(text("Add rule").size(13))
                    .style(theme::primary)
                    .on_press(AppMessage::Rules(Message::Add)),
                small("Cancel", AppMessage::Rules(Message::CancelEdit)),
                text(reaches).size(12).style(theme::muted),
            ]
        }
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(8)
    .into()
}

/// Under the rule form, for a picked window: its title is left out by
/// default, since titles change; this adds it on request.
fn title_from_window(state: &App) -> El<'_> {
    match &state.rules.window {
        Some(window) if state.rules.draft.title.trim().is_empty() && !window.title.is_empty() => {
            small(
                format!("Also match the title \"{}\"", window.title),
                AppMessage::Rules(Message::TitleFromWindow),
            )
            .into()
        }
        Some(_) | None => space().height(0).into(),
    }
}

/// The form's text fields, to focus and to save from with Enter.
const APP_INPUT: &str = "rule-app";
const TITLE_INPUT: &str = "rule-title";

/// The most apps and rules the header search lists from this page.
const FOUND_MOST: usize = 6;

/// How wide the index is beside the level open: two fifths of the page,
/// from 260 px for the names to 440 px, past which the rest goes to the
/// app.
fn index_width(content: f32) -> f32 {
    (content * 0.4).clamp(260.0, 440.0)
}

/// The rule form, or a rule's text, over the page while it is open.
pub fn dialog(state: &App) -> Option<El<'_>> {
    if let Some((_, content)) = &state.rules.editor {
        return Some(
            column![
                text("Change the rule").size(18).font(rows::SEMIBOLD),
                rule_editor(content),
            ]
            .spacing(8)
            .into(),
        );
    }
    if !state.rules.dialog {
        return None;
    }
    let mut form = column![rule_builder(state, windows(state))].spacing(8);
    if let Some(status) = state.status.as_ref().filter(|status| status.error) {
        form = form.push(text(status.text.clone()).size(12).style(theme::error));
    }
    Some(form.into())
}
