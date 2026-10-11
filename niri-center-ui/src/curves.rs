//! The Animations page's pictures: the easing curves niri accepts (with a
//! custom cubic bezier whose handles can be dragged), and a simulated
//! spring response. The animations and curves themselves are
//! `niri_center::animations`.

use iced::widget::canvas::{self, Canvas, Path, Program};
use iced::{Length, Point, Rectangle, Renderer, Theme, mouse};
use niri_center::animations::Curve;

/// New control points for the custom curve, while a handle is dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bezier(pub [f32; 4]);

const fn pad() -> f32 {
    10.0
}

fn plot_area(bounds: Rectangle) -> Rectangle {
    // local coordinates: the frame is translated by the widget bounds at draw
    let p = pad();
    Rectangle {
        x: p,
        y: p,
        width: 2.0f32.mul_add(-p, bounds.width).max(1.0),
        height: 2.0f32.mul_add(-p, bounds.height).max(1.0),
    }
}

fn to_plot(p: (f32, f32), area: Rectangle) -> Point {
    Point::new(
        p.0.mul_add(area.width, area.x),
        p.1.mul_add(-area.height, area.y + area.height),
    )
}

/// Sample a cubic bezier with endpoints (0,0)→(1,1) at `t`.
fn bezier(c: (f32, f32, f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let x = (t * t).mul_add(t, (3.0 * u * t * t).mul_add(c.2, 3.0 * u * u * t * c.0));
    let y = (t * t).mul_add(t, (3.0 * u * t * t).mul_add(c.3, 3.0 * u * u * t * c.1));
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

impl<Message> Program<Message, Theme, Renderer> for CurvesProgram
where
    Message: From<Bezier> + 'static,
{
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
                let p = cursor.position_in(bounds)?;
                let handles = self.handles(area);
                let index = (0..2).find(|&i| handles[i].distance(p) < 12.0)?;
                state.dragging = Some(index);
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let index = state.dragging?;
                let x = ((position.x - bounds.x - area.x) / area.width).clamp(0.0, 1.0);
                // y may overshoot: niri's curves can bounce past the end
                let y = ((area.y + area.height - (position.y - bounds.y)) / area.height)
                    .clamp(-0.5, 1.5);
                let mut points = self.custom;
                points[index * 2] = (x * 100.0).round() / 100.0;
                points[index * 2 + 1] = (y * 100.0).round() / 100.0;
                Some(canvas::Action::publish(Message::from(Bezier(points))))
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
        let c = crate::theme::colors(theme);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), c.canvas);
        let area = plot_area(bounds);

        // grid: x/y at 0, 0.5, 1
        for step in [0.0f32, 0.5, 1.0] {
            let vx = to_plot((step, 0.0), area).x;
            let hy = to_plot((0.0, step), area).y;
            let grid = Path::new(|b| {
                b.move_to(Point::new(vx, area.y));
                b.line_to(Point::new(vx, area.y + area.height));
                b.move_to(Point::new(area.x, hy));
                b.line_to(Point::new(area.x + area.width, hy));
            });
            frame.stroke(
                &grid,
                canvas::Stroke::default().with_width(1.0).with_color(c.line),
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
            let color = if highlighted { c.accent } else { c.line };
            let width = if highlighted { 2.5 } else { 1.0 };
            let path = Path::new(|b| {
                let steps = 60u32;
                b.move_to(to_plot(bezier(curve, 0.0), area));
                for i in 1..=steps {
                    b.line_to(to_plot(bezier(curve, i as f32 / steps as f32), area));
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
            let arms = Path::new(|b| {
                b.move_to(to_plot((0.0, 0.0), area));
                b.line_to(first);
                b.move_to(to_plot((1.0, 1.0), area));
                b.line_to(second);
            });
            frame.stroke(
                &arms,
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(c.pending),
            );
            for handle in [first, second] {
                frame.fill(&Path::circle(handle, 6.0), c.pending);
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
        let near = cursor.position_in(bounds).is_some_and(|p| {
            self.handles(plot_area(bounds))
                .iter()
                .any(|handle| handle.distance(p) < 12.0)
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

impl<Message> Program<Message, Theme, Renderer> for SpringProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let c = crate::theme::colors(theme);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), c.canvas);
        let area = plot_area(bounds);

        // mass-spring: x'' + 2ζω x' + ω²x = 0, x(0)=1, x'(0)=0
        let omega = self.stiffness.max(1.0).sqrt();
        let zeta = self.damping_ratio;
        let dt = 1.0 / 240.0;
        let steps = (1.2 / dt) as usize;
        let (mut x, mut v) = (1.0f32, 0.0f32);
        let mut samples = vec![(0.0f32, x)];
        for i in 1..=steps {
            let a = (omega * omega).mul_add(-x, -2.0 * zeta * omega * v);
            v = a.mul_add(dt, v);
            x = v.mul_add(dt, x);
            samples.push(((i as f32 * dt) / 1.2, x));
        }
        let path = Path::new(|b| {
            b.move_to(to_plot((samples[0].0, samples[0].1), area));
            for (t, s) in samples.iter().skip(1) {
                b.line_to(to_plot((*t, *s), area));
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_width(2.0)
                .with_color(c.success),
        );
        // settled-value line at the bottom (y = 0)
        let target = Path::new(|b| {
            b.move_to(Point::new(area.x, area.y + area.height));
            b.line_to(Point::new(area.x + area.width, area.y + area.height));
        });
        frame.stroke(
            &target,
            canvas::Stroke::default().with_width(1.0).with_color(c.line),
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
pub fn curves_canvas<Message>(selected: Curve, custom: [f32; 4]) -> Canvas<CurvesProgram, Message>
where
    Message: From<Bezier> + 'static,
{
    Canvas::new(CurvesProgram { selected, custom })
        .width(Length::Fill)
        .height(Length::Fixed(260.0))
}

/// Canvas previewing a spring's response trace.
pub fn spring_canvas<Message>(
    damping_ratio: f32,
    stiffness: f32,
) -> Canvas<SpringProgram, Message> {
    Canvas::new(SpringProgram {
        damping_ratio,
        stiffness,
    })
    .width(Length::Fill)
    .height(Length::Fixed(220.0))
}
