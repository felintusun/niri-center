//! The window: the header with search and the latest change, the sidebar,
//! the page and the config beside it, and the cards that float over them.

use iced::widget::{
    Column, Row, button, column, container, mouse_area, opaque, row, scrollable, space, stack,
    text, text_input,
};
use iced::{Alignment, Color, Length, Theme};
use niri_center::bindings::KeyCombo;
use niri_center::page::Page;
use niri_center::search;
use niri_center::session::{Platform, Problem, SaveState};
use niri_center::summary;
use niri_center::words::{count, shortened};

use crate::app::{App, Closing};
use crate::message::Message;
use crate::rows::{self, SEMIBOLD};
use crate::{El, controls, history, icons, pages, repair, source, theme};

/// The header's search field, to focus it from the keyboard.
pub const SEARCH_INPUT: &str = "search-input";

/// A small part of a page laid out by the width it gets, for a part whose
/// width the page cannot work out: a toolbar beside the keyboard, a field
/// in the rule form. It is built again at every layout, so whole pages go
/// by [`App::content_width`] instead.
pub fn by_width<'view>(layout: impl Fn(f32) -> El<'view> + 'view) -> El<'view> {
    iced::widget::responsive(move |size| layout(size.width))
        .height(Length::Shrink)
        .into()
}

pub fn view(state: &App) -> El<'_> {
    let base: El<'_> = container(frame(state))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(iced::Background::Color(theme::colors(theme).background)),
            text_color: Some(theme::colors(theme).text),
            ..container::Style::default()
        })
        .into();
    // the toast floats over the page; the rest of the page stays clickable
    let toast: El<'_> = toast(state).unwrap_or_else(|| space().into());
    // a display change on trial: over every page, so it never goes back
    // unseen, dropping in at the top as niri's own notification does
    let trial: El<'_> = pages::displays::trial(state).map_or_else(
        || space().into(),
        |card| {
            // centered over the page, not the window: the sidebar and the
            // config column keep their room
            let beside = |width: f32| if width > 0.0 { width + 1.0 } else { 0.0 };
            container(crate::motion::drop_in(card, state.motions.notice))
                .center_x(Length::Fill)
                .padding(iced::Padding {
                    top: 64.0,
                    right: 16.0 + beside(state.source_width()),
                    bottom: 0.0,
                    left: 16.0 + beside(state.sidebar_width()),
                })
                .into()
        },
    );
    let dialog: El<'_> = if state.review_open && state.session.pending() > 0 {
        overlay(review_card(state), Message::ShowReview(false))
    } else if let Some(form) = (state.page == Page::Rules)
        .then(|| pages::rules::dialog(state))
        .flatten()
    {
        overlay(
            dialog_card(form, state.window_height),
            Message::Rules(pages::rules::Message::CancelEdit),
        )
    } else if state.closing == Some(Closing::Asking) {
        overlay(close_card(state), Message::CloseCancel)
    } else if state.help_open {
        overlay(help_card(state), Message::ShowHelp(false))
    } else {
        space().into()
    };
    // the same layers whether or not a toast, trial or dialog shows: iced
    // keeps a widget's state (how far a list is scrolled, which field has
    // focus) by its place in the tree, so wrapping the page only while
    // something floats over it would scroll every list back to the top
    stack![base, toast, trial, dialog].into()
}

/// Header, banner, then the sidebar, the page and its config source, and
/// what became of the latest change along the bottom.
fn frame(state: &App) -> El<'_> {
    let narrow = state.narrow();
    let mut content = column![]
        .padding(if narrow { [16, 16] } else { [20, 24] })
        .spacing(16)
        .width(Length::Fill)
        .max_width(state.content_width() + 48.0);
    if state.header_search.is_none()
        && let Some(panel) = repair::panel(state)
    {
        content = content.push(panel);
    }
    content = if let Some((query, selected)) = &state.header_search {
        content.push(search_results(state, query, *selected))
    } else {
        content.push(pages::page(state))
    };
    if state.page.pops_out() && state.pane.is_none() && cfg!(not(target_arch = "wasm32")) {
        content = content.push(
            button(text("Open in its own window").size(13))
                .style(theme::secondary)
                .on_press(Message::PopOut(state.page)),
        );
    }
    // the page sits in the middle of the room it has, as KDE and GNOME
    // center their settings, so a wide window has room on both sides
    let page: El<'_> = if state.page_scrolls_itself() && state.header_search.is_none() {
        container(content.height(Length::Fill))
            .center_x(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        scrollable(container(content).center_x(Length::Fill))
            .style(theme::scrollbar)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };
    let mut body = Row::new().height(Length::Fill);
    if state.pane.is_none() {
        body = body.push(sidebar(state)).push(hairline_down());
    }
    body = body.push(container(page).width(Length::Fill).height(Length::Fill));
    if state.source_open() {
        body = body.push(hairline_down()).push(
            container(source::view(state))
                .width(state.source_width())
                .height(Length::Fill),
        );
    }
    let mut frame = column![
        container(header(state))
            .width(Length::Fill)
            .style(theme::header_band)
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    frame = frame.push(hairline_across());
    if let Some(banner) = banner(state) {
        frame = frame.push(banner);
    }
    frame = frame.push(body);
    if let Some(saving) = saving_line(state, narrow) {
        frame = frame.push(hairline_across()).push(
            container(saving)
                .padding(if narrow { [8, 12] } else { [8, 16] })
                .width(Length::Fill)
                .style(theme::header_band),
        );
    }
    frame.into()
}

/// The sections under their headings, always at the left. Folded, it is a
/// rail of icons that name themselves on hover; a narrow window folds it.
fn sidebar(state: &App) -> El<'_> {
    let folded = state.sidebar_folded();
    let mut list = column![].spacing(2).width(Length::Fill);
    for (index, (heading, pages)) in Page::SIDEBAR.into_iter().enumerate() {
        if folded {
            if index > 0 {
                list = list.push(container(hairline_across()).padding([8, 6]));
            }
        } else {
            list = list.push(
                container(text(heading).size(12).style(theme::muted)).padding(iced::Padding {
                    top: if index == 0 { 0.0 } else { 14.0 },
                    right: 8.0,
                    bottom: 4.0,
                    left: 10.0,
                }),
            );
        }
        for &page in pages {
            // the search results stand in for the page while they show
            let current = page == state.page && state.header_search.is_none();
            let entry: El<'_> = if folded {
                iced::widget::tooltip(
                    button(container(icons::strong(page_icon(page), 16.0)).center_x(Length::Fill))
                        .padding([8, 0])
                        .width(Length::Fill)
                        .style(theme::history_item(current))
                        .on_press(Message::Page(page)),
                    controls::tip(page.label()),
                    iced::widget::tooltip::Position::Right,
                )
                .into()
            } else {
                button(
                    row![icons::strong(page_icon(page), 16.0), {
                        // the font left unset where it is the default:
                        // the browser build draws an explicit default
                        // as nothing
                        let label = text(page.label()).size(14);
                        if current { label.font(SEMIBOLD) } else { label }
                    },]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::history_item(current))
                .on_press(Message::Page(page))
                .into()
            };
            list = list.push(entry);
        }
    }
    let fold = sidebar_button(
        icons::Icon::Sidebar,
        None,
        if folded {
            "Show the names, Ctrl+B"
        } else {
            "Fold to icons, Ctrl+B"
        },
        false,
        Some(Message::ToggleSidebar),
    );
    let help = sidebar_button(
        icons::Icon::Help,
        None,
        "Help, F1",
        state.help_open,
        Some(Message::ShowHelp(!state.help_open)),
    );
    let (toggle, what) = config_toggle(state);
    let config = sidebar_button(
        icons::Icon::Source,
        (!folded).then_some("Config text"),
        what,
        state.source_open(),
        toggle,
    );
    // one row under the names, a column in the folded rail
    let foot: El<'_> = if folded {
        column![config, help, fold].spacing(2).into()
    } else {
        row![
            container(config).width(Length::Fill),
            container(help).width(40),
            container(fold).width(40),
        ]
        .spacing(2)
        .into()
    };
    container(
        column![
            scrollable(list)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(4)
                        .scroller_width(4)
                        .spacing(4),
                ))
                .style(theme::scrollbar)
                .height(Length::Fill),
            hairline_across(),
            foot,
        ]
        .spacing(8),
    )
    .padding(if folded { [12, 6] } else { [16, 10] })
    .width(state.sidebar_width())
    .height(Length::Fill)
    .into()
}

/// What the config button at the foot of the sidebar does on this page,
/// and its tooltip: the page's part of config.kdl beside it, or in a
/// window of its own where there is no room beside it. Pages without a
/// part of their own leave it unavailable.
pub(crate) fn config_toggle(state: &App) -> (Option<Message>, &'static str) {
    if state.store().is_none() || !state.page.has_source() {
        return (None, "This page has no config text of its own");
    }
    if state.shows_source() {
        return if state.source_shown {
            (Some(Message::ShowSource(false)), "Hide config.kdl, Ctrl+.")
        } else {
            (
                Some(Message::ShowSource(true)),
                "Show config.kdl beside the page, Ctrl+.",
            )
        };
    }
    if cfg!(target_arch = "wasm32") {
        (Some(Message::Page(Page::Source)), "Show config.kdl")
    } else {
        (
            Some(Message::PopOut(Page::Source)),
            "Show config.kdl in its own window, Ctrl+Shift+N",
        )
    }
}

/// A 1 px line across, between parts of the window.
fn hairline_across() -> El<'static> {
    container(space().height(1).width(Length::Fill))
        .style(theme::hairline)
        .into()
}

/// A 1 px line down, between columns.
fn hairline_down() -> El<'static> {
    container(space().width(1).height(Length::Fill))
        .style(theme::hairline)
        .into()
}

/// A button at the foot of the sidebar: an icon and its `label`, or the
/// icon alone, which its tooltip names. `on` marks what it shows as
/// showing. Without a message it is unavailable.
fn sidebar_button<'text>(
    icon: icons::Icon,
    label: Option<&'text str>,
    tip: &'text str,
    on: bool,
    message: Option<Message>,
) -> El<'text> {
    let available = message.is_some();
    let drawn = if available {
        icons::strong(icon, 16.0)
    } else {
        icons::icon(icon, 16.0)
    };
    let face: El<'text> = if let Some(label) = label {
        let words = text(label).size(13).wrapping(text::Wrapping::None);
        row![
            drawn,
            if available {
                words
            } else {
                words.style(theme::muted)
            }
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    } else {
        container(drawn).center_x(Length::Fill).into()
    };
    let mut press = button(face)
        .padding(if label.is_some() { [8, 10] } else { [8, 0] })
        .width(Length::Fill)
        .style(theme::history_item(on));
    if let Some(message) = message {
        press = press.on_press(message);
    }
    iced::widget::tooltip(
        press,
        controls::tip(tip),
        iced::widget::tooltip::Position::Right,
    )
    .into()
}

/// The search, or the page's name in a window of its own.
fn header(state: &App) -> El<'_> {
    let narrow = state.narrow();
    let mut header = Row::<Message>::new()
        .spacing(12)
        .padding(if narrow { [10, 12] } else { [10, 16] })
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if state.pane.is_some() {
        header = header.push(
            text(state.page.label())
                .size(15)
                .font(SEMIBOLD)
                .width(Length::Fill),
        );
    } else {
        let query = state
            .header_search
            .as_ref()
            .map_or("", |(query, _)| query.as_str());
        let search = container(
            row![
                icons::icon(icons::Icon::Search, 14.0),
                text_input(
                    if state.page == Page::Rules {
                        "Find an app, rule, setting or key"
                    } else {
                        "Find a setting, key or action"
                    },
                    query,
                )
                .id(SEARCH_INPUT)
                .on_input(Message::SearchQuery)
                .on_submit(Message::SearchSubmit)
                .size(14)
                .padding(0)
                .style(theme::bare_input),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([7, 10])
        .style(theme::search_box);
        header = header.push(if narrow {
            El::from(search.width(Length::Fill))
        } else {
            row![
                search.width(Length::Fixed(440.0)),
                space().width(Length::Fill)
            ]
            .width(Length::Fill)
            .into()
        });
    }
    header.into()
}

/// What happened to the latest change, and Undo: along the window's
/// bottom, for every page, since Undo takes back the latest change
/// wherever it was made. A display change on trial has its banner instead.
fn saving_line(state: &App, narrow: bool) -> Option<El<'_>> {
    let undo = || {
        button(text("Undo").size(13))
            .style(theme::secondary)
            .on_press(Message::Undo)
    };
    let latest = state
        .session
        .history
        .last()
        .filter(|_| state.session.saved_this_session);
    if state.session.trial.is_some() {
        return None;
    }
    let (note, buttons): (El<'_>, Vec<El<'_>>) = match state.session.save_state() {
        SaveState::WaitingForNiri => (
            text("Waiting until niri can read the config")
                .size(12)
                .style(theme::pending)
                .into(),
            vec![undo().into()],
        ),
        SaveState::NotSaved(_) => (
            text("Not saved").size(12).style(theme::error).into(),
            vec![undo().into()],
        ),
        SaveState::OnRequest { files } => (
            text(format!("{} not saved", count(files, "file", "files")))
                .size(12)
                .style(theme::pending)
                .into(),
            vec![
                button(text("Review").size(13))
                    .style(theme::secondary)
                    .on_press(Message::ShowReview(true))
                    .into(),
                button(text("Save").size(13))
                    .style(theme::primary)
                    .on_press(Message::SaveNow)
                    .into(),
            ],
        ),
        SaveState::Saving => (
            text("Saving").size(12).style(theme::muted).into(),
            vec![undo().into()],
        ),
        SaveState::Saved => {
            let entry = latest?;
            let title = summary::short_title(entry);
            let shown = if narrow { shortened(title, 40) } else { title };
            (
                text(format!("{shown} saved"))
                    .size(12)
                    .style(theme::muted)
                    .into(),
                vec![undo().into()],
            )
        }
    };
    let mut line = row![container(note).width(Length::Fill)]
        .spacing(12)
        .align_y(Alignment::Center);
    for button in buttons {
        line = line.push(button);
    }
    Some(line.into())
}

/// How many settings, pages, keys and actions the search lists.
const SEARCH_RESULTS: usize = 12;

/// One thing the search found: what it says, and what picking it sends.
pub struct Hit {
    pub label: String,
    pub detail: String,
    pub message: Message,
}

/// What the search finds: on the page shown first (the apps and rules of
/// Window rules), then settings, pages, keys and actions everywhere. The
/// view lists them, and Enter and the arrows pick among them in this
/// order.
pub struct Hits {
    pub here: Vec<Hit>,
    pub everywhere: Vec<Hit>,
}

impl Hits {
    pub fn of(state: &App, query: &str) -> Self {
        let here = if state.page == Page::Rules {
            pages::rules::found(state, query)
                .into_iter()
                .map(|found| Hit {
                    label: found.label,
                    detail: found.detail,
                    message: Message::SearchPickHere(found.message),
                })
                .collect()
        } else {
            Vec::new()
        };
        let everywhere = state
            .search_index
            .matches(query)
            .take(SEARCH_RESULTS)
            .map(|entry| Hit {
                label: entry.label.clone(),
                detail: entry.detail.clone(),
                message: Message::SearchPick(entry.clone()),
            })
            .collect();
        Self { here, everywhere }
    }

    pub fn len(&self) -> usize {
        self.here.len() + self.everywhere.len()
    }

    /// The `n`th, counting the page's first.
    pub fn nth(self, place: usize) -> Option<Hit> {
        self.here.into_iter().chain(self.everywhere).nth(place)
    }
}

/// What the header search found, in place of the page: on the page shown
/// first, then everywhere.
fn search_results(state: &App, query: &str, selected: usize) -> El<'static> {
    let hits = Hits::of(state, query);
    let skip = hits.here.len();
    let rows = |hits: Vec<Hit>, first: usize| -> Vec<El<'static>> {
        hits.into_iter()
            .enumerate()
            .map(|(index, hit)| {
                button(
                    row![
                        text(hit.label).size(14).width(Length::Fill),
                        text(hit.detail)
                            .size(12)
                            .font(theme::MONO)
                            .style(theme::muted),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(theme::history_item(index + first == selected))
                .on_press(hit.message)
                .into()
            })
            .collect()
    };
    let here = rows(hits.here, 0);
    let everywhere = rows(hits.everywhere, skip);
    match (here.is_empty(), everywhere.is_empty()) {
        (true, true) => text("Nothing matches. Try a word like gaps, touchpad or rule.")
            .size(14)
            .style(theme::muted)
            .into(),
        (true, false) => rows::group(String::new(), everywhere),
        (false, true) => rows::group(state.page.label().to_string(), here),
        (false, false) => column![
            rows::group(state.page.label().to_string(), here),
            rows::group("Everywhere".to_string(), everywhere),
        ]
        .spacing(16)
        .into(),
    }
}

/// A full-width strip under the header, tinted with one of the colors.
fn strip(content: Column<'_, Message>, tint: fn(&theme::Colors) -> Color) -> El<'_> {
    container(content.spacing(8))
        .padding([8, 16])
        .width(Length::Fill)
        .style(move |theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color {
                a: 0.14,
                ..tint(theme::colors(theme))
            })),
            ..container::Style::default()
        })
        .into()
}

/// Why the latest change is not saved, or what the browser demo is. niri
/// failing to read the config gets a panel on the page instead.
fn banner(state: &App) -> Option<El<'_>> {
    if let Some(problem) = &state.session.problem {
        let (message, details, copy) = match problem {
            Problem::Rejected(diagnostics) => (
                "Not saved: niri does not accept the latest change. Changes before it are saved. Fix it, or undo it.".to_string(),
                Some(diagnostics.clone()),
                false,
            ),
            Problem::ChangedOnDisk(file) => (
                format!(
                    "Not saved: another program changed {file} meanwhile. Undo your change to load their version, or copy your change as a diff first."
                ),
                None,
                true,
            ),
            Problem::ReadOnly(file) => (
                format!(
                    "Not saved: {file} cannot be written here (it is read-only, for example a link into the Nix store). Copy the change as a diff to apply it where the config is made."
                ),
                None,
                true,
            ),
            Problem::Failed(reason) => (format!("Not saved: {reason}"), None, true),
        };
        let mut actions = row![text(message).size(13).width(Length::Fill)]
            .spacing(12)
            .align_y(Alignment::Center);
        if copy {
            actions = actions.push(
                button(text("Copy as a diff").size(13))
                    .style(theme::secondary)
                    .on_press(Message::CopyPending),
            );
        }
        let mut content = column![actions];
        if let Some(details) = details {
            content = content.push(controls::diagnostics(&details));
        }
        return Some(strip(content, |colors| colors.error));
    }
    if let Some(store) = &state.session.store
        && state
            .session
            .read_only
            .iter()
            .any(|path| path == store.path())
    {
        let mut copy = button(text("Copy as a diff").size(13)).style(theme::secondary);
        if state.session.pending() > 0 {
            copy = copy.on_press(Message::CopyPending);
        }
        return Some(strip(
            column![
                row![
                    text(format!(
                        "{} is read-only here, for example a link into the Nix store, so changes are not saved. Try them out, then copy them as a diff.",
                        store.file_label(store.path())
                    ))
                    .size(13)
                    .width(Length::Fill),
                    copy,
                ]
                .spacing(12)
                .align_y(Alignment::Center)
            ],
            |colors| colors.pending,
        ));
    }
    if state.session.platform == Platform::Browser && state.session.health().is_none() {
        return Some(strip(
            column![
                text(
                    "Running in your browser on niri's default config. Changes stay in this page."
                )
                .size(13),
            ],
            |colors| colors.accent,
        ));
    }
    None
}

/// The latest message, as a toast under the header on the right: what
/// worked in green with a check, an error in red with an alert sign.
fn toast(state: &App) -> Option<El<'_>> {
    let status = state.status.as_ref()?;
    let tint: fn(&theme::Colors) -> Color = if status.error {
        |colors| colors.error
    } else {
        |colors| colors.success
    };
    let card = container(
        row![
            if status.error {
                icons::tinted(icons::Icon::Alert, 16.0, icons::Tint::Error)
            } else {
                icons::tinted(icons::Icon::Check, 16.0, icons::Tint::Success)
            },
            text(status.text.clone()).size(13).width(Length::Fill),
            iced::widget::tooltip(
                button(icons::icon(icons::Icon::Remove, 10.0))
                    .padding(6)
                    .style(button::text)
                    .on_press(Message::DismissToast),
                controls::tip("Dismiss"),
                iced::widget::tooltip::Position::Left,
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .max_width(420)
    .style(move |theme: &Theme| theme::toast(theme, tint));
    let narrow = state.narrow();
    Some(
        container(card)
            .width(Length::Fill)
            .align_x(if narrow {
                iced::alignment::Horizontal::Center
            } else {
                iced::alignment::Horizontal::Right
            })
            .padding(iced::Padding {
                top: 96.0,
                right: 16.0,
                bottom: 0.0,
                left: 16.0,
            })
            .into(),
    )
}

/// Dim the page and float `card` above it; clicking the dimmed area sends
/// `dismiss`.
fn overlay(card: El<'_>, dismiss: Message) -> El<'_> {
    let backdrop = mouse_area(
        container(space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::background(Color::from_rgba8(0, 0, 0, 0.55))),
    )
    .on_press(dismiss);
    stack![
        opaque(backdrop),
        container(opaque(card))
            .center_x(Length::Fill)
            .padding([80, 20]),
    ]
    .into()
}

/// A dialog for a form taller than a card: it scrolls inside the window.
fn dialog_card(content: El<'_>, window_height: f32) -> El<'_> {
    let room = if window_height > 0.0 {
        (window_height - 160.0).max(240.0)
    } else {
        f32::INFINITY
    };
    container(iced::widget::scrollable(container(content).padding(20)))
        .width(Length::Fixed(640.0))
        .max_height(room)
        .style(theme::popup)
        .into()
}

fn overlay_card(content: Column<'_, Message>) -> El<'_> {
    container(content.spacing(8).padding(16))
        .width(Length::Fixed(560.0))
        .style(theme::popup)
        .into()
}

/// Help: niri's own documentation for the page, the app's keys, and what
/// the app is.
fn help_card(state: &App) -> El<'_> {
    let pane_mode = state.pane.is_some();
    let mut list = column![
        row![
            text("Help").size(18).font(SEMIBOLD).width(Length::Fill),
            text("F1 or Ctrl+? shows this").size(12).style(theme::muted),
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(6);
    if state.page.docs().is_some() {
        list = list.push(
            row![
                text(format!(
                    "What niri says about {}",
                    state.page.label().to_lowercase()
                ))
                .size(13)
                .width(Length::Fill),
                button(text("Open niri's documentation").size(13))
                    .style(theme::secondary)
                    .on_press(Message::OpenDocs(state.page)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    list = list.push(
        container(text("Keyboard").size(15).font(SEMIBOLD)).padding(iced::Padding {
            top: 8.0,
            ..iced::Padding::ZERO
        }),
    );
    for group in search::HelpGroup::ALL {
        // single-pane windows have no sections to move between or search
        if pane_mode && group == search::HelpGroup::MovingAround {
            continue;
        }
        list = list.push(
            container(text(group.heading()).size(13).font(SEMIBOLD)).padding(iced::Padding {
                top: 8.0,
                ..iced::Padding::ZERO
            }),
        );
        for line in search::HELP.iter().filter(|line| line.group == group) {
            list = list.push(
                row![
                    text(line.what.as_str()).size(13).width(Length::Fill),
                    text(line.keys.as_str())
                        .size(13)
                        .font(theme::MONO)
                        .style(theme::accent),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
    }
    // niri takes a bound key before the app sees it
    let clashes: Vec<String> = state
        .binding_rows()
        .iter()
        .filter(|row| !row.disabled)
        .filter(|row| {
            let combo = KeyCombo::parse(&row.key);
            search::HELP
                .iter()
                .flat_map(|line| line.binds)
                .any(|key| KeyCombo::parse(key) == combo)
        })
        .map(|row| row.key.clone())
        .collect();
    if !clashes.is_empty() {
        list = list.push(space().height(6));
        list = list.push(
            text(format!(
                "Bound in your config, so niri takes {} before the app sees it.",
                clashes.join(", ")
            ))
            .size(12)
            .style(theme::pending),
        );
    }
    list = list.push(
        column![
            text("About").size(15).font(SEMIBOLD),
            text(format!(
                "niri-center {}, an unofficial settings app for niri.",
                env!("CARGO_PKG_VERSION")
            ))
            .size(13),
        ]
        .spacing(6)
        .padding(iced::Padding {
            top: 8.0,
            ..iced::Padding::ZERO
        }),
    );
    overlay_card(list)
}

/// What is not saved yet, file by file, with Save: for saving on request.
fn review_card(state: &App) -> El<'_> {
    let Some(store) = &state.session.store else {
        return space().into();
    };
    let colors = state.colors();
    let card_width = 720.0_f32.min(state.width() - 40.0);
    let mut list = column![
        text("Not saved yet").size(18).font(SEMIBOLD),
        text("niri validate checks it when you save.")
            .size(12)
            .style(theme::muted),
    ]
    .spacing(8);
    for file in store.pending_diffs() {
        list = list.push(text(file.path).size(13).font(theme::MONO));
        list = list.push(history::diff_view(&file.hunks, colors, card_width - 48.0));
    }
    list = list.push(
        // as every dialog: the way out last, at the right
        row![
            button(text("Copy as a diff").size(13))
                .style(theme::secondary)
                .on_press(Message::CopyPending),
            space().width(Length::Fill),
            button(text("Save these changes").size(13))
                .style(theme::primary)
                .on_press(Message::SaveNow),
            button(text("Undo all of it").size(13))
                .style(theme::danger)
                .on_press(Message::Undo),
            button(text("Keep editing").size(13))
                .style(theme::secondary)
                .on_press(Message::ShowReview(false)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    container(scrollable(list).style(theme::scrollbar))
        .max_height(state.height() - 160.0)
        .width(Length::Fixed(card_width))
        .padding(16)
        .style(theme::popup)
        .into()
}

/// Asked when the window is closed while niri turns down the latest change.
/// Closing with edits that are not saved: why, and what can be done.
fn close_card(state: &App) -> El<'_> {
    let latest = "Close without the latest change?";
    let (title, body, can_save) = match state.session.save_state() {
        SaveState::OnRequest { files } => (
            "Save before closing?",
            format!(
                "{} not saved. Saving waits for Save, as the Files page sets it.",
                count(files, "file", "files")
            ),
            true,
        ),
        SaveState::NotSaved(Problem::Rejected(_)) => (
            latest,
            "niri does not accept it, so it is not saved. Everything before it is.".to_string(),
            false,
        ),
        SaveState::NotSaved(Problem::ChangedOnDisk(file)) => (
            latest,
            format!("Another program changed {file} meanwhile, so it is not saved."),
            false,
        ),
        SaveState::NotSaved(Problem::ReadOnly(file)) => (
            "Close without your changes?",
            format!(
                "{file} cannot be written here, so they are not saved. The banner can copy them as a diff first."
            ),
            false,
        ),
        SaveState::NotSaved(Problem::Failed(reason)) => (
            latest,
            format!("It could not be saved: {reason}."),
            false,
        ),
        SaveState::WaitingForNiri => (
            "Close without the latest changes?",
            "niri cannot read the config on disk, so saving waits until it can. Closing drops what waits."
                .to_string(),
            false,
        ),
        SaveState::Saving | SaveState::Saved => (
            "Close now?",
            "A save is still being written.".to_string(),
            false,
        ),
    };
    // as every dialog: right-aligned, the way out last
    let mut buttons = row![space().width(Length::Fill)].spacing(10);
    if can_save {
        buttons = buttons.push(
            button(text("Save and close").size(13))
                .style(theme::primary)
                .on_press(Message::SaveAndClose),
        );
    }
    buttons = buttons
        .push(
            button(
                text(if can_save {
                    "Close without saving"
                } else {
                    "Close without it"
                })
                .size(13),
            )
            .style(theme::danger)
            .on_press(Message::CloseDiscard),
        )
        .push(
            button(text("Keep editing").size(13))
                .style(theme::secondary)
                .on_press(Message::CloseCancel),
        );
    overlay_card(column![
        text(title).size(18).font(SEMIBOLD),
        text(body).size(13),
        buttons,
    ])
}

/// A page's icon in the sidebar.
const fn page_icon(page: Page) -> icons::Icon {
    match page {
        Page::Keybinds => icons::Icon::Keyboard,
        Page::Input => icons::Icon::Mouse,
        Page::Cursor => icons::Icon::Pointer,
        Page::Gestures => icons::Icon::Gesture,
        Page::Displays => icons::Icon::Display,
        Page::Windows => icons::Icon::Windows,
        Page::Screenshots => icons::Icon::Camera,
        Page::Layout => icons::Icon::Columns,
        Page::Rules => icons::Icon::WindowRule,
        Page::Workspaces => icons::Icon::Workspaces,
        Page::Overview => icons::Icon::Overview,
        Page::Animations => icons::Icon::Curve,
        Page::Startup => icons::Icon::Startup,
        Page::Environment => icons::Icon::Environment,
        Page::History => icons::Icon::Clock,
        Page::Events => icons::Icon::Events,
        Page::Files => icons::Icon::Folder,
        Page::Source => icons::Icon::File,
    }
}
