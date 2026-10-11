//! The Displays page: the arrangement to drag, and each display's
//! settings, with display changes on trial.

use iced::widget::{button, column, container, row, space, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::niri;
use niri_center::outputs::{self, MonitorInfo};
use niri_center::settings::{self, Coordinate, DraftKey, Edit, OutputSetting};

use crate::app::{App, error, info, room};
use crate::controls::{self, small};
use crate::message::Message as AppMessage;
use crate::monitors::DroppedMonitor;
use crate::rows;
use crate::{El, monitors, theme};

/// The display shown, and the arrangement as niri last reported it.
#[derive(Debug, Default)]
pub struct State {
    /// The display whose settings show; `None` is the first.
    pub selected: Option<String>,
    pub monitors: Vec<(String, MonitorInfo)>,
    /// Why niri could not say which displays there are.
    pub error: Option<String>,
}

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    Pick(String),
    /// Ask niri for the displays again.
    Refresh,
    /// A display dragged to a new place on the arrangement.
    Dropped(DroppedMonitor),
    /// A position typed: the display, the axis, the text.
    Position(String, Coordinate, String),
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Refresh => {
            return state.ask_niri(niri::Command::Outputs);
        }
        Message::Dropped(dropped) => {
            let mut moved = None;
            let result = state.session.change_display(false, |store, _| {
                // edge to edge with the nearest display, even left of 0,0
                let rects = outputs::staged(&state.outputs, store);
                let (x, y) =
                    outputs::snap(&dropped.name, dropped.x as f64, dropped.y as f64, &rects);
                moved = Some((x, y));
                store
                    .set_output_position(&dropped.name, x, y)
                    .map_err(|e| e.to_string())
            });
            match (result, moved) {
                (Ok(()), Some((x, y))) => {
                    state.status = info(format!("{} moved to {x}, {y}.", dropped.name));
                    // mirror the move so the canvas shows it now; a refused
                    // move snaps back
                    if let Some((_, m)) = state
                        .displays
                        .monitors
                        .iter_mut()
                        .find(|(n, _)| *n == dropped.name)
                    {
                        m.x = x as f64;
                        m.y = y as f64;
                    }
                }
                (Ok(()), None) => {}
                (Err(e), _) => state.status = error(e),
            }
        }
        Message::Pick(name) => state.displays.selected = Some(name),
        Message::Position(name, coordinate, typed) => {
            // tried once the typing stops and it is saved
            let outputs = &state.outputs;
            if let Err(e) = state.session.change_display(true, |store, drafts| {
                outputs::type_position(store, drafts, outputs, &name, coordinate, typed)
            }) {
                state.status = error(e);
            }
        }
    }
    Task::none()
}

/// The arrangement and the list, with a display's settings beside them
/// when there is room.
pub fn page(state: &App) -> El<'_> {
    let width = state.content_width();
    let beside = width >= room::DISPLAY_BESIDE_LIST;
    let rects = state.session.store.as_ref().map_or_else(
        || state.displays.monitors.clone(),
        |store| niri_center::outputs::staged(&state.outputs, store),
    );
    let mut overview = column![].spacing(12).width(Length::Fill);
    if let Some(e) = &state.displays.error {
        overview = overview.push(
            text(format!("niri did not answer: {e}"))
                .size(13)
                .style(theme::error),
        );
    }
    if state.outputs.is_empty() && state.displays.error.is_none() {
        overview = overview.push(text("niri reports no connected displays.").size(13));
    }
    if !rects.is_empty() {
        overview = overview.push(monitors::monitor_canvas(&rects));
        let (overlapping, alone) = niri_center::outputs::problems(&rects);
        for (a, b) in overlapping {
            overview = overview.push(
                text(format!(
                    "{a} and {b} overlap. niri moves overlapping displays apart on its own, so drag them edge to edge."
                ))
                .size(13)
                .style(theme::pending),
            );
        }
        for name in alone {
            overview = overview.push(
                text(format!(
                    "{name} touches no other display, so the pointer cannot move onto it."
                ))
                .size(13)
                .style(theme::pending),
            );
        }
    }
    let selected = state.selected_display();
    let mut list = Vec::new();
    for output in &state.outputs {
        let size = rects
            .iter()
            .find(|(name, _)| *name == output.name)
            .map_or_else(String::new, |(_, rect)| {
                format!(", {} × {} after scaling", rect.width, rect.height)
            });
        let name = output.title().to_string();
        list.push(
            button(
                column![
                    text(name).size(14),
                    text(format!("{}{size}", output.name))
                        .size(12)
                        .style(theme::muted),
                ]
                .spacing(2),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::history_item(selected == Some(output)))
            .on_press(AppMessage::Displays(Message::Pick(output.name.clone())))
            .into(),
        );
    }
    if !list.is_empty() {
        overview = overview.push(rows::group(String::new(), list));
    }
    overview = overview.push(
        row![
            text("Drag a display to move it.")
                .size(12)
                .style(theme::muted)
                .width(Length::Fill),
            small("Refresh from niri", AppMessage::Displays(Message::Refresh)),
        ]
        .align_y(Alignment::Center),
    );
    let detail: El<'_> = selected.map_or_else(
        || space().into(),
        |output| {
            // the settings take 6 of 11 parts beside the list, past the gap
            let width = if beside {
                (width - 28.0) * 6.0 / 11.0
            } else {
                width
            };
            output_rows(state, output, &rects, width)
        },
    );
    if beside {
        row![
            container(overview).width(Length::FillPortion(5)),
            container(detail).width(Length::FillPortion(6)),
        ]
        .spacing(28)
        .into()
    } else {
        column![overview, detail].spacing(20).into()
    }
}

/// Where a display sits, as two numbers to type: for keyboards, and for
/// a place the drag cannot reach.
fn position_row<'a>(
    state: &'a App,
    output: &'a niri_center::outputs::OutputInfo,
    rects: &[(String, niri_center::outputs::MonitorInfo)],
) -> El<'a> {
    let (x, y) = rects
        .iter()
        .find(|(name, _)| *name == output.name)
        .map_or((0, 0), |(_, rect)| (rect.x as i64, rect.y as i64));
    let field = |coordinate: Coordinate, value: i64| {
        let key = DraftKey::Position(output.name.clone(), coordinate);
        let shown = state
            .session
            .drafts
            .typed(&key)
            .cloned()
            .unwrap_or_else(|| value.to_string());
        let name = output.name.clone();
        let placeholder = match coordinate {
            Coordinate::X => "x",
            Coordinate::Y => "y",
        };
        text_input(placeholder, &shown)
            .on_input(move |typed| {
                AppMessage::Displays(Message::Position(name.clone(), coordinate, typed))
            })
            .width(Length::Fill)
    };
    let mut label = column![
        text("Position").size(14),
        text("In logical pixels. Negative values place it left of or above another.")
            .size(12)
            .style(theme::muted),
    ]
    .spacing(2);
    for coordinate in [Coordinate::X, Coordinate::Y] {
        if let Some(problem) = state
            .session
            .drafts
            .error(&DraftKey::Position(output.name.clone(), coordinate))
        {
            label = label.push(text(problem.clone()).size(12).style(theme::error));
        }
    }
    row![
        label.width(Length::Fill),
        container(row![field(Coordinate::X, x), field(Coordinate::Y, y)].spacing(6)).width(220),
        space().width(30),
    ]
    .spacing(8)
    .padding([10, 12])
    .align_y(Alignment::Center)
    .into()
}

/// While a display change is on trial: what it is, how long until it goes
/// back, Keep and Go back.
pub fn trial(state: &App) -> Option<El<'_>> {
    let trying = state.session.trial.as_ref()?;
    let fraction = trying.seconds_left as f32 / niri_center::session::TRIAL_SECONDS as f32;
    Some(
        container(
            column![
                text(format!(
                    "Trying {}. Going back in {} s.",
                    trying.what, trying.seconds_left
                ))
                .size(14),
                iced::widget::progress_bar(0.0..=1.0, fraction)
                    .girth(3)
                    .style(|theme: &iced::Theme| iced::widget::progress_bar::Style {
                        background: iced::Background::Color(theme::colors(theme).line),
                        bar: iced::Background::Color(theme::colors(theme).pending),
                        border: iced::Border::default(),
                    }),
                row![
                    button(text("Keep").size(13))
                        .style(theme::primary)
                        .on_press(AppMessage::KeepTrial),
                    button(text("Go back").size(13))
                        .style(theme::secondary)
                        .on_press(AppMessage::EndTrial),
                    text("Enter keeps, Esc goes back")
                        .size(12)
                        .style(theme::muted),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding(12)
        .width(Length::Fill)
        .style(theme::changed_row)
        .into(),
    )
}

/// One display's settings, laid out for `width`.
fn output_rows<'a>(
    state: &'a App,
    output: &'a niri_center::outputs::OutputInfo,
    rects: &[(String, niri_center::outputs::MonitorInfo)],
    width: f32,
) -> El<'a> {
    let Some(store) = state.store() else {
        return space().into();
    };
    let chain = settings::output_chain(&output.name);
    let row = |setting: OutputSetting, help: Option<String>, notes, control| {
        rows::output_row(
            state,
            &output.name,
            setting,
            rows::Extra {
                help,
                notes,
                control,
            },
            width,
        )
    };
    let preferred = output.preferred.and_then(|index| output.modes.get(index));
    let mode_control = {
        let name = output.name.clone();
        controls::choice_list(
            output
                .modes
                .iter()
                .map(|mode| (mode.value.clone(), mode.choice.clone()))
                .collect(),
            store.setting(&chain, "mode"),
            &preferred.map_or_else(
                || settings::NIRI_DEFAULT.to_string(),
                |mode| format!("{} ({})", settings::NIRI_DEFAULT, mode.label),
            ),
            move |edit| AppMessage::Output(name.clone(), OutputSetting::Mode, edit),
        )
    };
    let scale_control = {
        let name = output.name.clone();
        let typed = name.clone();
        let current = store.setting(&chain, "scale");
        // `1.0` in the config is the list's `1`: picked by value, not text
        let picked = current
            .as_deref()
            .and_then(niri_center::outputs::listed_scale)
            .map(str::to_string);
        let key = DraftKey::Output(output.name.clone(), OutputSetting::Scale);
        let shown = state
            .session
            .drafts
            .typed(&key)
            .cloned()
            .or_else(|| current.clone())
            .unwrap_or_default();
        row![
            controls::choice_list(
                niri_center::outputs::SCALES
                    .iter()
                    .map(|scale| {
                        let percent = scale.parse::<f64>().unwrap_or(1.0) * 100.0;
                        ((*scale).to_string(), format!("{percent}%"))
                    })
                    .collect(),
                picked.or_else(|| current.clone()),
                &format!("{} (fits the display)", settings::NIRI_DEFAULT),
                move |edit| AppMessage::Output(name.clone(), OutputSetting::Scale, edit),
            ),
            text_input("1.5", &shown)
                .on_input(move |input| AppMessage::Output(
                    typed.clone(),
                    OutputSetting::Scale,
                    Edit::Draft(input)
                ))
                .width(72),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    };
    let desktop = rects
        .iter()
        .find(|(name, _)| *name == output.name)
        .map_or_else(String::new, |(_, rect)| {
            format!(
                "The desktop on it is {} × {} logical pixels.",
                rect.width, rect.height
            )
        });
    let mut rows = vec![
        row(OutputSetting::Mode, None, Vec::new(), Some(mode_control)),
        row(
            OutputSetting::Scale,
            Some(desktop),
            Vec::new(),
            Some(scale_control),
        ),
        row(OutputSetting::Transform, None, Vec::new(), None),
    ];
    if output.vrr_supported {
        rows.push(row(
            OutputSetting::VariableRefreshRate,
            None,
            Vec::new(),
            None,
        ));
    }
    rows.push(row(OutputSetting::BackdropColor, None, Vec::new(), None));
    // the last display that is on stays on: the library refuses the edit,
    // and the row says why instead of offering it
    let others_on = settings::others_on(store, &state.outputs, &output.name);
    let is_off = store.is_set(&chain, "off");
    let (off_control, off_notes): (Option<El<'a>>, Vec<settings::Note>) = if others_on || is_off {
        (None, Vec::new())
    } else {
        (
            Some(iced::widget::toggler(false).size(20).into()),
            vec![settings::Note::info(
                "This is the only display that is on, so it stays on.",
            )],
        )
    };
    rows.push(position_row(state, output, rects));
    rows.push(row(OutputSetting::Off, None, off_notes, off_control));
    let heading = output.title().to_string();
    let detail = column![text(heading).size(20).font(rows::SEMIBOLD),]
        .spacing(12)
        .width(Length::Fill);
    detail.push(rows::group(String::new(), rows)).into()
}
