//! A setting's row, as every settings page draws it from the table in
//! `niri_center::settings`: label, help, the `file:line` it is written at,
//! the control, and after a change what the value was.

use iced::widget::{column, container, row, rule, text};
use iced::{Alignment, Font, Length};
use niri_center::page::Page;
use niri_center::settings::{
    DraftKey, Edit, Note, OutputSetting, Reading, Section, Setting, Spec, line_link, notes,
    output_chain, read, unset,
};
use niri_center::store::{Block, Store};
use niri_center::summary::shown;

use crate::app::{App, room};
use crate::message::Message;
use crate::{El, controls, theme};

/// Every registry section of `page`, as grouped rows.
pub fn sections(state: &App, page: Page) -> El<'_> {
    let width = state.content_width();
    let mut groups: Vec<(Option<Section>, Vec<El<'_>>)> = Vec::new();
    let mut previous_parent = None;
    for &setting in Setting::ALL
        .iter()
        .filter(|setting| setting.spec().page == page)
    {
        let spec = setting.spec();
        // rows that depend on the same setting say so once, on the first
        let explain_parent = spec.requires != previous_parent;
        previous_parent = spec.requires;
        let row = setting_row(state, setting, width, explain_parent);
        match groups.last_mut() {
            Some((section, rows)) if *section == spec.section => rows.push(row),
            Some(_) | None => groups.push((spec.section, vec![row])),
        }
    }
    let mut sections = column![].spacing(24).width(Length::Fill);
    for (index, (section, rows)) in groups.into_iter().enumerate() {
        // the page title already names a first section of the same name
        let heading = match section {
            Some(section) if index > 0 || section.title() != page.label() => section.title(),
            Some(_) | None => "",
        };
        sections = sections.push(group(heading.to_string(), rows));
    }
    sections.into()
}

/// A heading (or none) and its rows on one surface, hairlines between rows.
pub fn group(heading: String, rows: Vec<El<'_>>) -> El<'_> {
    let mut body = column![].width(Length::Fill);
    let count = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        body = body.push(row);
        if index + 1 < count {
            body = body.push(rule::horizontal(1).style(rule::weak));
        }
    }
    let surface = container(body).width(Length::Fill).style(theme::group);
    if heading.is_empty() {
        return surface.into();
    }
    column![text(heading).size(14).font(SEMIBOLD), surface]
        .spacing(6)
        .width(Length::Fill)
        .into()
}

pub const SEMIBOLD: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::DEFAULT
};

/// `explain_parent`: say which setting this one depends on, when that is off.
fn setting_row(state: &App, id: Setting, width: f32, explain_parent: bool) -> El<'_> {
    let Some(store) = state.store() else {
        return iced::widget::space().into();
    };
    let setting = id.spec();
    let off_parent = setting
        .requires
        .map(Setting::spec)
        .filter(|parent| !parent.is_on(store));
    spec_row(
        state,
        setting,
        RowParts {
            chain: setting.chain,
            key: DraftKey::Setting(id),
            help: setting.help.to_string(),
            notes: notes(
                store,
                &state.system,
                setting,
                setting.chain,
                off_parent.filter(|_| explain_parent),
            ),
            dimmed: off_parent.is_some(),
            highlighted: state.highlight == Some(id),
            set: false,
            control: None,
            width,
        },
        move |edit| Message::Setting(id, edit),
    )
}

/// One display's setting as a row `width` wide. `extra` adds what the
/// Displays page knows beyond the table, such as the display's size and
/// modes.
pub fn output_row<'app>(
    state: &'app App,
    output: &str,
    id: OutputSetting,
    extra: Extra<'app>,
    width: f32,
) -> El<'app> {
    let setting = id.spec();
    let Some(store) = state.store() else {
        return iced::widget::space().into();
    };
    let Extra {
        help,
        notes: more,
        control,
    } = extra;
    let chain = output_chain(output);
    let key = DraftKey::Output(output.to_string(), id);
    let mut notes = notes(store, &state.system, setting, &chain, None);
    notes.extend(more);
    let name = output.to_string();
    spec_row(
        state,
        setting,
        RowParts {
            chain: &chain,
            key,
            help: help.unwrap_or_else(|| setting.help.to_string()),
            notes,
            dimmed: false,
            highlighted: false,
            set: false,
            control,
            width,
        },
        move |edit| Message::Output(name.clone(), id, edit),
    )
}

/// What a page says or offers in a display's row beyond the table.
#[derive(Default)]
pub struct Extra<'view> {
    pub help: Option<String>,
    pub notes: Vec<Note>,
    pub control: Option<El<'view>>,
}

/// What a row shows besides its setting: where the value lives, and the
/// words and control a page may give instead of the table's own.
struct RowParts<'chain, 'view> {
    chain: &'chain [Block<'chain>],
    /// Key for drafts and their errors.
    key: DraftKey,
    help: String,
    notes: Vec<Note>,
    dimmed: bool,
    highlighted: bool,
    /// Whether the config sets the value, rather than niri's default.
    set: bool,
    control: Option<El<'view>>,
    /// The width the row has.
    width: f32,
}

/// A setting's row: its value read where it lives, what it was before the
/// latest change, and a reset to niri's default.
fn spec_row<'app>(
    state: &'app App,
    setting: &'static Spec,
    mut parts: RowParts<'_, 'app>,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'app> {
    let Some(store) = state.store() else {
        return iced::widget::space().into();
    };
    let read_in = |store: &Store| read(store, parts.chain, &setting.child, setting.kind);
    let reading = read_in(store);
    parts.set = reading != unset(setting.kind);
    let was = state
        .session
        .saved
        .as_ref()
        .map(read_in)
        .filter(|before| *before != reading)
        .map(|before| format!("Was {}", shown(setting, &before)));
    let reset = matches!(reading, Reading::Value(Some(_))).then(|| {
        let default = if setting.default.is_empty() {
            String::new()
        } else {
            format!(", {}", setting.default)
        };
        (
            make(Edit::Unset),
            format!("Use niri's default{default}. Takes it out of the config."),
        )
    });
    let control = parts.control.take().unwrap_or_else(|| {
        controls::control(
            state,
            &parts.key,
            setting.kind,
            setting.default.as_str(),
            reading,
            make,
        )
    });
    let path = line_link(store, parts.chain, &setting.child);
    row_frame(state, setting, parts, control, path, was, reset)
}

/// The size of a row's facts: what it was, where it is written.
const FACTS_SIZE: f32 = 12.0;

/// A setting row: label, help, where it is written (`path`), and the
/// control on the right, or under the label when it needs the room or the
/// row is narrow. After a change it is tinted amber and says what the
/// value `was`. A value written in the config has a `reset` button beside
/// the control that takes it out again.
fn row_frame<'app>(
    state: &App,
    setting: &Spec,
    parts: RowParts<'_, 'app>,
    control: El<'app>,
    path: String,
    was: Option<String>,
    reset: Option<(Message, String)>,
) -> El<'app> {
    let narrow = parts.width < room::CONTROL_BESIDE_LABEL;
    let stacked = controls::is_wide(state, &parts.key, setting.kind)
        || (narrow && !controls::fits_beside(setting.kind));
    let label = text(setting.label.as_str()).size(14);
    let label = if parts.dimmed {
        label.style(theme::muted)
    } else {
        label
    };
    let mut label = column![label].spacing(2);
    if !parts.help.is_empty() {
        label = label.push(text(parts.help).size(12).style(theme::muted));
    }
    let changed = was.is_some();
    // one line high even while empty, so the first change to a setting
    // does not push every row below it down
    let mut facts = row![]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(text::LineHeight::default().to_absolute(FACTS_SIZE.into()));
    if let Some(was) = was {
        facts = facts.push(text(was).size(FACTS_SIZE).style(theme::pending));
    }
    if !path.is_empty() {
        facts = facts.push(
            text(path)
                .size(FACTS_SIZE)
                .font(theme::MONO)
                .style(theme::muted),
        );
    }
    label = label.push(facts);
    for note in parts.notes {
        label = label.push(text(note.text).size(12).style(if note.warn {
            theme::pending
        } else {
            theme::muted
        }));
    }
    if let Some(error) = state.session.drafts.error(&parts.key) {
        label = label.push(text(error.clone()).size(12).style(theme::error));
    }
    // the reset sits before the control, so every control in a group ends
    // at the same edge whether or not its value can be taken out
    let reset: Option<El<'app>> = reset.map(|(message, what)| {
        controls::icon_button(crate::icons::Icon::Reset, 14.0, what, message)
    });
    // narrow windows (a third or half of the screen is common in niri) and
    // controls that need the room stack the control under its label
    let line: El<'app> = if stacked {
        let mut under = row![control].spacing(4).align_y(Alignment::Center);
        if let Some(reset) = reset {
            under = under.push(reset);
        }
        column![label, under]
            .spacing(8)
            .padding([10, 12])
            .width(Length::Fill)
            .into()
    } else {
        let control = if narrow {
            container(control)
        } else {
            container(control).width(220).align_right(220)
        };
        let mut beside = row![label.width(Length::Fill)]
            .spacing(8)
            .padding([10, 12])
            .align_y(Alignment::Center);
        if let Some(reset) = reset {
            beside = beside.push(reset);
        }
        beside.push(control).into()
    };
    if changed {
        // tinted, with niri's amber at the left edge
        return container(
            row![
                container(iced::widget::space().width(3).height(Length::Fill))
                    .height(Length::Fill)
                    .style(theme::changed_bar),
                line,
            ]
            .height(Length::Shrink),
        )
        .width(Length::Fill)
        .style(theme::changed_row)
        .into();
    }
    // set in the config: a quiet bar in the accent color at the left edge,
    // to find what the config sets at a glance (KDE's "highlight changed
    // settings", always on). Every row keeps the bar's room, so labels
    // line up whether or not it shows.
    let edge = container(iced::widget::space().width(3).height(Length::Fill)).height(Length::Fill);
    let line: El<'app> = row![
        if parts.set {
            edge.style(theme::set_bar)
        } else {
            edge
        },
        line,
    ]
    .height(Length::Shrink)
    .into();
    let frame = container(line).width(Length::Fill);
    if parts.highlighted {
        frame
            .style(|theme: &iced::Theme| container::Style {
                border: iced::Border {
                    color: theme::colors(theme).accent,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    } else {
        frame.into()
    }
}
