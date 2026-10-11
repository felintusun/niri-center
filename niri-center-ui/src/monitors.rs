//! Monitor drag-drop canvas: arrange connected outputs by dragging, then stage
//! the result as `position x=… y=…` edits in the config (via the app's store).

use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Color, Length, Point, Rectangle, Renderer, Theme, mouse};
use niri_center::outputs::MonitorInfo;

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
        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (_, m) in monitors {
            min_x = min_x.min(m.x);
            min_y = min_y.min(m.y);
            max_x = max_x.max(m.x + m.width);
            max_y = max_y.max(m.y + m.height);
        }
        let pad = 24.0;
        let bbox_w = (max_x - min_x).max(1.0) as f32;
        let bbox_h = (max_y - min_y).max(1.0) as f32;
        let scale = ((2.0f32.mul_add(-pad, bounds.width) / bbox_w)
            .min(2.0f32.mul_add(-pad, bounds.height) / bbox_h))
        .clamp(0.05, 2.0);
        // local coordinates: the frame is translated by the widget bounds at
        // draw time, so the origin is relative to the canvas corner
        let origin = Point::new(
            (min_x as f32).mul_add(-scale, bbox_w.mul_add(-scale, bounds.width) / 2.0),
            (min_y as f32).mul_add(-scale, bbox_h.mul_add(-scale, bounds.height) / 2.0),
        );
        Some(Self { scale, origin })
    }

    fn logical_to_canvas(&self, m: &MonitorInfo) -> Rectangle {
        Rectangle {
            x: (m.x as f32).mul_add(self.scale, self.origin.x),
            y: (m.y as f32).mul_add(self.scale, self.origin.y),
            width: m.width as f32 * self.scale,
            height: m.height as f32 * self.scale,
        }
    }

    fn canvas_to_logical(&self, p: Point) -> Point {
        // niri takes positions left of or above 0,0 as well
        Point::new(
            (p.x - self.origin.x) / self.scale,
            (p.y - self.origin.y) / self.scale,
        )
    }
}

/// A canvas rendering the monitor arrangement with drag interaction.
pub struct MonitorProgram {
    /// `(name, logical geometry)` in IPC order.
    pub monitors: Vec<(String, MonitorInfo)>,
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

impl<Message> Program<Message, Theme, Renderer> for MonitorProgram
where
    Message: From<DroppedMonitor> + 'static,
{
    type State = MonitorsState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let local = |p: Point| Point::new(p.x - bounds.x, p.y - bounds.y);
        let cursor_local = cursor.position().map(local);
        let layout = Layout::new(&self.monitors, bounds)?;

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor_local?;
                for (i, (_, m)) in self.monitors.iter().enumerate() {
                    let rect = layout.logical_to_canvas(m);
                    if rect.contains(pos) {
                        state.drag = Some(Drag {
                            index: i,
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
                    let pos = cursor.position().map(local).unwrap_or(drag.cursor);
                    let logical = layout.canvas_to_logical(Point::new(
                        pos.x - drag.offset.x,
                        pos.y - drag.offset.y,
                    ));
                    let (name, _) = &self.monitors[drag.index];
                    return Some(
                        canvas::Action::publish(Message::from(DroppedMonitor {
                            name: name.clone(),
                            x: logical.x.round() as i64,
                            y: logical.y.round() as i64,
                        }))
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
        let c = crate::theme::colors(theme);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), c.canvas);

        let Some(layout) = Layout::new(&self.monitors, bounds) else {
            return vec![frame.into_geometry()];
        };

        for (i, (name, m)) in self.monitors.iter().enumerate() {
            let drag = state.drag.as_ref().filter(|d| d.index == i);
            let rect = drag.map_or_else(
                || layout.logical_to_canvas(m),
                |d| Rectangle {
                    x: d.cursor.x - d.offset.x,
                    y: d.cursor.y - d.offset.y,
                    width: m.width as f32 * layout.scale,
                    height: m.height as f32 * layout.scale,
                },
            );
            let (fill, edge) = if drag.is_some() {
                (crate::theme::mix(c.surface, c.accent, 0.35), c.accent)
            } else {
                (c.surface, c.line)
            };
            frame.fill_rectangle(rect.position(), rect.size(), fill);
            frame.stroke_rectangle(
                rect.position(),
                rect.size(),
                canvas::Stroke::default().with_width(1.0).with_color(edge),
            );
            frame.fill_text(label(name.clone(), rect.center(), c.text, 14.0));
            frame.fill_text(label(
                format!(
                    "{} × {} at {}, {}",
                    m.width as i32, m.height as i32, m.x as i32, m.y as i32
                ),
                Point::new(rect.center().x, rect.center().y + 20.0),
                c.muted,
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
    pub x: i64,
    pub y: i64,
}

/// Convenience constructor for the page view.
pub fn monitor_canvas<Message>(
    monitors: &[(String, MonitorInfo)],
) -> Canvas<MonitorProgram, Message>
where
    Message: From<DroppedMonitor> + 'static,
{
    let height = if monitors.len() > 1 { 380.0 } else { 220.0 };
    Canvas::new(MonitorProgram {
        monitors: monitors.to_vec(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
}
