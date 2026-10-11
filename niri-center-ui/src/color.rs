//! The color picker under a color field: a saturation and brightness square,
//! a hue bar and an alpha bar, drawn on one canvas. Which colors niri
//! reads, and the picker's HSV, are `niri_center::color`.

use iced::widget::canvas::{self, Canvas, Frame, Program, gradient};
use iced::{Color, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use niri_center::color::Rgba;
pub use niri_center::color::{Hsva, SWATCHES};

const fn to_iced(color: Rgba) -> Color {
    Color::from_rgba(color.r, color.g, color.b, color.a)
}

const fn from_iced(color: Color) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

/// Any color niri accepts, as an iced color.
pub fn parse(input: &str) -> Option<Color> {
    Rgba::parse(input).map(to_iced)
}

/// `color` as `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn hex(color: Color) -> String {
    from_iced(color).hex()
}

/// `color` as HSV, keeping `previous`'s hue when `color` is grey.
pub fn hsva(color: Color, previous: Hsva) -> Hsva {
    Hsva::from_rgba(from_iced(color), previous)
}

/// The color `hsva` picks.
pub fn shown(hsva: Hsva) -> Color {
    to_iced(hsva.rgba())
}

const SQUARE: f32 = 120.0;
const BAR: f32 = 12.0;
const GAP: f32 = 8.0;
pub const HEIGHT: f32 = SQUARE + 2.0 * (GAP + BAR);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Square,
    Hue,
    Alpha,
}

#[derive(Default)]
pub struct PickerState {
    dragging: Option<Part>,
}

pub struct Picker {
    pub hsva: Hsva,
}

fn part_at(y: f32) -> Part {
    if y < SQUARE + GAP / 2.0 {
        Part::Square
    } else if y < SQUARE + GAP + BAR + GAP / 2.0 {
        Part::Hue
    } else {
        Part::Alpha
    }
}

impl Picker {
    /// The color the pointer at `p` (canvas-local) picks on `part`.
    fn pick(&self, part: Part, p: Point, width: f32) -> Hsva {
        let fx = (p.x / width.max(1.0)).clamp(0.0, 1.0);
        let mut hsva = self.hsva;
        match part {
            Part::Square => {
                hsva.saturation = fx;
                hsva.value = 1.0 - (p.y / SQUARE).clamp(0.0, 1.0);
            }
            Part::Hue => hsva.hue = fx * 360.0,
            Part::Alpha => hsva.alpha = fx,
        }
        hsva
    }
}

impl<Message> Program<Message, Theme, Renderer> for Picker
where
    Message: From<Hsva> + 'static,
{
    type State = PickerState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let local = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let part = part_at(p.y);
                state.dragging = Some(part);
                Some(
                    canvas::Action::publish(Message::from(self.pick(part, p, bounds.width)))
                        .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let part = state.dragging?;
                let p = local?;
                Some(canvas::Action::publish(Message::from(self.pick(
                    part,
                    p,
                    bounds.width,
                ))))
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
        let c = crate::theme::colors(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let pure = shown(Hsva {
            saturation: 1.0,
            value: 1.0,
            alpha: 1.0,
            ..self.hsva
        });

        // saturation left to right, then brightness top to bottom
        let square = Size::new(width, SQUARE);
        frame.fill_rectangle(
            Point::ORIGIN,
            square,
            gradient::Linear::new(Point::ORIGIN, Point::new(width, 0.0))
                .add_stop(0.0, Color::WHITE)
                .add_stop(1.0, pure),
        );
        frame.fill_rectangle(
            Point::ORIGIN,
            square,
            gradient::Linear::new(Point::ORIGIN, Point::new(0.0, SQUARE))
                .add_stop(0.0, Color::TRANSPARENT)
                .add_stop(1.0, Color::BLACK),
        );
        let marker = Point::new(
            self.hsva.saturation * width,
            (1.0 - self.hsva.value) * SQUARE,
        );
        let ring = canvas::Path::circle(marker, 6.0);
        frame.stroke(
            &ring,
            canvas::Stroke::default()
                .with_width(2.0)
                .with_color(Color::WHITE),
        );
        frame.stroke(
            &canvas::Path::circle(marker, 7.5),
            canvas::Stroke::default()
                .with_width(1.0)
                .with_color(Color::BLACK),
        );

        // hue
        let hue_y = SQUARE + GAP;
        let mut hues = gradient::Linear::new(Point::new(0.0, hue_y), Point::new(width, hue_y));
        for step in 0..=6 {
            let fraction = step as f32 / 6.0;
            hues = hues.add_stop(
                fraction,
                shown(Hsva {
                    hue: fraction * 360.0,
                    saturation: 1.0,
                    value: 1.0,
                    alpha: 1.0,
                }),
            );
        }
        frame.fill_rectangle(Point::new(0.0, hue_y), Size::new(width, BAR), hues);
        bar_marker(&mut frame, self.hsva.hue / 360.0 * width, hue_y);

        // alpha over a checkerboard, so see-through colors read as such
        let alpha_y = hue_y + BAR + GAP;
        let cell = BAR / 2.0;
        let columns = (width / cell).ceil() as usize;
        for column in 0..columns {
            let x = column as f32 * cell;
            for row in 0..2 {
                let light = (column + row) % 2 == 0;
                frame.fill_rectangle(
                    Point::new(x, (row as f32).mul_add(cell, alpha_y)),
                    Size::new(cell.min(width - x), cell),
                    if light { c.surface } else { c.line },
                );
            }
        }
        let solid = Color {
            a: 1.0,
            ..shown(self.hsva)
        };
        frame.fill_rectangle(
            Point::new(0.0, alpha_y),
            Size::new(width, BAR),
            gradient::Linear::new(Point::new(0.0, alpha_y), Point::new(width, alpha_y))
                .add_stop(0.0, Color { a: 0.0, ..solid })
                .add_stop(1.0, solid),
        );
        bar_marker(&mut frame, self.hsva.alpha * width, alpha_y);

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging.is_some() || cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A two-tone tick across a bar at `x`.
fn bar_marker(frame: &mut Frame, x: f32, y: f32) {
    let x = x.clamp(1.5, frame.width() - 1.5);
    let line = canvas::Path::line(Point::new(x, y - 2.0), Point::new(x, y + BAR + 2.0));
    frame.stroke(
        &line,
        canvas::Stroke::default()
            .with_width(4.0)
            .with_color(Color::BLACK),
    );
    frame.stroke(
        &line,
        canvas::Stroke::default()
            .with_width(2.0)
            .with_color(Color::WHITE),
    );
}

pub fn picker<Message>(hsva: Hsva) -> Canvas<Picker, Message>
where
    Message: From<Hsva> + 'static,
{
    Canvas::new(Picker { hsva })
        .width(Length::Fill)
        .height(Length::Fixed(HEIGHT))
}
