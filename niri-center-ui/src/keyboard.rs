//! A clickable keyboard visualizer for the Keybinds page.
//!
//! The picture follows the configured xkb layout (see [`Layout`]). Keys bound
//! in the config are shaded and numbered by how many combos end on them;
//! clicking a key filters the keybind list to combos ending on that key.

use iced::widget::canvas::{self, Canvas, Program};
use iced::{Color, Length, Point, Rectangle, Renderer, Theme, mouse};
use std::collections::HashMap;

use niri_center::bindings::key_id;
use niri_center::keyboard::{Layout, key_label, key_width, same_key};

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

struct KeyRect {
    token: &'static str,
    rect: Rectangle,
}

fn compute_rects(layout: Layout, bounds: Rectangle) -> (Vec<KeyRect>, f32) {
    // local coordinates: the frame is translated by the widget's bounds at
    // draw time, so (0, 0) is the canvas origin
    let rows = layout.rows();
    let avail_w = (bounds.width - 2.0 * PAD).max(1.0);
    let avail_h = (bounds.height - 2.0 * PAD - NAV_GAP).max(1.0);
    let row_h = avail_h / rows.len() as f32;

    let mut rects = Vec::new();
    for (row, keys) in rows.iter().enumerate() {
        let units: f32 = keys.iter().map(|k| key_width(k)).sum();
        let key_w = avail_w / units;
        let nav_offset = if row + 1 == rows.len() { NAV_GAP } else { 0.0 };
        let y = PAD + row as f32 * row_h + nav_offset;
        let mut x = PAD;
        for token in *keys {
            let w = key_width(token) * key_w;
            rects.push(KeyRect {
                token,
                rect: Rectangle::new(
                    Point::new(x + KEY_GAP / 2.0, y),
                    iced::Size::new(w - KEY_GAP, row_h - KEY_GAP),
                ),
            });
            x += w;
        }
    }
    (rects, row_h)
}

/// Highlights keys by usage count; `selected` draws a ring and is clickable to
/// toggle the list filter.
pub struct KeyboardProgram {
    pub layout: Layout,
    pub counts: HashMap<String, usize>,
    pub selected: Option<String>,
}

/// Key fills by use: an unused key sits flush with the surface; used keys
/// take on the accent, more of it the more bindings end on them.
pub fn heat(c: &crate::theme::Colors) -> [Color; 3] {
    [
        c.surface,
        crate::theme::mix(c.surface, c.accent, 0.35),
        crate::theme::mix(c.surface, c.accent, 0.7),
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

impl<Message> Program<Message, Theme, Renderer> for KeyboardProgram
where
    Message: From<KeyClicked> + 'static,
{
    type State = crate::icons::Kept;

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let pressed = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor
                .position()
                .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y)),
            // all other events are irrelevant to the visualizer
            canvas::Event::Mouse(_)
            | canvas::Event::Keyboard(_)
            | canvas::Event::Window(_)
            | canvas::Event::Touch(_)
            | canvas::Event::InputMethod(_) => None,
        };
        if let Some(cursor) = pressed {
            let (keys, _) = compute_rects(self.layout, bounds);
            if let Some(hit) = keys.iter().find(|k| k.rect.contains(cursor)) {
                return Some(
                    canvas::Action::publish(Message::from(KeyClicked {
                        token: hit.token.to_string(),
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
        let mut counts: Vec<(&String, &usize)> = self.counts.iter().collect();
        counts.sort_unstable();
        let what = (
            self.layout,
            counts,
            &self.selected,
            crate::icons::palette_of(theme),
        );
        vec![kept.draw(renderer, bounds.size(), what, |frame| {
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

impl KeyboardProgram {
    fn draw_keys(&self, frame: &mut canvas::Frame, theme: &Theme, bounds: Rectangle) {
        let c = crate::theme::colors(theme);
        // heat: an unused key sits flush with the surface; used keys take on
        // the accent, more of it the more bindings end on them
        let [key_off, key_on, key_hot] = heat(c);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), c.canvas);
        let (keys, row_h) = compute_rects(self.layout, bounds);
        for key in &keys {
            let count = self.counts.get(&key_id(key.token)).copied().unwrap_or(0);
            let (key_color, text_color) = match count {
                0 => (key_off, c.muted),
                1 => (key_on, crate::theme::readable_on(c, key_on)),
                _ => (key_hot, crate::theme::readable_on(c, key_hot)),
            };
            let cap =
                canvas::Path::rounded_rectangle(key.rect.position(), key.rect.size(), 3.0.into());
            frame.fill(&cap, key_color);

            let text_size = (row_h * 0.40).clamp(9.0, 14.0);
            frame.fill_text(label(
                key_label(key.token),
                key.rect.center(),
                text_color,
                text_size,
            ));
            // bindings ending on this key, in the cap's top-right corner
            if count > 0 {
                let corner = Point::new(
                    key.rect.x + key.rect.width - text_size * 0.55,
                    key.rect.y + text_size * 0.55,
                );
                frame.fill_text(label(
                    &count.to_string(),
                    corner,
                    text_color,
                    text_size * 0.75,
                ));
            }

            if self
                .selected
                .as_deref()
                .is_some_and(|selected| same_key(selected, key.token))
            {
                frame.stroke(
                    &cap,
                    canvas::Stroke::default()
                        .with_width(2.0)
                        .with_color(c.pending),
                );
            }
        }
    }
}

/// Canvas of the visualizer; height is bounded by the caller.
pub fn keyboard_canvas<Message>(
    layout: Layout,
    counts: HashMap<String, usize>,
    selected: Option<String>,
) -> Canvas<KeyboardProgram, Message>
where
    Message: From<KeyClicked> + 'static,
{
    Canvas::new(KeyboardProgram {
        layout,
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
        Key::Character(c) => Some(match c {
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
            other => other.to_uppercase(),
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
