//! The Displays page: the arrangement to drag, and each display's
//! settings, with display changes on trial.

use iced::widget::{button, column, container, row, space, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::niri;
use niri_center::outputs::{self, MonitorInfo};
use niri_center::settings::{self, Coordinate, DraftKey, Edit, OutputSetting};
use num_traits::ToPrimitive;

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
    pub error: Option<niri::NiriError>,
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
                let Some((x, y)) = outputs::snap(
                    &dropped.name,
                    f64::from(dropped.x),
                    f64::from(dropped.y),
                    &rects,
                ) else {
                    return Ok(());
                };
                moved = Some((x, y));
                store.set_output_position(&dropped.name, x, y)
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
                (Err(failure), _) => state.status = error(failure.to_string()),
            }
        }
        Message::Pick(name) => state.displays.selected = Some(name),
        Message::Position(name, coordinate, typed) => {
            // tried once the typing stops and it is saved
            let outputs = &state.outputs;
            if let Err(failure) = state.session.change_display(true, |store, drafts| {
                outputs::type_position(store, drafts, outputs, &name, coordinate, typed)
            }) {
                state.status = error(failure.to_string());
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
    if let Some(failure) = &state.displays.error {
        overview = overview.push(
            text(format!("niri did not answer: {failure}"))
                .size(13)
                .style(theme::error),
        );
    }
    if state.outputs.is_empty() && state.displays.error.is_none() {
        overview = overview.push(text("niri reports no connected displays.").size(13));
    }
    if !rects.is_empty() {
        overview = overview.push(monitors::monitor_canvas(
            &rects,
            width,
            state.selected_display().map(|output| output.name.as_str()),
        ));
        let (overlapping, alone) = niri_center::outputs::problems(&rects);
        for (first, second) in overlapping {
            overview = overview.push(
                text(format!(
                    "{first} and {second} overlap. niri moves overlapping displays apart on its own, so drag them edge to edge."
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
fn position_row(
    state: &App,
    output: &niri_center::outputs::OutputInfo,
    rects: &[(String, niri_center::outputs::MonitorInfo)],
) -> El<'static> {
    let (position_x, position_y) = rects
        .iter()
        .find(|(name, _)| *name == output.name)
        .and_then(|(_, rect)| Some((rect.x.round().to_i64()?, rect.y.round().to_i64()?)))
        .unwrap_or((0, 0));
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
            label = label.push(text(problem.to_string()).size(12).style(theme::error));
        }
    }
    row![
        label.width(Length::Fill),
        container(
            row![
                field(Coordinate::X, position_x),
                field(Coordinate::Y, position_y)
            ]
            .spacing(6)
        )
        .width(220),
    ]
    .spacing(8)
    .padding([10, 12])
    .align_y(Alignment::Center)
    .into()
}

/// While a display change is on trial: what it is, how long until it goes
/// back, Keep and Go back, on a card that floats over the top of the
/// window, so the page under it stays where it is.
pub fn trial(state: &App) -> Option<El<'_>> {
    let trying = state.session.trial.as_ref()?;
    Some(
        container(
            column![
                text(format!(
                    "Trying {}. Going back in {} s.",
                    trying.what, trying.seconds_left
                ))
                .size(14),
                crate::motion::countdown(
                    trying.seconds_left,
                    niri_center::session::TRIAL_SECONDS,
                    state.motions.notice,
                ),
                row![
                    button(text("Keep").size(13))
                        .style(theme::primary)
                        .on_press(AppMessage::KeepTrial),
                    button(text("Go back").size(13))
                        .style(theme::secondary)
                        .on_press(AppMessage::EndTrial),
                    text("Enter or Esc goes back").size(12).style(theme::muted),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding(14)
        .max_width(560)
        .style(theme::trial_card)
        .into(),
    )
}

/// One display's settings, laid out for `width`.
fn output_rows<'app>(
    state: &'app App,
    output: &niri_center::outputs::OutputInfo,
    rects: &[(String, niri_center::outputs::MonitorInfo)],
    width: f32,
) -> El<'app> {
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
            store.setting(&chain, "mode").as_deref(),
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
        // unset, niri picks a scale that fits the display: the one it runs
        let fitted = if current.is_none() {
            percent(output.scale)
        } else {
            "fits the display".to_string()
        };
        let default_label = format!("{} ({fitted})", settings::NIRI_DEFAULT);
        row![
            controls::choice_list(
                niri_center::outputs::SCALES
                    .iter()
                    .map(|scale| {
                        let factor = scale.parse::<f64>().unwrap_or(1.0);
                        ((*scale).to_string(), percent(factor))
                    })
                    .collect(),
                picked.as_deref().or(current.as_deref()),
                &default_label,
                move |edit| AppMessage::Output(name.clone(), OutputSetting::Scale, edit),
            ),
            text_input(&niri_center::words::short_number(output.scale), &shown)
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
    let (off_control, off_notes): (Option<El<'app>>, Vec<settings::Note>) = if others_on || is_off {
        (None, Vec::new())
    } else {
        (
            Some(crate::motion::switch(false, state.motions.switch).into()),
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

/// A scale as people read it: `1.25` is `125%`.
fn percent(scale: f64) -> String {
    format!("{}%", niri_center::words::short_number(scale * 100.0))
}
