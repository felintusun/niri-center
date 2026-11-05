//! The widget that edits each kind of setting value.
//!
//! Every control reports through `make`, which turns an [`Edit`] into the
//! message for its setting, so the same widgets serve registry rows and the
//! per-output rows on the Displays page. Values that need the room (lists,
//! gradients, an open color picker) sit under their label instead of beside
//! it; see [`is_wide`].

use iced::widget::{
    Row, button, canvas, checkbox, column, combo_box, container, pick_list, row, slider, space,
    text, text_input,
};
use iced::{Alignment, Color, Font, Length, Point, Rectangle, Renderer, Size, Theme, mouse};
use num_traits::ToPrimitive;

use niri_center::color::{Gradient, GradientEnd};
use niri_center::settings::DraftKey;
use niri_center::settings::{
    Axis, Edit, Kind, NIRI_DEFAULT, NO_FILE, OnOff, Reading, join_sizes, size_label, sizes,
};
use niri_center::settings::{default_label, default_placeholder};
use niri_center::words::{
    comma_list, expand_strftime, join_path, short_fraction, short_number, split_path,
};

use niri_center::system;

use crate::app::{App, ColorField};
use crate::message::Message;
use crate::{El, color, theme};

/// Stand-in for a value a picker or file dialog fills in later.
pub const HOLE: char = '\u{0}';

/// Where a picked color or a chosen path goes: a field, by its draft key,
/// and the text it will hold, with [`HOLE`] where the value goes. Without
/// a hole the value is the whole text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Field(DraftKey, String),
    /// The command of the autostart entry being added.
    StartupCommand(String),
}

impl Destination {
    /// The message that puts `value` there.
    pub fn filled(&self, value: &str) -> Message {
        let fill = |text: &str| {
            if text.contains(HOLE) {
                text.replace(HOLE, value)
            } else {
                value.to_string()
            }
        };
        match self {
            Self::Field(DraftKey::Setting(id), text) => {
                Message::Setting(*id, Edit::Draft(fill(text)))
            }
            Self::Field(DraftKey::Output(output, child), text) => {
                Message::Output(output.clone(), *child, Edit::Draft(fill(text)))
            }
            Self::Field(DraftKey::Position(output, coordinate), text) => Message::Displays(
                crate::pages::displays::Message::Position(output.clone(), *coordinate, fill(text)),
            ),
            Self::StartupCommand(text) => {
                Message::Startup(crate::pages::startup::Message::New(fill(text)))
            }
        }
    }
}

/// A pick-list entry: what gets written (`None` = niri's default) and what
/// the user reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labeled<T = String> {
    pub value: Option<T>,
    pub label: String,
}

impl<T> std::fmt::Display for Labeled<T> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.label)
    }
}

/// Whether the control for `kind` goes under its label rather than beside it.
pub fn is_wide(state: &App, key: &DraftKey, kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Sizes { many: true, .. }
            | Kind::Layouts
            | Kind::XkbOptions
            | Kind::Screenshot
            | Kind::Gradient
            | Kind::File { .. }
    ) || state
        .color
        .as_ref()
        .is_some_and(|open| open.field.key == *key)
}

/// Whether a control of `kind` stays beside its label in a narrow row: a
/// switch does, as in GNOME and KDE.
pub const fn fits_beside(kind: Kind) -> bool {
    matches!(kind, Kind::Switch | Kind::Enabled)
}

/// The control for a value of `kind` whose current reading is `reading`;
/// `make` turns an edit into the message to send.
pub fn control<'app>(
    state: &'app App,
    key: &DraftKey,
    kind: Kind,
    default: &'app str,
    reading: Reading,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'app> {
    let drafted = |current: Option<String>| {
        state
            .session
            .drafts
            .typed(key)
            .cloned()
            .or(current)
            .unwrap_or_default()
    };
    match (kind, reading) {
        (Kind::Switch | Kind::Enabled, Reading::Switch(on)) => {
            crate::motion::switch(on, state.motions.switch)
                .on_toggle(move |on| make(Edit::Switch(on)))
                .into()
        }
        (Kind::OnOff, Reading::OnOff(current)) => on_off_list(current, default, make),
        (Kind::Choice(options), Reading::Value(current)) => {
            let labeled: Vec<(String, String)> = options
                .iter()
                .map(|(value, label)| ((*value).to_string(), (*label).to_string()))
                .collect();
            let shown_default = kind.word(default).unwrap_or(default);
            choice_list(
                labeled,
                current.as_deref(),
                &default_label(shown_default),
                make,
            )
        }
        (Kind::Bool, Reading::Value(current)) => choice_list(
            vec![
                ("true".to_string(), "On".to_string()),
                ("false".to_string(), "Off".to_string()),
            ],
            current.as_deref(),
            &default_label(kind.word(default).unwrap_or(default)),
            make,
        ),
        (Kind::Color, Reading::Value(current)) => {
            color_field(state, key, &drafted(current), default, make)
        }
        (Kind::Gradient, Reading::Value(current)) => gradient_field(
            state,
            key,
            state
                .session
                .drafts
                .typed(key)
                .map(String::as_str)
                .or(current.as_deref()),
            default,
            make,
        ),
        (Kind::Sizes { axis, many: true }, Reading::Value(current)) => {
            size_list(state, key, axis, current, default, make)
        }
        (Kind::Sizes { many: false, .. }, Reading::Value(current)) => {
            default_size(state, current.as_deref(), default, make)
        }
        (Kind::Layouts, Reading::Value(current)) => layouts_field(state, &drafted(current), make),
        (Kind::XkbOptions, Reading::Value(current)) => {
            options_field(state, &drafted(current), make)
        }
        (Kind::CursorTheme, Reading::Value(current)) => {
            let themes = state
                .system
                .cursor_themes
                .iter()
                .map(|theme_name| (theme_name.clone(), theme_name.clone()))
                .collect();
            choice_list(themes, current.as_deref(), &default_label(default), make)
        }
        (Kind::Output, Reading::Value(current)) => {
            let outputs = state
                .outputs
                .iter()
                .map(|output| {
                    let label = if output.description.is_empty() {
                        output.name.clone()
                    } else {
                        format!("{} ({})", output.name, output.description)
                    };
                    (output.name.clone(), label)
                })
                .collect();
            choice_list(outputs, current.as_deref(), &default_label(default), make)
        }
        (Kind::File { example }, Reading::Value(current)) => {
            file_field(key, &drafted(current), example.as_str(), default, make)
        }
        (Kind::Screenshot, Reading::Value(current)) => {
            screenshot_field(state, key, &drafted(current), default, make)
        }
        (Kind::Float { min, max }, Reading::Value(current)) if max - min <= 2.0 => {
            slider_field(&drafted(current), min, max, default, make)
        }
        (_, Reading::Value(current)) => {
            let example = if let Kind::Text { example } | Kind::Prop { example, .. } = kind {
                example.as_str()
            } else {
                ""
            };
            let placeholder = if !default.is_empty() {
                default_placeholder(default)
            } else if !example.is_empty() {
                example.to_string()
            } else {
                NIRI_DEFAULT.to_string()
            };
            text_input(&placeholder, &drafted(current))
                .on_input(move |input| make(Edit::Draft(input)))
                .width(Length::Fill)
                .into()
        }
        // a reading that does not fit its kind cannot be produced by `read`
        (_, Reading::Switch(_) | Reading::OnOff(_)) => space().into(),
    }
}

fn on_off_list(
    current: OnOff,
    default: &str,
    make: impl Fn(Edit) -> Message + 'static,
) -> El<'static> {
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Item(OnOff, String);
    impl std::fmt::Display for Item {
        fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            out.write_str(&self.1)
        }
    }
    let items = vec![
        Item(
            OnOff::Default,
            default_label(match default {
                "on" => "On",
                "off" => "Off",
                other => other,
            }),
        ),
        Item(OnOff::On, "On".to_string()),
        Item(OnOff::Off, "Off".to_string()),
    ];
    let selected = items.iter().find(|item| item.0 == current).cloned();
    pick_list(items, selected, move |item| make(Edit::OnOff(item.0)))
        .width(Length::Fill)
        .into()
}

/// A pick list of `(value, label)` options with niri's default first (it
/// removes the setting). A current value missing from the options is kept,
/// so the list never hides what the config says.
pub fn choice_list(
    options: Vec<(String, String)>,
    current: Option<&str>,
    default: &str,
    make: impl Fn(Edit) -> Message + 'static,
) -> El<'static> {
    let mut items = vec![Labeled {
        value: None,
        label: default.to_string(),
    }];
    items.extend(options.into_iter().map(|(value, label)| Labeled {
        value: Some(value),
        label,
    }));
    if let Some(value) = current
        && !items
            .iter()
            .any(|item| item.value.as_deref() == Some(value))
    {
        items.push(Labeled {
            value: Some(value.to_string()),
            label: value.to_string(),
        });
    }
    let selected = items
        .iter()
        .find(|item| item.value.as_deref() == current)
        .cloned();
    pick_list(items, selected, move |item| make(Edit::Choice(item.value)))
        .width(Length::Fill)
        .into()
}

/// A number with a slider for settings with a short range.
fn slider_field(
    value: &str,
    min: f64,
    max: f64,
    default: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let position = value
        .parse::<f64>()
        .ok()
        .or_else(|| default.parse().ok())
        .unwrap_or(min)
        .clamp(min, max);
    let on_slide = make.clone();
    row![
        slider(min..=max, position, move |value| on_slide(Edit::Draft(
            short_number(value)
        )))
        .step(0.01),
        text_input(
            if default.is_empty() {
                "default"
            } else {
                default
            },
            value
        )
        .on_input(move |input| make(Edit::Draft(input)))
        .width(72),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

/// A square of `color` with a hairline edge.
fn swatch(color: Option<Color>, size: f32) -> El<'static> {
    container(space().width(size).height(size))
        .style(move |theme: &Theme| {
            let colors = theme::colors(theme);
            container::Style {
                background: color.map(iced::Background::Color),
                border: iced::Border {
                    color: colors.line,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// A swatch that opens the picker for `field`, the picked color going to
/// `destination`.
fn swatch_button(color: Option<Color>, field: ColorField, destination: Destination) -> El<'static> {
    button(swatch(color, 22.0))
        .padding(0)
        .style(button::text)
        .on_press(Message::OpenColor(field, color, destination))
        .into()
}

fn color_field(
    state: &App,
    key: &DraftKey,
    value: &str,
    default: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let shown = color::parse(value).or_else(|| {
        value
            .trim()
            .is_empty()
            .then(|| color::parse(default))
            .flatten()
    });
    let placeholder = if default.is_empty() {
        NIRI_DEFAULT.to_string()
    } else {
        default_placeholder(default)
    };
    let target = Destination::Field(key.clone(), HOLE.to_string());
    let line = row![
        swatch_button(
            shown,
            ColorField {
                key: key.clone(),
                end: None,
            },
            target,
        ),
        text_input(&placeholder, value)
            .on_input({
                let make = make.clone();
                move |input| make(Edit::Draft(input))
            })
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    match &state.color {
        Some(open) if open.field.key == *key && open.field.end.is_none() => {
            column![line, picker_panel(open.hsva, make)]
                .spacing(10)
                .into()
        }
        Some(_) | None => line.into(),
    }
}

/// The picker under an open color field: the canvas, a few colors to start
/// from, picking from the screen through niri, and Done.
fn picker_panel(
    hsva: color::Hsva,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let mut swatches = Row::new().spacing(6).align_y(Alignment::Center);
    for hex in color::SWATCHES {
        let make = make.clone();
        swatches = swatches.push(
            button(swatch(color::parse(hex), 18.0))
                .padding(0)
                .style(button::text)
                .on_press(make(Edit::Draft(hex.to_string()))),
        );
    }
    container(
        column![
            color::picker(hsva),
            swatches,
            row![
                text(hsva.hex())
                    .size(12)
                    .font(theme::MONO)
                    .width(Length::Fill),
                button(text("Pick from screen").size(12))
                    .padding([3, 10])
                    .style(theme::secondary)
                    .on_press(Message::PickScreenColor),
                button(text("Done").size(12))
                    .padding([3, 10])
                    .style(theme::primary)
                    .on_press(Message::CloseColor),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fill)
    .max_width(420)
    .style(theme::code_block)
    .into()
}

/// What niri says about a config it cannot read, as it says it.
pub fn diagnostics(words: &str) -> El<'static> {
    container(
        text(words.trim_end().to_string())
            .size(12)
            .font(theme::MONO),
    )
    .padding(10)
    .width(Length::Fill)
    .style(theme::code_block)
    .into()
}

fn gradient_field(
    state: &App,
    key: &DraftKey,
    current: Option<&str>,
    default: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let Some(gradient) = current.and_then(Gradient::parse) else {
        return button(text("Add a gradient").size(12))
            .padding([3, 10])
            .style(theme::secondary)
            .on_press(make(Edit::Draft(default.to_string())))
            .into();
    };
    let end = |end: GradientEnd| {
        let value = gradient.end(end);
        let field = ColorField {
            key: key.clone(),
            end: Some(end),
        };
        let hole = gradient.with_end(end, HOLE.to_string());
        let typed = {
            let make = make.clone();
            let gradient = gradient.clone();
            move |input: String| make(Edit::Draft(gradient.with_end(end, input).text()))
        };
        let line = row![
            text(end.label()).size(13).width(48),
            swatch_button(
                color::parse(value),
                field.clone(),
                Destination::Field(key.clone(), hole.text())
            ),
            text_input("#7fc8ff", value)
                .on_input(typed)
                .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        match &state.color {
            Some(open) if open.field == field => {
                let make = make.clone();
                let gradient = gradient.clone();
                let into_end = move |edit: Edit| match edit {
                    Edit::Draft(color) => make(Edit::Draft(gradient.with_end(end, color).text())),
                    other @ (Edit::Switch(_) | Edit::OnOff(_) | Edit::Choice(_) | Edit::Unset) => {
                        make(other)
                    }
                };
                column![line, picker_panel(open.hsva, into_end)]
                    .spacing(8)
                    .into()
            }
            Some(_) | None => El::from(line),
        }
    };
    let angle = {
        let make = make.clone();
        let gradient = gradient.clone();
        move |input: String| {
            make(Edit::Draft(
                Gradient {
                    angle: input,
                    ..gradient.clone()
                }
                .text(),
            ))
        }
    };
    let across = {
        let make = make.clone();
        let gradient = gradient.clone();
        move |on: bool| {
            make(Edit::Draft(
                Gradient {
                    across_view: on,
                    ..gradient.clone()
                }
                .text(),
            ))
        }
    };
    column![
        end(GradientEnd::From),
        end(GradientEnd::To),
        row![
            text("Angle").size(13).width(48),
            text_input("180", &gradient.angle).on_input(angle).width(72),
            checkbox(gradient.across_view)
                .label("One gradient across all windows")
                .text_size(13)
                .on_toggle(across),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        button(text("Remove the gradient").size(12))
            .padding([3, 10])
            .style(theme::secondary)
            .on_press(make(Edit::Choice(None))),
    ]
    .spacing(8)
    .into()
}

/// The logical size of the screen sizes are shown against: the first
/// display niri reports, or a common laptop screen.
fn screen(state: &App) -> Size {
    state
        .displays
        .monitors
        .first()
        .map_or(Size::new(1920.0, 1200.0), |(_, m)| {
            // screen sizes are far inside f32's range
            Size::new(
                m.width.to_f32().unwrap_or(1920.0),
                m.height.to_f32().unwrap_or(1200.0),
            )
        })
}

/// What part of `extent` a size takes.
fn fraction(name: &str, value: &str, extent: f32) -> f32 {
    match (name, value.parse::<f32>()) {
        ("proportion", Ok(proportion)) => proportion,
        ("fixed", Ok(px)) => px / extent.max(1.0),
        _ => 0.0,
    }
    .clamp(0.0, 1.0)
}

fn size_list(
    state: &App,
    key: &DraftKey,
    axis: Axis,
    current: Option<String>,
    default: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let set = current.is_some();
    let text_now = state
        .session
        .drafts
        .typed(key)
        .cloned()
        .or(current)
        .unwrap_or_else(|| default.to_string());
    let entries = sizes(&text_now);
    let screen = screen(state);
    let extent = match axis {
        Axis::Width => screen.width,
        Axis::Height => screen.height,
    };
    let mut list = column![SizesPreview::canvas(&entries, axis, screen)].spacing(6);
    for (index, (name, value)) in entries.iter().enumerate() {
        let with = {
            let entries = entries.clone();
            move |entry: (String, String)| {
                let mut next = entries.clone();
                if let Some(slot) = next.get_mut(index) {
                    *slot = entry;
                }
                join_sizes(&next)
            }
        };
        let kinds = vec![
            Labeled {
                value: Some("proportion".to_string()),
                label: "Part of the screen".to_string(),
            },
            Labeled {
                value: Some("fixed".to_string()),
                label: "Fixed pixels".to_string(),
            },
        ];
        let selected = kinds
            .iter()
            .find(|known| known.value.as_deref() == Some(name.as_str()))
            .cloned();
        let switch_kind = {
            let make = make.clone();
            let (name, value) = (name.clone(), value.clone());
            let with = with.clone();
            move |picked: Labeled| {
                let picked = picked.value.unwrap_or_default();
                let converted = match (name.as_str(), picked.as_str(), value.parse::<f32>()) {
                    ("proportion", "fixed", Ok(proportion)) => {
                        format!("{}", (proportion * extent).round())
                    }
                    ("fixed", "proportion", Ok(px)) => {
                        short_fraction((px / extent.max(1.0)).clamp(0.01, 1.0))
                    }
                    _ => value.clone(),
                };
                make(Edit::Draft(with((picked, converted))))
            }
        };
        let typed = {
            let make = make.clone();
            let name = name.clone();
            let with = with.clone();
            move |input: String| make(Edit::Draft(with((name.clone(), input))))
        };
        let mut remaining = entries.clone();
        remaining.remove(index);
        let mut line = row![
            pick_list(kinds, selected, switch_kind).width(170),
            text_input("0.5", value).on_input(typed).width(90),
            text(size_label(name, value))
                .size(12)
                .style(theme::muted)
                .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if entries.len() > 1 {
            line = line.push(remove_button(
                format!("Remove {}", size_label(name, value)),
                make(Edit::Draft(join_sizes(&remaining))),
            ));
        }
        list = list.push(line);
    }
    let mut add = Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .push(text("Add").size(12).style(theme::muted));
    for (label, value) in [
        ("1/3", "0.33333"),
        ("1/2", "0.5"),
        ("2/3", "0.66667"),
        ("Full", "1"),
    ] {
        let mut next = entries.clone();
        next.push(("proportion".to_string(), value.to_string()));
        add = add.push(small(label, make(Edit::Draft(join_sizes(&next)))));
    }
    let mut fixed = entries.clone();
    fixed.push(("fixed".to_string(), format!("{}", (extent / 2.0).round())));
    add = add.push(small("Fixed size", make(Edit::Draft(join_sizes(&fixed)))));
    if set {
        add = add
            .push(space().width(Length::Fill))
            .push(small("Use niri's defaults", make(Edit::Choice(None))));
    }
    list.push(add.wrap()).into()
}

fn default_size(
    state: &App,
    current: Option<&str>,
    default: &str,
    make: impl Fn(Edit) -> Message + 'static,
) -> El<'static> {
    let label_of = |text: &str| {
        let entries = sizes(text);
        entries.first().map_or_else(
            || "The app decides".to_string(),
            |(name, value)| size_label(name, value),
        )
    };
    let presets = state
        .store()
        .and_then(|store| {
            store.sizes(
                &[niri_center::store::Block::named("layout")],
                "preset-column-widths",
            )
        })
        .unwrap_or_else(|| {
            sizes(
                niri_center::settings::Setting::LayoutPresetColumnWidths
                    .spec()
                    .default
                    .as_str(),
            )
        });
    let mut options = vec![(String::new(), "The app decides".to_string())];
    for (name, value) in presets {
        let entry = format!("{name} {value}");
        options.push((entry.clone(), size_label(&name, &value)));
    }
    let mut items: Vec<(String, String)> = Vec::new();
    for (value, label) in options {
        if !items.iter().any(|(v, _)| *v == value) {
            items.push((value, label));
        }
    }
    if let Some(value) = &current
        && !items.iter().any(|(v, _)| v == value)
    {
        items.push((value.to_string(), label_of(value)));
    }
    choice_list(items, current, &default_label(&label_of(default)), make)
}

/// An × that removes one entry of a list; the tooltip says which.
pub fn remove_button(what: String, on_press: Message) -> El<'static> {
    icon_button(crate::icons::Icon::Remove, 12.0, what, on_press)
}

/// A bare icon that does `on_press`, named by its tooltip.
pub fn icon_button(
    icon: crate::icons::Icon,
    size: f32,
    what: String,
    on_press: Message,
) -> El<'static> {
    iced::widget::tooltip(
        button(crate::icons::icon(icon, size))
            .padding(8)
            .style(button::text)
            .on_press(on_press),
        tip(what),
        iced::widget::tooltip::Position::Left,
    )
    .into()
}

/// The words a tooltip shows, in the popup style every tooltip shares.
pub fn tip<'view>(
    words: impl iced::widget::text::IntoFragment<'view>,
) -> container::Container<'view, Message> {
    container(text(words).size(12))
        .padding(6)
        .style(theme::popup)
}

/// A compact button for list rows.
pub fn small<'view>(
    label: impl iced::widget::text::IntoFragment<'view>,
    on_press: Message,
) -> button::Button<'view, Message> {
    button(text(label).size(12))
        .padding([3, 10])
        .style(theme::secondary)
        .on_press(on_press)
}

/// A removable entry such as a keyboard layout.
fn chip<'view>(label: String, actions: Vec<(&'view str, Message)>) -> El<'view> {
    let mut line = row![text(label).size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    for (action, message) in actions {
        line = line.push(
            button(text(action).size(11))
                .padding([1, 6])
                .style(theme::link)
                .on_press(message),
        );
    }
    container(line)
        .padding([4, 10])
        .style(theme::code_block)
        .into()
}

fn layouts_field<'app>(
    state: &'app App,
    value: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'app> {
    if state.system.layouts.is_empty() {
        return text_input("us,de", value)
            .on_input(move |input| make(Edit::Draft(input)))
            .into();
    }
    let codes = comma_list(value);
    let mut chips = Row::new().spacing(6).align_y(Alignment::Center);
    for (index, code) in codes.iter().enumerate() {
        let name = state
            .system
            .layout_name(code)
            .map_or_else(|| code.clone(), |name| format!("{name} ({code})"));
        let mut without = codes.clone();
        without.remove(index);
        let mut actions = vec![("Remove", make(Edit::Draft(without.join(","))))];
        if index > 0 {
            let mut first = codes.clone();
            let moved = first.remove(index);
            first.insert(0, moved);
            actions.insert(0, ("Make first", make(Edit::Draft(first.join(",")))));
        }
        chips = chips.push(chip(name, actions));
    }
    let add = {
        move |item: system::XkbItem| {
            let mut next = codes.clone();
            if !next.contains(&item.code) {
                next.push(item.code);
            }
            make(Edit::Draft(next.join(",")))
        }
    };
    column![
        chips.wrap(),
        combo_box(&state.layout_box, "Add a layout: type to search", None, add).width(Length::Fill),
    ]
    .spacing(8)
    .into()
}

fn options_field<'app>(
    state: &'app App,
    value: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'app> {
    if state.system.options.is_empty() {
        return text_input("grp:win_space_toggle,ctrl:nocaps", value)
            .on_input(move |input| make(Edit::Draft(input)))
            .into();
    }
    let codes = comma_list(value);
    let mut chips = column![].spacing(6);
    for (index, code) in codes.iter().enumerate() {
        let name = state
            .system
            .option_name(code)
            .map_or_else(|| code.clone(), |name| format!("{name} ({code})"));
        let mut without = codes.clone();
        without.remove(index);
        chips = chips.push(chip(
            name,
            vec![("Remove", make(Edit::Draft(without.join(","))))],
        ));
    }
    let add = {
        move |item: system::XkbItem| {
            let mut next = codes.clone();
            if !next.contains(&item.code) {
                next.push(item.code);
            }
            make(Edit::Draft(next.join(",")))
        }
    };
    column![
        chips,
        combo_box(
            &state.option_box,
            "Add an option: type to search",
            None,
            add
        )
        .width(Length::Fill),
    ]
    .spacing(8)
    .into()
}

fn file_field(
    key: &DraftKey,
    value: &str,
    example: &str,
    default: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let placeholder = if default.is_empty() {
        example.to_string()
    } else {
        default_placeholder(default)
    };
    let browse = Message::Browse {
        to: Destination::Field(key.clone(), HOLE.to_string()),
        folder: false,
    };
    row![
        text_input(&placeholder, value)
            .on_input(move |input| make(Edit::Draft(input)))
            .width(Length::Fill),
        small("Browse", browse),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn screenshot_field(
    state: &App,
    key: &DraftKey,
    value: &str,
    default: &str,
    make: impl Fn(Edit) -> Message + Clone + 'static,
) -> El<'static> {
    let saving = value != NO_FILE;
    let toggle = {
        let make = make.clone();
        move |on: bool| {
            make(if on {
                Edit::Choice(None)
            } else {
                Edit::Draft(NO_FILE.to_string())
            })
        }
    };
    let mut col = column![
        row![
            crate::motion::switch(saving, state.motions.switch).on_toggle(toggle),
            text("Save each screenshot to a file").size(13),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(8);
    if !saving {
        return col
            .push(
                text("Screenshots still go to the clipboard.")
                    .size(12)
                    .style(theme::muted),
            )
            .into();
    }
    let effective = if value.trim().is_empty() {
        default.to_string()
    } else {
        value.to_string()
    };
    let (folder, file) = split_path(&effective);
    let (shown_folder, shown_file) = if value.trim().is_empty() {
        (String::new(), String::new())
    } else {
        (folder.clone(), file.clone())
    };
    let (default_folder, default_file) = split_path(default);
    let on_folder = {
        let make = make.clone();
        let file = file.clone();
        move |input: String| make(Edit::Draft(join_path(&input, &file)))
    };
    let on_file = move |input: String| make(Edit::Draft(join_path(&folder, &input)));
    let browse = Message::Browse {
        to: Destination::Field(key.clone(), join_path(&HOLE.to_string(), &file)),
        folder: true,
    };
    col = col
        .push(
            row![
                text("Folder").size(13).width(80),
                text_input(&default_folder, &shown_folder)
                    .on_input(on_folder)
                    .width(Length::Fill),
                small("Browse", browse),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .push(
            row![
                text("File name").size(13).width(80),
                text_input(&default_file, &shown_file)
                    .on_input(on_file)
                    .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    let preview = match expand_strftime(&file) {
        Ok(name) => text(format!("A screenshot taken now: {name}"))
            .size(12)
            .style(theme::muted),
        Err(why) => text(why).size(12).style(theme::error),
    };
    col.push(preview).into()
}

/// The sizes drawn against the screen: one bar per width (or height).
pub struct SizesPreview {
    bars: Vec<(f32, String)>,
    axis: Axis,
    screen: Size,
}

impl SizesPreview {
    fn canvas(
        entries: &[(String, String)],
        axis: Axis,
        screen: Size,
    ) -> canvas::Canvas<Self, Message> {
        let extent = match axis {
            Axis::Width => screen.width,
            Axis::Height => screen.height,
        };
        let bars = entries
            .iter()
            .map(|(name, value)| (fraction(name, value, extent), size_label(name, value)))
            .collect();
        let height = match axis {
            Axis::Width => 24.0f32.mul_add(entries.len().max(1) as f32, 16.0),
            Axis::Height => 120.0,
        };
        canvas::Canvas::new(Self { bars, axis, screen })
            .width(Length::Fill)
            .height(Length::Fixed(height))
    }
}

impl canvas::Program<Message, Theme, Renderer> for SizesPreview {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let colors = theme::colors(theme);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), colors.canvas);
        let pad = 8.0;
        let label = |content: String, position: Point| canvas::Text {
            content,
            position,
            color: colors.text,
            size: 11.0.into(),
            font: Font::default(),
            align_y: iced::alignment::Vertical::Center,
            shaping: iced::widget::text::Shaping::Advanced,
            ..canvas::Text::default()
        };
        match self.axis {
            Axis::Width => {
                let width = 2.0f32.mul_add(-pad, bounds.width);
                for (index, (part, name)) in self.bars.iter().enumerate() {
                    let top = (index as f32).mul_add(24.0, pad);
                    frame.fill_rectangle(
                        Point::new(pad, top),
                        Size::new(width, 18.0),
                        colors.surface,
                    );
                    frame.fill_rectangle(
                        Point::new(pad, top),
                        Size::new(width * part, 18.0),
                        Color {
                            a: 0.45,
                            ..colors.accent
                        },
                    );
                    frame.fill_text(label(
                        format!("{name}, {} px", (part * self.screen.width).round()),
                        Point::new(pad + 6.0, top + 9.0),
                    ));
                }
            }
            Axis::Height => {
                let height = 2.0f32.mul_add(-pad, bounds.height);
                let column = 90.0;
                for (index, (part, name)) in self.bars.iter().enumerate() {
                    let left = (index as f32).mul_add(column + 8.0, pad);
                    frame.fill_rectangle(
                        Point::new(left, pad),
                        Size::new(column, height),
                        colors.surface,
                    );
                    let bar = height * part;
                    frame.fill_rectangle(
                        Point::new(left, pad + height - bar),
                        Size::new(column, bar),
                        Color {
                            a: 0.45,
                            ..colors.accent
                        },
                    );
                    frame.fill_text(label(
                        format!("{name}, {} px", (part * self.screen.height).round()),
                        Point::new(left + 6.0, pad + 10.0),
                    ));
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
