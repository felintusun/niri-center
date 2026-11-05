//! The Keybinds page: the keyboard picture, the bindings on it, and the
//! form for a new one or one being changed.

use std::collections::HashMap;

use iced::widget::{
    Row, button, column, container, pick_list, row, rule, scrollable, space, text, text_input,
    tooltip,
};
use iced::{Alignment, Length, Task};
use niri_center::bindings::{self, ActionInfo, KeyCombo, Modifier, key_id};
use niri_center::forms::BindingForm;

use super::{include_warnings, on_off_label, pad, tag};
use niri_center::keyboard::{Picture, Xkb};
use niri_center::store::Store;

use crate::app::{App, error, info, outcome, room, turned};
use crate::controls::{self, small};
use crate::message::Message as AppMessage;
use crate::rows;
use crate::view::by_width;
use crate::{El, keyboard, theme};

/// Long `spawn` command lines show as a preview; the full text is a hover away.
const ACTION_PREVIEW_CHARS: usize = 72;

// ---- keybinds ----

/// The binding form, the keyboard picture and the list's filters.
pub struct State {
    /// niri's actions, as the linked niri-ipc defines them.
    pub actions: Vec<ActionInfo>,
    /// The action picker: niri's actions, searchable by name and description.
    pub action_box: iced::widget::combo_box::State<ActionChoice>,
    /// Modifiers picked while building a combo on the keyboard picture;
    /// `None` when not building one.
    pub composing: Option<Vec<Modifier>>,
    /// The binding form, while it is open.
    pub form: Option<BindingForm>,
    /// The next key pressed picks a key on the picture.
    pub finding_keys: bool,
    pub search: String,
    /// Only keys bound more than once are listed.
    pub only_doubled: bool,
    pub keyboard_hidden: bool,
    pub key_filter: Option<String>,
    /// The layout picked for the keyboard picture; `None` follows niri.
    pub layout: Option<String>,
    /// The configured layout niri types with, by its place in the list.
    pub niri_layout: Option<usize>,
    /// The keyboard picture and what it shows; made again only when what
    /// it is made from moves.
    pub shown: Option<Shown>,
}

/// The keyboard picture as made: compiling a keymap is too slow for every
/// frame.
pub struct Shown {
    /// The config's revision, the layout picked and niri's.
    made_from: (u64, Option<String>, Option<usize>),
    /// The layout shown, and its picture: US when it has none (`found`).
    pub layout: String,
    pub picture: Picture,
    pub found: bool,
}

impl Default for State {
    fn default() -> Self {
        let actions = bindings::actions();
        Self {
            action_box: action_box(&actions),
            actions,
            composing: None,
            form: None,
            finding_keys: false,
            search: String::new(),
            only_doubled: false,
            keyboard_hidden: false,
            key_filter: None,
            layout: None,
            niri_layout: None,
            shown: None,
        }
    }
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    // the binding form, for a new binding or one being changed
    Key(String),
    Action(String),
    Args(String),
    /// Add the new binding, or change the one being edited.
    Save,
    /// Open the form on the binding at this index.
    Edit(usize),
    Close,
    Remove(usize),
    /// Turn a binding on (true) or off: `/-` in front of it.
    On(usize, bool),
    KeyClicked(String),
    /// Build the key combo by clicking the keyboard picture (or stop).
    Compose(bool),
    ComposeModifier(Modifier),
    /// Open the new-binding form, its key ending on this one if given.
    New(Option<String>),
    /// Listen for the next key pressed in the window (or stop).
    FindKeys(bool),
    KeyFound(String),
    Layout(String),
    Search(String),
    ToggleKeyboard,
    /// List only keys bound more than once (or all again).
    OnlyDoubled(bool),
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Key(key) => {
            if let Some(form) = &mut state.keybinds.form {
                form.key = key;
            }
        }
        Message::Action(action) => {
            if let Some(form) = &mut state.keybinds.form {
                form.action = Some(action);
            }
        }
        Message::Args(args) => {
            if let Some(form) = &mut state.keybinds.form {
                form.args = args;
            }
        }
        Message::Save => {
            let (Some(store), Some(form)) = (&mut state.session.store, &state.keybinds.form) else {
                return Task::none();
            };
            match form.apply(store, &state.rows.bindings) {
                Ok(done) => {
                    state.keybinds.form = None;
                    state.status = info(done);
                }
                Err(failure) => state.status = error(failure.to_string()),
            }
        }
        Message::Remove(index) => {
            let Some(store) = &mut state.session.store else {
                return Task::none();
            };
            state.status = match store.remove_binding(index) {
                Ok(_) => None,
                Err(failure) => error(failure.to_string()),
            };
        }
        Message::Edit(index) => {
            if let Some(binding) = state.binding_rows().get(index) {
                // start from the source text: the display form drops quotes
                let (action, args) = bindings::split_action(&binding.action_raw);
                state.keybinds.form = Some(BindingForm {
                    key: binding.key.clone(),
                    action: Some(action),
                    args,
                    editing: Some(index),
                });
            }
        }
        Message::Close => {
            state.keybinds.form = None;
            state.keybinds.composing = None;
        }
        Message::On(index, on) => {
            if let Some(store) = &mut state.session.store {
                state.status = outcome(
                    store.set_binding_disabled(index, !on),
                    turned("Binding", on),
                );
            }
        }
        Message::Layout(code) => state.keybinds.layout = Some(code),
        Message::Compose(on) => {
            state.keybinds.composing = on.then(Vec::new);
            if on {
                state.keybinds.keyboard_hidden = false;
            }
        }
        Message::ComposeModifier(modifier) => {
            if let Some(mods) = &mut state.keybinds.composing {
                if let Some(at) = mods.iter().position(|held| *held == modifier) {
                    mods.remove(at);
                } else {
                    mods.push(modifier);
                }
            }
        }
        Message::KeyClicked(token) if state.keybinds.composing.is_some() => {
            // a modifier key on the picture toggles like its chip
            if let Some(modifier) = bindings::Modifier::COMPOSED
                .into_iter()
                .find(|modifier| modifier.written() == token)
            {
                return update(state, Message::ComposeModifier(modifier));
            }
            let mut combo: Vec<&str> = bindings::Modifier::COMPOSED
                .into_iter()
                .filter(|modifier| {
                    state
                        .keybinds
                        .composing
                        .as_ref()
                        .is_some_and(|mods| mods.contains(modifier))
                })
                .map(bindings::Modifier::written)
                .collect();
            combo.push(&token);
            if let Some(form) = &mut state.keybinds.form {
                form.key = combo.join("+");
            }
            state.keybinds.composing = None;
        }
        Message::KeyClicked(token) => {
            // clicking the active key (or Clear) turns the filter off
            if state.keybinds.key_filter.as_deref() == Some(token.as_str()) {
                state.keybinds.key_filter = None;
            } else {
                state.keybinds.key_filter = Some(token);
            }
        }
        Message::Search(query) => state.keybinds.search = query,
        Message::OnlyDoubled(only) => state.keybinds.only_doubled = only,
        Message::New(key) => {
            state.keybinds.form = Some(BindingForm {
                key: key.map(|key| format!("Mod+{key}")).unwrap_or_default(),
                ..BindingForm::default()
            });
        }
        Message::FindKeys(on) => state.keybinds.finding_keys = on,
        Message::KeyFound(pressed) => {
            state.keybinds.finding_keys = false;
            // a typed character is the key whose cap shows it: `Ü` is
            // `udiaeresis` on a German keyboard
            let token = state
                .keybinds
                .shown
                .as_ref()
                .and_then(|shown| {
                    shown
                        .picture
                        .rows
                        .iter()
                        .flatten()
                        .find(|key| key.label == pressed)
                })
                .map_or(pressed.clone(), |key| key.token.clone());
            state.keybinds.key_filter = Some(token);
            // the list on that key takes the place of a new binding's form
            if state
                .keybinds
                .form
                .as_ref()
                .is_some_and(|form| form.editing.is_none())
            {
                state.keybinds.form = None;
            }
        }
        Message::ToggleKeyboard => state.keybinds.keyboard_hidden = !state.keybinds.keyboard_hidden,
    }
    Task::none()
}

pub fn page<'app>(state: &'app App, store: &Store) -> El<'app> {
    let rows = state.binding_rows();
    // usage counts for the keyboard picture (trailing combo key, lowercased)
    let mut counts: HashMap<String, usize> = HashMap::new();
    for binding in rows.iter().filter(|binding| !binding.disabled) {
        let last = niri_center::keyboard::last_key(&binding.key);
        if !last.is_empty() {
            *counts.entry(key_id(last)).or_insert(0) += 1;
        }
    }
    let bindings_counted = niri_center::words::count(rows.len(), "binding", "bindings");
    // the toolbar names the file most bindings are in, and only rows from
    // other files name theirs
    let home = niri_center::store::home_binding_file(rows);
    let counted = match home {
        None => bindings_counted,
        Some((home, 0)) => format!("{bindings_counted} in {home}"),
        Some((home, others)) => format!(
            "{bindings_counted} in {home} and {}",
            niri_center::words::count(others, "other file", "other files")
        ),
    };
    // the count, where the toolbar has room for it
    let toolbar = by_width(move |width| {
        let mut toolbar = row![
            small(
                if state.keybinds.finding_keys {
                    "Stop finding"
                } else {
                    "Find by pressing keys"
                },
                AppMessage::Keybinds(Message::FindKeys(!state.keybinds.finding_keys)),
            ),
            small("New binding", AppMessage::Keybinds(Message::New(None))),
            small(
                if state.keybinds.keyboard_hidden {
                    "Show keyboard"
                } else {
                    "Hide keyboard"
                },
                AppMessage::Keybinds(Message::ToggleKeyboard),
            ),
            space().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if width >= room::BINDING_COUNT {
            toolbar = toolbar.push(text(counted.clone()).size(12).style(theme::muted));
        }
        toolbar.into()
    });
    let mut keyboard = column![toolbar].spacing(12).width(Length::Fill);
    if state.keybinds.finding_keys {
        keyboard = keyboard.push(
            text(
                "Press a key. niri keeps combos with Mod for itself, so press the key on its own.",
            )
            .size(13)
            .style(theme::pending),
        );
    }
    let bindings = key_bindings(state, rows, &counts, home.map(|(home, _)| home));
    if !state.keybinds.keyboard_hidden {
        keyboard = keyboard.push(keyboard_picture(state, store, counts));
    }
    // two columns side by side when there is room, else one under the other,
    // the bindings scrolling under the keyboard when the window is tall
    if state.keybinds_pinned() {
        row![
            container(keyboard).width(Length::FillPortion(3)),
            container(
                scrollable(bindings)
                    .spacing(8)
                    .height(Length::Fill)
                    .style(theme::scrollbar),
            )
            .width(Length::FillPortion(2)),
        ]
        .spacing(24)
        .height(Length::Fill)
        .into()
    } else if state.keyboard_stays() {
        column![
            keyboard,
            rule::horizontal(1),
            scrollable(bindings)
                .spacing(8)
                .height(Length::Fill)
                .style(theme::scrollbar),
        ]
        .spacing(16)
        .height(Length::Fill)
        .into()
    } else {
        column![keyboard, rule::horizontal(1), bindings]
            .spacing(16)
            .into()
    }
}

/// Beside or under the keyboard: the bindings on the chosen key (all of
/// them when none is chosen), or the form for a new one.
/// `home` is the file most bindings are in: rows from other files name
/// theirs.
fn key_bindings<'app>(
    state: &'app App,
    rows: &[niri_center::store::BindingRow],
    counts: &HashMap<String, usize>,
    home: Option<&str>,
) -> El<'app> {
    if let Some(form) = state
        .keybinds
        .form
        .as_ref()
        .filter(|form| form.editing.is_none())
    {
        return column![
            row![
                text("New binding")
                    .size(20)
                    .font(rows::SEMIBOLD)
                    .width(Length::Fill),
                small("Cancel", AppMessage::Keybinds(Message::Close)),
            ]
            .align_y(Alignment::Center),
            new_binding_form(state, form),
        ]
        .spacing(12)
        .into();
    }
    let query = state.keybinds.search.trim().to_lowercase();
    let doubled = |binding: &niri_center::store::BindingRow| {
        binding.hidden_by.is_some() || !binding.replaces.is_empty()
    };
    let shows = |binding: &niri_center::store::BindingRow| {
        (!state.keybinds.only_doubled || doubled(binding))
            && state.keybinds.key_filter.as_deref().is_none_or(|filter| {
                niri_center::keyboard::same_key(
                    niri_center::keyboard::last_key(&binding.key),
                    filter,
                )
            })
            && (query.is_empty()
                || binding.key.to_lowercase().contains(&query)
                || binding.action.to_lowercase().contains(&query))
    };
    // worked out once: the heading counts them, the list shows them
    let listed: Vec<(usize, &niri_center::store::BindingRow)> = rows
        .iter()
        .enumerate()
        .filter(|(_, binding)| shows(binding))
        .collect();
    let shown = listed.len();
    let shown_key = state
        .keybinds
        .key_filter
        .as_deref()
        .map(|key| niri_center::keyboard::key_label(key).to_string());
    let heading = match (&shown_key, shown) {
        (Some(key), 0) => format!("Nothing on {key}"),
        (Some(key), n) => format!(
            "{} on {key}",
            niri_center::words::count(n, "binding", "bindings")
        ),
        (None, _) => "All bindings".to_string(),
    };
    let mut top = row![
        text(heading)
            .size(20)
            .font(rows::SEMIBOLD)
            .width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(key) = &state.keybinds.key_filter {
        top = top.push(small(
            "Show all",
            AppMessage::Keybinds(Message::KeyClicked(key.clone())),
        ));
    }
    let mut column = column![top].spacing(10).width(Length::Fill);
    if state.keybinds.key_filter.is_none() {
        column = column.push(
            text_input("Search bindings", &state.keybinds.search)
                .on_input(move |value| AppMessage::Keybinds(Message::Search(value)))
                .width(Length::Fill),
        );
    }
    // a key bound in more than one file: niri uses the binding it reads
    // last, and the others do nothing
    let hidden = rows.iter().filter(|row| row.hidden_by.is_some()).count();
    if (hidden > 0 || state.keybinds.only_doubled) && state.keybinds.key_filter.is_none() {
        let said = match hidden {
            0 => "No key is bound more than once now.".to_string(),
            1 => "1 binding is not used: its key is bound again in a file niri reads later."
                .to_string(),
            n => format!(
                "{n} bindings are not used: their keys are bound again in files niri reads later."
            ),
        };
        let only = state.keybinds.only_doubled;
        column = column.push(
            row![
                text(said)
                    .size(12)
                    .style(if hidden > 0 {
                        theme::pending
                    } else {
                        theme::muted
                    })
                    .width(Length::Fill),
                small(
                    if only { "Show all" } else { "Show them" },
                    AppMessage::Keybinds(Message::OnlyDoubled(!only)),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    let mut list = column![].spacing(2).width(Length::Fill);
    for (index, binding) in listed {
        if let Some(form) = state
            .keybinds
            .form
            .as_ref()
            .filter(|form| form.editing == Some(index))
        {
            list = list.push(edit_binding_form(state, form, index, binding));
            continue;
        }
        let name_file = home != Some(binding.file.as_str());
        list = list.push(binding_row(index, binding, rows, counts, name_file));
    }
    if shown == 0 {
        list = list.push(
            text(if shown_key.is_some() {
                "Nothing is bound to this key yet."
            } else if rows.is_empty() {
                "No bindings yet."
            } else {
                "No binding matches the search."
            })
            .size(13)
            .style(theme::muted),
        );
    }
    column = column.push(list);
    if let (Some(token), Some(key)) = (&state.keybinds.key_filter, &shown_key) {
        column = column.push(small(
            format!("Bind something to {key}"),
            AppMessage::Keybinds(Message::New(Some(token.clone()))),
        ));
    }
    if let Some(warnings) = include_warnings(state) {
        column = column.push(warnings);
    }
    column.into()
}

/// The layout the picture shows: the one picked here, else the one niri
/// types with, else the first configured, else xkb's `us`.
fn shown_layout(state: &State, xkb: &Xkb) -> String {
    state
        .layout
        .clone()
        .filter(|code| xkb.layouts.contains(code))
        .or_else(|| {
            state
                .niri_layout
                .and_then(|index| xkb.layouts.get(index))
                .cloned()
        })
        .or_else(|| xkb.layouts.first().cloned())
        .unwrap_or_else(|| "us".to_string())
}

/// Make the keyboard picture again when the config, the layout picked or
/// niri's layout moved, while the Keybinds page is open.
pub fn refresh_picture(state: &mut App) {
    if state.page != niri_center::page::Page::Keybinds {
        return;
    }
    let Some(store) = state.session.store.as_ref() else {
        return;
    };
    let keybinds = &mut state.keybinds;
    let made_from = (
        store.revision(),
        keybinds.layout.clone(),
        keybinds.niri_layout,
    );
    if keybinds
        .shown
        .as_ref()
        .is_some_and(|shown| shown.made_from == made_from)
    {
        return;
    }
    let xkb = store.xkb();
    let layout = shown_layout(keybinds, &xkb);
    let picture = Picture::of(&xkb, &layout);
    keybinds.shown = Some(Shown {
        made_from,
        found: picture.is_some(),
        picture: picture.unwrap_or_default(),
        layout,
    });
}

fn keyboard_picture<'app>(
    state: &'app App,
    store: &niri_center::store::Store,
    counts: HashMap<String, usize>,
) -> El<'app> {
    let Some(shown) = &state.keybinds.shown else {
        return space().into();
    };
    let xkb = store.xkb();
    let active = &shown.layout;
    let caption = match (shown.found, &xkb.file) {
        (false, _) => format!("There is no picture for the {active} layout, so this shows us."),
        (true, Some(_)) => {
            format!("Your config loads a keymap file: this shows xkb's {active} layout.")
        }
        (true, None) => "Numbers count the bindings that end on a key.".to_string(),
    };
    let mut line = Row::<AppMessage>::new()
        .push(
            text(caption)
                .size(12)
                .style(theme::muted)
                .width(Length::Fill),
        )
        .spacing(6)
        .align_y(Alignment::Center);
    // the configured layouts, by the names xkb gives them
    if xkb.layouts.len() > 1 {
        let named = |code: &String| {
            let name = state
                .system
                .layouts
                .iter()
                .find(|item| item.code == *code)
                .map_or_else(|| code.clone(), ToString::to_string);
            controls::Labeled {
                value: Some(code.clone()),
                label: match xkb.variant_of(code) {
                    "" => name,
                    variant => format!("{name}, {variant}"),
                },
            }
        };
        let choices: Vec<controls::Labeled> = xkb.layouts.iter().map(named).collect();
        let selected = choices
            .iter()
            .find(|choice| choice.value.as_ref() == Some(active))
            .cloned();
        line = line.push(
            pick_list(choices, selected, |choice: controls::Labeled| {
                AppMessage::Keybinds(Message::Layout(choice.value.unwrap_or_default()))
            })
            .text_size(12)
            .padding([3, 8]),
        );
    }
    column![
        keyboard::keyboard_canvas(&shown.picture, counts, state.keybinds.key_filter.clone()),
        line,
    ]
    .spacing(6)
    .into()
}

/// One niri action in the searchable picker: found by its name or by
/// words from niri's description of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionChoice {
    pub name: String,
    pub about: String,
}

impl std::fmt::Display for ActionChoice {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.about.is_empty() {
            out.write_str(&self.name)
        } else {
            write!(out, "{}: {}", self.name, self.about)
        }
    }
}

pub fn action_box(
    actions: &[niri_center::bindings::ActionInfo],
) -> iced::widget::combo_box::State<ActionChoice> {
    iced::widget::combo_box::State::new(
        actions
            .iter()
            .map(|action| ActionChoice {
                name: action.name.clone(),
                about: action.about.clone(),
            })
            .collect(),
    )
}

/// The searchable action picker, showing `current`.
fn action_picker<'app>(state: &'app App, current: Option<&str>) -> El<'app> {
    let selected = current.and_then(|name| {
        state
            .keybinds
            .actions
            .iter()
            .find(|action| action.name == name)
    });
    let placeholder = selected.map_or_else(
        || "Action: type to search".to_string(),
        |action| action.name.clone(),
    );
    // the chosen name sits in the placeholder: as the selection, the box
    // would show the whole "name: description" line
    iced::widget::combo_box(
        &state.keybinds.action_box,
        &placeholder,
        None,
        |choice: ActionChoice| AppMessage::Keybinds(Message::Action(choice.name)),
    )
    .width(Length::Fill)
    .into()
}

/// Modifier toggles and a hint while a combo is built on the keyboard.
fn compose_row(state: &App) -> El<'_> {
    let Some(mods) = &state.keybinds.composing else {
        return row![
            small(
                "Choose keys on the keyboard",
                AppMessage::Keybinds(Message::Compose(true))
            ),
            text("or type a combo such as Mod+Shift+T.")
                .size(12)
                .style(theme::muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into();
    };
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    for modifier in bindings::Modifier::COMPOSED {
        line = line.push(
            button(text(modifier.written()).size(12))
                .padding([3, 10])
                .style(if mods.contains(&modifier) {
                    theme::primary
                } else {
                    theme::secondary
                })
                .on_press(AppMessage::Keybinds(Message::ComposeModifier(modifier))),
        );
    }
    column![
        line.push(
            text("then click a key on the keyboard above.")
                .size(12)
                .style(theme::pending),
        )
        .push(space().width(Length::Fill))
        .push(small("Stop", AppMessage::Keybinds(Message::Compose(false))))
        .wrap(),
        text("Mod is Super unless the Input page sets another key.")
            .size(12)
            .style(theme::muted),
    ]
    .spacing(4)
    .into()
}

/// niri's description and argument syntax for `action`.
fn action_help(state: &App, action: Option<&str>) -> El<'static> {
    let Some(info) = action.and_then(|name| {
        state
            .keybinds
            .actions
            .iter()
            .find(|action| action.name == name)
    }) else {
        return space().into();
    };
    let usage = info.usage();
    let mut line = row![text(info.about.clone()).size(12).style(theme::muted)].spacing(10);
    if !usage.is_empty() {
        line = line.push(
            text(format!("{} {usage}", info.name))
                .size(12)
                .font(theme::MONO)
                .style(theme::code),
        );
    }
    line.into()
}

fn args_placeholder(state: &App, action: Option<&str>) -> String {
    match action.and_then(|name| {
        state
            .keybinds
            .actions
            .iter()
            .find(|action| action.name == name)
    }) {
        None => "Arguments".to_string(),
        Some(info) if info.arguments.is_empty() => "No arguments".to_string(),
        Some(info) => info.usage(),
    }
}

/// The binding form's fields: the key, the action and its arguments.
fn binding_fields<'app>(state: &'app App, form: &niri_center::forms::BindingForm) -> [El<'app>; 3] {
    let action = form.action.as_deref();
    [
        text_input("Mod+T", &form.key)
            .on_input(move |value| AppMessage::Keybinds(Message::Key(value)))
            .width(Length::FillPortion(2))
            .into(),
        container(action_picker(state, action))
            .width(Length::FillPortion(3))
            .into(),
        text_input(&args_placeholder(state, action), &form.args)
            .on_input(move |value| AppMessage::Keybinds(Message::Args(value)))
            .on_submit(AppMessage::Keybinds(Message::Save))
            .width(Length::FillPortion(3))
            .into(),
    ]
}

fn new_binding_form<'app>(state: &'app App, form: &niri_center::forms::BindingForm) -> El<'app> {
    let action = form.action.as_deref();
    let [key, picker, args] = binding_fields(state, form);
    let add = button(text("Add binding").size(13))
        .style(theme::primary)
        .on_press(AppMessage::Keybinds(Message::Save));
    // two lines: the form sits in the column beside the keyboard
    let fields: El<'_> = column![
        row![key, picker].spacing(10),
        row![args, add].spacing(10).align_y(Alignment::Center),
    ]
    .spacing(8)
    .into();
    rows::group(
        "New binding".to_string(),
        vec![pad(column![
            compose_row(state),
            fields,
            action_help(state, action)
        ]
        .spacing(8)
        .into())],
    )
}

/// One binding: its key, and under it the action on a line of its own, so
/// it never shrinks below a word. `name_file` tags it with its file.
fn binding_row(
    index: usize,
    binding: &niri_center::store::BindingRow,
    rows: &[niri_center::store::BindingRow],
    counts: &HashMap<String, usize>,
    name_file: bool,
) -> El<'static> {
    let key = text(binding.key.replace('+', "+\u{200b}"))
        .size(14)
        .font(theme::MONO)
        .wrapping(text::Wrapping::WordOrGlyph)
        .width(Length::Fill);
    // a binding niri does not use (off, or replaced later) reads muted
    let key = if binding.disabled || binding.hidden_by.is_some() {
        key.style(theme::muted)
    } else {
        key.style(theme::accent)
    };
    let action: El<'static> = if binding.action.chars().count() > ACTION_PREVIEW_CHARS {
        tooltip(
            text(niri_center::words::shortened(
                binding.action.clone(),
                ACTION_PREVIEW_CHARS,
            ))
            .size(14)
            .wrapping(text::Wrapping::WordOrGlyph),
            container(
                text(binding.action.clone())
                    .size(12)
                    .wrapping(text::Wrapping::WordOrGlyph),
            )
            .max_width(520)
            .padding(8)
            .style(theme::group),
            tooltip::Position::Bottom,
        )
        .into()
    } else {
        text(binding.action.clone())
            .size(14)
            .wrapping(text::Wrapping::WordOrGlyph)
            .into()
    };
    let last_key = niri_center::keyboard::last_key(&binding.key);
    let shared = counts.get(&key_id(last_key)).copied().unwrap_or(0);
    let mut details = Row::<AppMessage>::new()
        .spacing(8)
        .align_y(Alignment::Center);
    if binding.disabled {
        details = details.push(tag("Off"));
    }
    if shared > 1 {
        details = details.push(tag(format!("{shared} bindings use {last_key}")));
    }
    if name_file {
        details = details.push(tag(binding.file.clone()));
    }
    let content: El<'static> = column![
        row![key, details].spacing(10).align_y(Alignment::Center),
        container(action).padding(iced::Padding {
            left: 12.0,
            ..iced::Padding::ZERO
        }),
    ]
    .spacing(4)
    .padding([6, 0])
    .width(Length::Fill)
    .into();
    // niri reads a key bound in several files from the last one
    // the other bindings of the same key, to say what each does: worked out
    // only for a key bound more than once
    let same_key = || {
        let combo = KeyCombo::parse(&binding.key);
        move |other: &&niri_center::store::BindingRow| {
            !other.disabled && KeyCombo::parse(&other.key) == combo
        }
    };
    let overrides = match &binding.hidden_by {
        Some(later) => {
            let used = rows
                .iter()
                .filter(same_key())
                .find(|other| other.hidden_by.is_none())
                .map_or_else(String::new, |used| format!(", to {}", used.action));
            Some(
                text(format!("Not used: {later} binds this key later{used}."))
                    .size(12)
                    .style(theme::pending),
            )
        }
        None if !binding.replaces.is_empty() => {
            let replaced: Vec<String> = rows
                .iter()
                .filter(same_key())
                .filter(|other| other.hidden_by.is_some())
                .map(|other| format!("{} in {}", other.action, other.file))
                .collect();
            Some(
                text(format!("Used instead of {}.", replaced.join(", ")))
                    .size(12)
                    .style(theme::pending),
            )
        }
        None => None,
    };
    let content: El<'static> = match overrides {
        Some(line) => column![content, line].spacing(2).into(),
        None => content,
    };
    button(content)
        .padding([2, 6])
        .width(Length::Fill)
        .style(theme::list_row)
        .on_press(AppMessage::Keybinds(Message::Edit(index)))
        .into()
}

/// Inline form that replaces one binding row while it is being edited.
/// Only the key and the action change; props on the line are kept.
fn edit_binding_form<'app>(
    state: &'app App,
    form: &niri_center::forms::BindingForm,
    index: usize,
    binding: &niri_center::store::BindingRow,
) -> El<'app> {
    let [key, picker, args] = binding_fields(state, form);
    container(
        column![
            compose_row(state),
            row![key, picker, args]
                .spacing(8)
                .align_y(Alignment::Center),
            action_help(state, form.action.as_deref()),
            // as every dialog: what acts on the binding at the left, Save
            // and the way out last at the right
            row![
                small(
                    on_off_label(binding.disabled),
                    AppMessage::Keybinds(Message::On(index, binding.disabled))
                ),
                small("Remove", AppMessage::Keybinds(Message::Remove(index))),
                space().width(Length::Fill),
                button(text("Save").size(12))
                    .padding([3, 10])
                    .style(theme::primary)
                    .on_press(AppMessage::Keybinds(Message::Save)),
                small("Cancel", AppMessage::Keybinds(Message::Close)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(8),
    )
    .padding(10)
    .style(theme::group)
    .into()
}
