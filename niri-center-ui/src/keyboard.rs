//! A clickable keyboard visualizer for the Keybinds page.
//!
//! The picture follows the configured xkb layout (see [`Picture`]). Keys bound
//! in the config are shaded and numbered by how many combos end on them;
//! clicking a key filters the keybind list to combos ending on that key.

use crate::message::Message;
use iced::widget::canvas::{self, Canvas, Program};
use iced::{Color, Length, Point, Rectangle, Renderer, Theme, mouse};
use std::collections::HashMap;

use niri_center::bindings::key_id;
use niri_center::keyboard::{Key, Picture, key_width, same_key};

/// Which key is pressed/clicked; the app toggles the list filter on it.
#[derive(Debug, Clone)]
pub struct KeyClicked {
    pub token: String,
}

/// Space around the keys and between the main block and the navigation row.
const PAD: f32 = 8.0;
const NAV_GAP: f32 = 8.0;
/// Gap between neighbouring key caps, so adjacent bound keys stay distinct.
const KEY_GAP: f32 = 4.0;

struct KeyRect<'picture> {
    key: &'picture Key,
    rect: Rectangle,
}

fn compute_rects(picture: &Picture, bounds: Rectangle) -> (Vec<KeyRect<'_>>, f32) {
    // local coordinates: the frame is translated by the widget's bounds at
    // draw time, so (0, 0) is the canvas origin
    let rows = &picture.rows;
    let avail_w = (bounds.width - 2.0 * PAD).max(1.0);
    let avail_h = (bounds.height - 2.0 * PAD - NAV_GAP).max(1.0);
    let row_h = avail_h / rows.len() as f32;

    let mut rects = Vec::new();
    for (row, keys) in rows.iter().enumerate() {
        let units: f32 = keys.iter().map(|key| key_width(&key.token)).sum();
        let key_w = avail_w / units;
        let nav_offset = if row + 1 == rows.len() { NAV_GAP } else { 0.0 };
        let top = PAD + row as f32 * row_h + nav_offset;
        let mut left = PAD;
        for key in keys {
            let width = key_width(&key.token) * key_w;
            rects.push(KeyRect {
                key,
                rect: Rectangle::new(
                    Point::new(left + KEY_GAP / 2.0, top),
                    iced::Size::new(width - KEY_GAP, row_h - KEY_GAP),
                ),
            });
            left += width;
        }
    }
    (rects, row_h)
}

/// Highlights keys by usage count; `selected` draws a ring and is clickable to
/// toggle the list filter.
pub struct KeyboardProgram<'picture> {
    pub picture: &'picture Picture,
    pub counts: HashMap<String, usize>,
    pub selected: Option<String>,
}

/// Key fills by use: an unused key sits flush with the surface; used keys
/// take on the accent, more of it the more bindings end on them.
pub fn heat(colors: &crate::theme::Colors) -> [Color; 3] {
    [
        colors.surface,
        crate::theme::mix(colors.surface, colors.accent, 0.35),
        crate::theme::mix(colors.surface, colors.accent, 0.7),
    ]
}

fn label(token: &str, position: Point, color: Color, size: f32) -> canvas::Text {
    canvas::Text {
        content: token.to_string(),
        position,
        max_width: f32::INFINITY,
        color,
        size: size.into(),
        line_height: iced::widget::text::LineHeight::default(),
        font: iced::Font::default(),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        // advanced shaping finds bundled fonts too (the browser demo has no others)
        shaping: iced::widget::text::Shaping::Advanced,
    }
}

/// About how wide `content` draws at `size`. A canvas cannot measure text,
/// and the app's sans averages a little over half an em a glyph.
fn estimated_width(content: &str, size: f32) -> f32 {
    content.chars().count() as f32 * size * 0.6
}

impl Program<Message, Theme, Renderer> for KeyboardProgram<'_> {
    type State = crate::icons::Kept;

    fn update(
        &self,
        kept: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        // what the drawing shows besides the theme and its size, worked out
        // only before a frame: every pointer move comes through here too
        if matches!(
            event,
            canvas::Event::Window(iced::window::Event::RedrawRequested(_))
        ) {
            let mut counts: Vec<(&String, &usize)> = self.counts.iter().collect();
            counts.sort_unstable();
            kept.follow(event, (self.picture, counts, &self.selected));
        }
        let pressed = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor
                .position()
                .map(|point| Point::new(point.x - bounds.x, point.y - bounds.y)),
            // all other events are irrelevant to the visualizer
            canvas::Event::Mouse(_)
            | canvas::Event::Keyboard(_)
            | canvas::Event::Window(_)
            | canvas::Event::Touch(_)
            | canvas::Event::InputMethod(_) => None,
        };
        if let Some(cursor) = pressed {
            let (keys, _) = compute_rects(self.picture, bounds);
            if let Some(hit) = keys.iter().find(|key_rect| key_rect.rect.contains(cursor)) {
                return Some(
                    canvas::Action::publish(Message::from(KeyClicked {
                        token: hit.key.token.clone(),
                    }))
                    .and_capture(),
                );
            }
        }
        None
    }

    fn draw(
        &self,
        kept: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        vec![kept.draw(renderer, theme, bounds.size(), |frame| {
            self.draw_keys(frame, theme, bounds);
        })]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Pointer
    }
}

impl KeyboardProgram<'_> {
    fn draw_keys(&self, frame: &mut canvas::Frame, theme: &Theme, bounds: Rectangle) {
        let colors = crate::theme::colors(theme);
        // heat: an unused key sits flush with the surface; used keys take on
        // the accent, more of it the more bindings end on them
        let [key_off, key_on, key_hot] = heat(colors);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), colors.canvas);
        let (keys, row_h) = compute_rects(self.picture, bounds);
        for key in &keys {
            let count = self
                .counts
                .get(&key_id(&key.key.token))
                .copied()
                .unwrap_or(0);
            let (key_color, text_color) = match count {
                0 => (key_off, colors.muted),
                1 => (key_on, crate::theme::readable_on(colors, key_on)),
                _ => (key_hot, crate::theme::readable_on(colors, key_hot)),
            };
            let cap =
                canvas::Path::rounded_rectangle(key.rect.position(), key.rect.size(), 3.0.into());
            frame.fill(&cap, key_color);

            let text_size = (row_h * 0.40).clamp(9.0, 14.0);
            let count_size = text_size * 0.75;
            // bindings ending on this key sit in the cap's top-right corner
            // and the label takes the room left of them. A key too small for
            // both keeps its label: its fill still says it is used.
            let name = key.key.label.as_str();
            let name_width = estimated_width(name, text_size);
            let shown_count = (count > 0)
                .then(|| count.to_string())
                .map(|text| {
                    let room = estimated_width(&text, count_size) + 3.0;
                    (text, room)
                })
                .filter(|(_, room)| name_width + room <= key.rect.width - 4.0);
            let room = key.rect.width - 4.0 - shown_count.as_ref().map_or(0.0, |(_, room)| *room);
            let fitted = text_size * (room / name_width).min(1.0);
            frame.fill_text(label(
                name,
                Point::new(
                    key.rect.x + 2.0 + room / 2.0,
                    key.rect.y + key.rect.height / 2.0,
                ),
                text_color,
                fitted.max(7.0),
            ));
            if let Some((text, count_room)) = shown_count {
                let corner = Point::new(
                    key.rect.x + key.rect.width - count_room / 2.0 - 1.0,
                    key.rect.y + count_size * 0.75,
                );
                frame.fill_text(label(&text, corner, text_color, count_size));
            }

            if self
                .selected
                .as_deref()
                .is_some_and(|selected| same_key(selected, &key.key.token))
            {
                frame.stroke(
                    &cap,
                    canvas::Stroke::default()
                        .with_width(2.0)
                        .with_color(colors.pending),
                );
            }
        }
    }
}

/// Canvas of the visualizer; height is bounded by the caller.
pub fn keyboard_canvas(
    picture: &Picture,
    counts: HashMap<String, usize>,
    selected: Option<String>,
) -> Canvas<KeyboardProgram<'_>, Message> {
    Canvas::new(KeyboardProgram {
        picture,
        counts,
        selected,
    })
    .width(Length::Fill)
    .height(Length::Fixed(220.0))
}

/// The picture's name for a key pressed in the app: the key itself, with
/// Shift and the rest left out, the way a binding ends on it.
#[allow(clippy::wildcard_enum_match_arm)] // hundreds of keys the picture does not have
pub fn token_for(key: &iced::keyboard::Key) -> Option<String> {
    use iced::keyboard::{Key, key::Named};
    match key.as_ref() {
        Key::Character(typed) => Some(match typed {
            "-" => "Minus".to_string(),
            "=" => "Equal".to_string(),
            "`" => "Grave".to_string(),
            "," => "Comma".to_string(),
            "." => "Period".to_string(),
            "/" => "Slash".to_string(),
            ";" => "Semicolon".to_string(),
            "'" => "Apostrophe".to_string(),
            "[" => "BracketLeft".to_string(),
            "]" => "BracketRight".to_string(),
            "\\" => "BackSlash".to_string(),
            other => niri_center::keyboard::capital(other),
        }),
        Key::Named(named) => Some(
            match named {
                Named::Escape => "Escape",
                Named::PrintScreen => "Print",
                Named::Delete => "Delete",
                Named::Home => "Home",
                Named::End => "End",
                Named::PageUp => "Page_Up",
                Named::PageDown => "Page_Down",
                Named::ArrowLeft => "Left",
                Named::ArrowRight => "Right",
                Named::ArrowUp => "Up",
                Named::ArrowDown => "Down",
                Named::Tab => "Tab",
                Named::Enter => "Return",
                Named::Backspace => "BackSpace",
                Named::Space => "space",
                Named::F1 => "F1",
                Named::F2 => "F2",
                Named::F3 => "F3",
                Named::F4 => "F4",
                Named::F5 => "F5",
                Named::F6 => "F6",
                Named::F7 => "F7",
                Named::F8 => "F8",
                Named::F9 => "F9",
                Named::F10 => "F10",
                Named::F11 => "F11",
                Named::F12 => "F12",
                _ => return None,
            }
            .to_string(),
        ),
        Key::Unidentified => None,
    }
}
