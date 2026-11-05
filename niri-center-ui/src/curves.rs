//! The Animations page's pictures: the easing curves niri accepts (with a
//! custom cubic bezier whose handles can be dragged), and a simulated
//! spring response. The animations and curves themselves are
//! `niri_center::animations`.

use crate::message::Message;
use iced::widget::canvas::{self, Canvas, Path, Program};
use iced::{Length, Point, Rectangle, Renderer, Theme, mouse};
use niri_center::animations::Curve;

const fn pad() -> f32 {
    10.0
}

fn plot_area(bounds: Rectangle) -> Rectangle {
    // local coordinates: the frame is translated by the widget bounds at draw
    let padding = pad();
    Rectangle {
        x: padding,
        y: padding,
        width: 2.0f32.mul_add(-padding, bounds.width).max(1.0),
        height: 2.0f32.mul_add(-padding, bounds.height).max(1.0),
    }
}

fn to_plot(point: (f32, f32), area: Rectangle) -> Point {
    Point::new(
        point.0.mul_add(area.width, area.x),
        point.1.mul_add(-area.height, area.y + area.height),
    )
}

/// Sample a cubic bezier with endpoints (0,0)→(1,1) at `along`.
fn bezier(controls: (f32, f32, f32, f32), along: f32) -> (f32, f32) {
    let rest = 1.0 - along;
    let x = (along * along).mul_add(
        along,
        (3.0 * rest * along * along).mul_add(controls.2, 3.0 * rest * rest * along * controls.0),
    );
    let y = (along * along).mul_add(
        along,
        (3.0 * rest * along * along).mul_add(controls.3, 3.0 * rest * rest * along * controls.1),
    );
    (x, y)
}

/// Grid + every named curve; the selected one is highlighted, and the
/// custom curve shows two handles to drag.
pub struct CurvesProgram {
    pub selected: Curve,
    pub custom: [f32; 4],
}

#[derive(Default)]
pub struct CurvesState {
    /// Which handle is held: 0 for the first control point, 1 for the second.
    dragging: Option<usize>,
}

impl CurvesProgram {
    fn handles(&self, area: Rectangle) -> [Point; 2] {
        let [x1, y1, x2, y2] = self.custom;
        [to_plot((x1, y1), area), to_plot((x2, y2), area)]
    }
}

impl Program<Message, Theme, Renderer> for CurvesProgram {
    type State = CurvesState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if self.selected != Curve::CubicBezier {
            return None;
        }
        let area = plot_area(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pointer = cursor.position_in(bounds)?;
                let handles = self.handles(area);
                let index = handles
                    .iter()
                    .position(|handle| handle.distance(pointer) < 12.0)?;
                state.dragging = Some(index);
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let index = state.dragging?;
                let across = ((position.x - bounds.x - area.x) / area.width).clamp(0.0, 1.0);
                // y may overshoot: niri's curves can bounce past the end
                let up = ((area.y + area.height - (position.y - bounds.y)) / area.height)
                    .clamp(-0.5, 1.5);
                let mut points = self.custom;
                // each handle is a pair of the four numbers: x then y
                if let Some([handle_x, handle_y]) = points.as_chunks_mut::<2>().0.get_mut(index) {
                    *handle_x = (across * 100.0).round() / 100.0;
                    *handle_y = (up * 100.0).round() / 100.0;
                }
                Some(canvas::Action::publish(Message::Animations(
                    crate::pages::animations::Message::Bezier(points),
                )))
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging.take().map(|_| canvas::Action::capture())
            }
            canvas::Event::Mouse(_)
            | canvas::Event::Keyboard(_)
            | canvas::Event::Touch(_)
            | canvas::Event::Window(_)
            | canvas::Event::InputMethod(_) => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let colors = crate::theme::colors(theme);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), colors.canvas);
        let area = plot_area(bounds);

        // grid: x/y at 0, 0.5, 1
        for step in [0.0f32, 0.5, 1.0] {
            let vx = to_plot((step, 0.0), area).x;
            let hy = to_plot((0.0, step), area).y;
            let grid = Path::new(|builder| {
                builder.move_to(Point::new(vx, area.y));
                builder.line_to(Point::new(vx, area.y + area.height));
                builder.move_to(Point::new(area.x, hy));
                builder.line_to(Point::new(area.x + area.width, hy));
            });
            frame.stroke(
                &grid,
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(colors.line),
            );
        }

        for each in Curve::ALL {
            let highlighted = self.selected == each;
            let curve = match each.control_points() {
                Some(points) => points,
                // the custom curve shows only while it is the one picked
                None if highlighted => self.custom.into(),
                None => continue,
            };
            let color = if highlighted {
                colors.accent
            } else {
                colors.line
            };
            let width = if highlighted { 2.5 } else { 1.0 };
            let path = Path::new(|builder| {
                let steps = 60u32;
                builder.move_to(to_plot(bezier(curve, 0.0), area));
                for step in 1..=steps {
                    builder.line_to(to_plot(bezier(curve, step as f32 / steps as f32), area));
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(width)
                    .with_color(color),
            );
        }

        if self.selected == Curve::CubicBezier {
            let [first, second] = self.handles(area);
            let arms = Path::new(|builder| {
                builder.move_to(to_plot((0.0, 0.0), area));
                builder.line_to(first);
                builder.move_to(to_plot((1.0, 1.0), area));
                builder.line_to(second);
            });
            frame.stroke(
                &arms,
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(colors.pending),
            );
            for handle in [first, second] {
                frame.fill(&Path::circle(handle, 6.0), colors.pending);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let near = cursor.position_in(bounds).is_some_and(|pointer| {
            self.handles(plot_area(bounds))
                .iter()
                .any(|handle| handle.distance(pointer) < 12.0)
        });
        if state.dragging.is_some() {
            mouse::Interaction::Grabbing
        } else if self.selected == Curve::CubicBezier && near {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Simulated spring response for the given parameters (unit step from 1 to 0).
pub struct SpringProgram {
    pub damping_ratio: f32,
    pub stiffness: f32,
}

impl Program<Message, Theme, Renderer> for SpringProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        // 1.2 s in steps of 1/240 s
        const STEPS: u16 = 288;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let colors = crate::theme::colors(theme);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), colors.canvas);
        let area = plot_area(bounds);

        // the spring as niri moves it, over 1.2 s
        let spring = niri_center::animations::Spring {
            damping_ratio: self.damping_ratio,
            stiffness: self.stiffness,
            epsilon: 0.0001,
        };
        let path = Path::new(|builder| {
            builder.move_to(to_plot((0.0, 1.0), area));
            for step in 1..=STEPS {
                let along = f32::from(step) / f32::from(STEPS);
                let offset = niri_center::motion::spring_offset(spring, along * 1.2);
                builder.line_to(to_plot((along, offset), area));
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_width(2.0)
                .with_color(colors.success),
        );
        // settled-value line at the bottom (y = 0)
        let target = Path::new(|builder| {
            builder.move_to(Point::new(area.x, area.y + area.height));
            builder.line_to(Point::new(area.x + area.width, area.y + area.height));
        });
        frame.stroke(
            &target,
            canvas::Stroke::default()
                .with_width(1.0)
                .with_color(colors.line),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

/// Canvas previewing niri's easing curves, with the custom one editable.
pub fn curves_canvas(selected: Curve, custom: [f32; 4]) -> Canvas<CurvesProgram, Message> {
    Canvas::new(CurvesProgram { selected, custom })
        .width(Length::Fill)
        .height(Length::Fixed(260.0))
}

/// Canvas previewing a spring's response trace.
pub fn spring_canvas(damping_ratio: f32, stiffness: f32) -> Canvas<SpringProgram, Message> {
    Canvas::new(SpringProgram {
        damping_ratio,
        stiffness,
    })
    .width(Length::Fill)
    .height(Length::Fixed(220.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_a_handle_moves_only_its_own_two_numbers() {
        let program = CurvesProgram {
            selected: Curve::CubicBezier,
            custom: [0.25, 0.25, 0.75, 0.75],
        };
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 260.0));
        let area = plot_area(bounds);
        let first_handle = program.handles(area)[0];
        let mut state = CurvesState::default();
        let pressed = program.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(first_handle),
        );
        assert!(pressed.is_some());
        let middle = Point::new(area.x + area.width / 2.0, area.y + area.height / 2.0);
        let dragged = program.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::CursorMoved { position: middle }),
            bounds,
            mouse::Cursor::Available(middle),
        );
        match dragged.map(|action| action.into_inner().0) {
            Some(Some(Message::Animations(crate::pages::animations::Message::Bezier(points)))) => {
                assert_eq!(points, [0.5, 0.5, 0.75, 0.75]);
            }
            other => panic!("expected new control points, got {other:?}"),
        }
    }
}
