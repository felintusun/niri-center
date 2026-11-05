//! Monitor drag-drop canvas: arrange connected outputs by dragging, then stage
//! the result as `position x=… y=…` edits in the config (via the app's store).

use crate::message::Message;
use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Color, Length, Point, Rectangle, Renderer, Theme, mouse};
use niri_center::outputs::MonitorInfo;
use num_traits::ToPrimitive;

/// Canvas program state persisted by iced between events.
#[derive(Default)]
pub struct MonitorsState {
    drag: Option<Drag>,
}

struct Drag {
    index: usize,
    /// Grab offset inside the monitor rectangle (canvas coordinates).
    offset: Point,
    cursor: Point,
}

/// A display's logical rectangle in the canvas's f32. Screen geometry is
/// far inside f32's range; one that is not is not drawn.
fn drawn(monitor: &MonitorInfo) -> Option<Rectangle> {
    Some(Rectangle {
        x: monitor.x.to_f32()?,
        y: monitor.y.to_f32()?,
        width: monitor.width.to_f32()?,
        height: monitor.height.to_f32()?,
    })
}

/// 2D mapping between logical config coordinates and canvas pixels.
struct Layout {
    scale: f32,
    origin: Point,
}

impl Layout {
    /// Fit logical bounding box into `bounds` with padding, centered.
    fn new(monitors: &[(String, MonitorInfo)], bounds: Rectangle) -> Option<Self> {
        if monitors.is_empty() || bounds.width < 1.0 || bounds.height < 1.0 {
            return None;
        }
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for monitor in monitors.iter().filter_map(|(_, monitor)| drawn(monitor)) {
            min_x = min_x.min(monitor.x);
            min_y = min_y.min(monitor.y);
            max_x = max_x.max(monitor.x + monitor.width);
            max_y = max_y.max(monitor.y + monitor.height);
        }
        let pad = 24.0;
        let bbox_w = (max_x - min_x).max(1.0);
        let bbox_h = (max_y - min_y).max(1.0);
        let scale = ((2.0f32.mul_add(-pad, bounds.width) / bbox_w)
            .min(2.0f32.mul_add(-pad, bounds.height) / bbox_h))
        .clamp(0.05, 2.0);
        // local coordinates: the frame is translated by the widget bounds at
        // draw time, so the origin is relative to the canvas corner
        let origin = Point::new(
            min_x.mul_add(-scale, bbox_w.mul_add(-scale, bounds.width) / 2.0),
            min_y.mul_add(-scale, bbox_h.mul_add(-scale, bounds.height) / 2.0),
        );
        Some(Self { scale, origin })
    }

    fn logical_to_canvas(&self, rect: Rectangle) -> Rectangle {
        Rectangle {
            x: rect.x.mul_add(self.scale, self.origin.x),
            y: rect.y.mul_add(self.scale, self.origin.y),
            width: rect.width * self.scale,
            height: rect.height * self.scale,
        }
    }

    fn canvas_to_logical(&self, point: Point) -> Point {
        // niri takes positions left of or above 0,0 as well
        Point::new(
            (point.x - self.origin.x) / self.scale,
            (point.y - self.origin.y) / self.scale,
        )
    }
}

/// A canvas rendering the monitor arrangement with drag interaction.
pub struct MonitorProgram {
    /// `(name, logical geometry)` in IPC order.
    pub monitors: Vec<(String, MonitorInfo)>,
    /// The display whose settings the page shows, outlined in the accent.
    pub selected: Option<String>,
}

fn label(content: String, position: Point, color: Color, size: f32) -> canvas::Text {
    canvas::Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: size.into(),
        line_height: LineHeight::default(),
        font: iced::Font::default(),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

impl Program<Message, Theme, Renderer> for MonitorProgram {
    type State = MonitorsState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let local = |point: Point| Point::new(point.x - bounds.x, point.y - bounds.y);
        let cursor_local = cursor.position().map(local);
        let layout = Layout::new(&self.monitors, bounds)?;

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor_local?;
                for (place, (_, monitor)) in self.monitors.iter().enumerate() {
                    let Some(logical) = drawn(monitor) else {
                        continue;
                    };
                    let rect = layout.logical_to_canvas(logical);
                    if rect.contains(pos) {
                        state.drag = Some(Drag {
                            index: place,
                            offset: Point::new(pos.x - rect.x, pos.y - rect.y),
                            cursor: pos,
                        });
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(drag) = &mut state.drag {
                    drag.cursor = local(*position);
                    return Some(canvas::Action::request_redraw());
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = state.drag.take() {
                    let pos = cursor.position().map_or(drag.cursor, local);
                    let logical = layout.canvas_to_logical(Point::new(
                        pos.x - drag.offset.x,
                        pos.y - drag.offset.y,
                    ));
                    let (name, _) = self.monitors.get(drag.index)?;
                    return Some(
                        canvas::Action::publish(Message::Displays(
                            crate::pages::displays::Message::Dropped(DroppedMonitor {
                                name: name.clone(),
                                x: logical.x,
                                y: logical.y,
                            }),
                        ))
                        .and_capture(),
                    );
                }
            }
            // non-drag events (keyboard, touch, window, and unhandled mouse)
            // never affect the arrangement
            canvas::Event::Mouse(_)
            | canvas::Event::Keyboard(_)
            | canvas::Event::Touch(_)
            | canvas::Event::Window(_)
            | canvas::Event::InputMethod(_) => {}
        }
        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let colors = crate::theme::colors(theme);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), colors.canvas);

        let Some(layout) = Layout::new(&self.monitors, bounds) else {
            return vec![frame.into_geometry()];
        };

        for (place, (name, monitor)) in self.monitors.iter().enumerate() {
            let Some(logical) = drawn(monitor) else {
                continue;
            };
            let drag = state.drag.as_ref().filter(|dragged| dragged.index == place);
            let rect = drag.map_or_else(
                || layout.logical_to_canvas(logical),
                |dragged| Rectangle {
                    x: dragged.cursor.x - dragged.offset.x,
                    y: dragged.cursor.y - dragged.offset.y,
                    width: logical.width * layout.scale,
                    height: logical.height * layout.scale,
                },
            );
            let selected = self.selected.as_ref() == Some(name);
            let (fill, edge, edge_width) = match (drag.is_some(), selected) {
                (true, _) => (
                    crate::theme::mix(colors.surface, colors.accent, 0.35),
                    colors.accent,
                    1.0,
                ),
                (false, true) => (colors.surface, colors.accent, 2.0),
                (false, false) => (colors.surface, colors.line, 1.0),
            };
            frame.fill_rectangle(rect.position(), rect.size(), fill);
            frame.stroke_rectangle(
                rect.position(),
                rect.size(),
                canvas::Stroke::default()
                    .with_width(edge_width)
                    .with_color(edge),
            );
            frame.fill_text(label(name.clone(), rect.center(), colors.text, 14.0));
            frame.fill_text(label(
                format!(
                    "{:.0} × {:.0} at {:.0}, {:.0}",
                    monitor.width, monitor.height, monitor.x, monitor.y
                ),
                Point::new(rect.center().x, rect.center().y + 20.0),
                colors.muted,
                11.0,
            ));
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Computed move request emitted on drop; the app stages it in the config.
#[derive(Debug, Clone)]
pub struct DroppedMonitor {
    pub name: String,
    /// Where it was let go, in logical pixels: snapping makes them whole.
    pub x: f32,
    pub y: f32,
}

/// The displays to arrange, `width` wide, the `selected` one outlined. Its
/// height follows the width, not the arrangement, so it stays put while
/// displays are dragged.
pub fn monitor_canvas(
    monitors: &[(String, MonitorInfo)],
    width: f32,
    selected: Option<&str>,
) -> Canvas<MonitorProgram, Message> {
    let height = if monitors.len() > 1 {
        (width * 0.45).clamp(200.0, 380.0)
    } else {
        220.0
    };
    Canvas::new(MonitorProgram {
        monitors: monitors.to_vec(),
        selected: selected.map(str::to_string),
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_a_display_reports_where_it_was_let_go_in_logical_pixels() {
        let program = MonitorProgram {
            monitors: vec![(
                "eDP-1".to_string(),
                MonitorInfo {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                },
            )],
            selected: None,
        };
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 300.0));
        let mut state = MonitorsState::default();
        let pressed = program.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(Point::new(200.0, 150.0)),
        );
        assert!(pressed.is_some());
        let dropped = program.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(Point::new(240.0, 150.0)),
        );
        // the width sets the scale, with 24 of padding each side: 352 / 1920,
        // so 40 canvas pixels right is 40 * 1920 / 352 logical pixels
        match dropped.map(|action| action.into_inner().0) {
            Some(Some(Message::Displays(crate::pages::displays::Message::Dropped(dropped)))) => {
                assert_eq!(dropped.name, "eDP-1");
                assert!((dropped.x - 218.18).abs() < 0.01, "x was {}", dropped.x);
                assert!(dropped.y.abs() < 0.01, "y was {}", dropped.y);
            }
            other => panic!("expected a dropped display, got {other:?}"),
        }
    }
}
