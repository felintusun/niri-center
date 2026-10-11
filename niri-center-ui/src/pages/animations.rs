//! The Animations page: each animation as a spring or an easing curve.

use iced::widget::{Row, button, column, pick_list, row, slider, space, text, text_input};
use iced::{Alignment, Length, Task};
use niri_center::animations::{self, Animation, Curve};
use niri_center::page::Page;
use niri_center::words::short_number;

use super::{pad, segments};
use niri_center::store::Store;

use crate::app::{App, said};
use crate::controls::{self, small};
use crate::message::Message as AppMessage;
use crate::rows;
use crate::{El, curves, theme};

/// What the page's controls ask for.
#[derive(Debug, Clone)]
pub enum Message {
    Select(Animation),
    /// Edit the animation as a spring (true) or an easing curve.
    Spring(bool),
    Curve(Curve),
    Bezier(curves::Bezier),
    /// One of the custom curve's four numbers, typed.
    Point(usize, String),
    Duration(String),
    UseCurve,
    Damping(f32),
    Stiffness(f32),
    UseSpring,
    Plays(bool),
    Reset,
}

pub fn update(state: &mut App, message: Message) -> Task<AppMessage> {
    match message {
        Message::Select(animation) => state.load_animation(animation),
        Message::Spring(spring) => state.animation.spring = spring,
        Message::Curve(name) => state.animation.curve = name,
        Message::Bezier(curves::Bezier(points)) => {
            state.animation.bezier = points.map(|point| short_number(f64::from(point)));
        }
        Message::Point(index, value) => {
            if let Some(slot) = state.animation.bezier.get_mut(index) {
                *slot = value;
            }
        }
        Message::Duration(value) => state.animation.duration = value,
        Message::UseCurve => {
            if let Some(store) = &mut state.session.store {
                state.status = said(state.animation.set_curve(store));
            }
        }
        Message::Damping(value) => state.animation.damping = value,
        Message::Stiffness(value) => state.animation.stiffness = value,
        Message::UseSpring => {
            if let Some(store) = &mut state.session.store {
                state.status = said(state.animation.set_spring(store));
            }
        }
        Message::Plays(on) => {
            if let Some(store) = &mut state.session.store {
                state.status = said(state.animation.set_playing(store, on));
            }
        }
        Message::Reset => {
            if let Some(store) = &mut state.session.store {
                state.status = said(state.animation.reset(store));
                state.animation.reload(Some(store));
            }
        }
    }
    Task::none()
}

pub fn page<'a>(state: &'a App, store: &'a Store) -> El<'a> {
    let col = column![rows::sections(state, Page::Animations)].spacing(24);
    let animation = state.animation.animation;
    let name = animation.name();
    let configured = store.animation_names();
    let names: Vec<controls::Labeled<Animation>> = Animation::ALL
        .into_iter()
        .map(|each| controls::Labeled {
            value: Some(each),
            label: if configured.iter().any(|c| c == each.name()) {
                format!("{} (in your config)", each.label())
            } else {
                each.label().to_string()
            },
        })
        .collect();
    let selected = names
        .iter()
        .find(|item| item.value == Some(animation))
        .cloned();
    let off = store.animation_off(name);
    let now = if off {
        "Turned off in your config.".to_string()
    } else if let Some((damping, stiffness, _)) = store.spring_params(name) {
        format!("A spring in your config: damping {damping}, stiffness {stiffness}.")
    } else if let Some(curve) = store.easing_curve(name) {
        let duration = store
            .duration_ms(name)
            .map_or_else(String::new, |ms| format!(" over {ms} ms"));
        format!("The {curve} curve{duration}, set in your config.")
    } else {
        match animation.default_motion() {
            animations::Motion::Spring {
                damping, stiffness, ..
            } => format!("niri's default: a spring, damping {damping}, stiffness {stiffness}."),
            animations::Motion::Easing { curve, duration_ms } => {
                format!(
                    "niri's default: the {} curve over {duration_ms} ms.",
                    curve.name()
                )
            }
        }
    };
    let mut rows = vec![
        pad(column![
            row![
                text("Animation").size(14),
                pick_list(
                    names,
                    selected,
                    move |item: controls::Labeled<Animation>| {
                        AppMessage::Animations(Message::Select(item.value.unwrap_or(animation)))
                    }
                )
                .width(Length::Fill),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
            text(now).size(13).style(theme::muted),
        ]
        .spacing(6)
        .into()),
        pad(row![
            text("Play this animation").size(14).width(Length::Fill),
            iced::widget::toggler(!off)
                .on_toggle(move |value| AppMessage::Animations(Message::Plays(value)))
                .size(20),
        ]
        .align_y(Alignment::Center)
        .into()),
    ];
    if store.has_custom_shader(name) {
        rows.push(pad(text(
            "It also has a custom shader. niri-center leaves the shader as it is.",
        )
        .size(13)
        .style(theme::pending)
        .into()));
    }
    let mode = |label: &'static str, spring: bool| {
        button(text(label).size(13))
            .padding([5, 12])
            .style(theme::segment(state.animation.spring == spring))
            .on_press(AppMessage::Animations(Message::Spring(spring)))
    };
    rows.push(pad(row![
        text("Move it with").size(14).width(Length::Fill),
        segments(row![mode("A spring", true), mode("An easing curve", false)]),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()));
    if state.animation.spring {
        rows.push(pad(column![
            row![
                text(format!("Damping {:.2}", state.animation.damping))
                    .size(14)
                    .width(130),
                slider(0.1_f32..=2.0_f32, state.animation.damping, move |value| {
                    AppMessage::Animations(Message::Damping(value))
                })
                .step(0.01_f32),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            text("Below 1 it overshoots. At 1 it settles without bouncing.")
                .size(12)
                .style(theme::muted),
            row![
                text(format!("Stiffness {}", state.animation.stiffness as i64))
                    .size(14)
                    .width(130),
                slider(
                    50.0_f32..=2000.0_f32,
                    state.animation.stiffness,
                    move |value| AppMessage::Animations(Message::Stiffness(value))
                )
                .step(10.0_f32),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            curves::spring_canvas(state.animation.damping, state.animation.stiffness),
            row![
                space().width(Length::Fill),
                small(
                    "Use this spring",
                    AppMessage::Animations(Message::UseSpring)
                ),
            ],
        ]
        .spacing(10)
        .into()));
    } else {
        let curve = state.animation.curve;
        let curve_items: Vec<controls::Labeled<Curve>> = Curve::ALL
            .into_iter()
            .map(|each| controls::Labeled {
                value: Some(each),
                label: each.label().to_string(),
            })
            .collect();
        let selected_curve = curve_items
            .iter()
            .find(|item| item.value == Some(curve))
            .cloned();
        let custom: [f32; 4] = std::array::from_fn(|i| {
            state.animation.bezier[i]
                .trim()
                .parse::<f32>()
                .unwrap_or(0.0)
        });
        let mut editor = column![
            row![
                text("Curve").size(14).width(Length::Fill),
                pick_list(
                    curve_items,
                    selected_curve,
                    move |item: controls::Labeled<Curve>| {
                        AppMessage::Animations(Message::Curve(item.value.unwrap_or(curve)))
                    }
                )
                .width(220),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                text("Duration in milliseconds")
                    .size(14)
                    .width(Length::Fill),
                text_input("250", &state.animation.duration)
                    .on_input(move |value| AppMessage::Animations(Message::Duration(value)))
                    .width(90),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            curves::curves_canvas(curve, custom),
        ]
        .spacing(10);
        if curve == Curve::CubicBezier {
            let mut points = Row::new().spacing(8).align_y(Alignment::Center);
            for (index, name) in ["x1", "y1", "x2", "y2"].into_iter().enumerate() {
                points = points.push(text(name).size(12).style(theme::muted)).push(
                    text_input("0.5", &state.animation.bezier[index])
                        .on_input(move |value| AppMessage::Animations(Message::Point(index, value)))
                        .width(64),
                );
            }
            editor = editor.push(points).push(
                text("Drag the handles, or type the points as in CSS cubic-bezier().")
                    .size(12)
                    .style(theme::muted),
            );
        }
        editor = editor.push(row![
            space().width(Length::Fill),
            small("Use this curve", AppMessage::Animations(Message::UseCurve)),
        ]);
        rows.push(pad(editor.into()));
    }
    if configured.iter().any(|c| c == name) {
        rows.push(pad(row![
            text("Your config sets this animation.")
                .size(13)
                .style(theme::muted)
                .width(Length::Fill),
            small(
                "Back to niri's default",
                AppMessage::Animations(Message::Reset)
            ),
        ]
        .align_y(Alignment::Center)
        .into()));
    }
    col.push(rows::group("One animation".to_string(), rows))
        .into()
}
