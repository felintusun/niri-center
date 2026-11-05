//! What moves in the app, timed as niri times its own animations
//! (`niri_center::motion`): a switch's knob, a notice dropping in from the
//! top, a countdown running down. Each keeps its own clock in the widget
//! tree and asks for frames only while it moves. Until the first frame
//! (and in the headless tests, which draw none) everything shows at rest.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::time::Instant;
use iced::widget::toggler;
use iced::{
    Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    mouse, touch, window,
};
use niri_center::motion::Played;

use crate::El;
use crate::message::Message;

/// Where a movement is, `now`: from its start at `since` to 1 at rest. A
/// movement that never started, or with motion off, is at rest.
fn along(motion: Option<Played>, since: Option<Instant>, now: Option<Instant>) -> f32 {
    match (motion, since, now) {
        (Some(motion), Some(since), Some(now)) => motion.at(now.saturating_duration_since(since)),
        _ => 1.0,
    }
}

/// Whether a movement that started at `since` is still going `now`.
fn moving(motion: Option<Played>, since: Option<Instant>, now: Instant) -> bool {
    match (motion, since) {
        (Some(motion), Some(since)) => now.saturating_duration_since(since) < motion.duration(),
        _ => false,
    }
}

fn color_of(background: Background) -> Color {
    match background {
        Background::Color(color) => color,
        Background::Gradient(_) => Color::TRANSPARENT,
    }
}

// ---- the switch ----

/// An on/off switch whose knob slides, and whose track changes color, the
/// way niri moves a window (`window-movement`).
pub struct Switch<Toggle> {
    on: bool,
    on_toggle: Option<Toggle>,
    size: f32,
    motion: Option<Played>,
}

/// What a switch without a message sends: nothing, it is unavailable.
pub type Unavailable = fn(bool) -> Message;

/// A switch showing `on`, moving as `motion` says.
pub const fn switch(on: bool, motion: Option<Played>) -> Switch<Unavailable> {
    Switch {
        on,
        on_toggle: None,
        size: 20.0,
        motion,
    }
}

impl<Toggle> Switch<Toggle> {
    /// What flipping it sends; without one it is unavailable.
    pub fn on_toggle<Sends: Fn(bool) -> Message>(self, on_toggle: Sends) -> Switch<Sends> {
        Switch {
            on: self.on,
            on_toggle: Some(on_toggle),
            size: self.size,
            motion: self.motion,
        }
    }
}

struct SwitchState {
    /// The value the knob is going to.
    target: bool,
    /// Where the knob was when it set off, 0 off to 1 on.
    from: f32,
    since: Option<Instant>,
    now: Option<Instant>,
    hovered: bool,
}

impl SwitchState {
    fn position(&self, motion: Option<Played>) -> f32 {
        let to = if self.target { 1.0 } else { 0.0 };
        self.from + (to - self.from) * along(motion, self.since, self.now)
    }
}

impl<Toggle: Fn(bool) -> Message> Widget<Message, Theme, Renderer> for Switch<Toggle> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SwitchState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SwitchState {
            target: self.on,
            from: if self.on { 1.0 } else { 0.0 },
            since: None,
            now: None,
            hovered: false,
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(2.0 * self.size, self.size))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<SwitchState>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(on_toggle) = &self.on_toggle
                    && cursor.is_over(layout.bounds())
                {
                    shell.publish(on_toggle(!self.on));
                    shell.capture_event();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                // the value changed since the last frame: set off from
                // wherever the knob is, even halfway back
                if state.target != self.on {
                    state.from = state.position(self.motion);
                    state.target = self.on;
                    state.since = Some(*now);
                }
                state.now = Some(*now);
                if moving(self.motion, state.since, *now) {
                    shell.request_redraw();
                }
            }
            Event::Keyboard(_)
            | Event::Mouse(_)
            | Event::Window(_)
            | Event::Touch(_)
            | Event::InputMethod(_) => {}
        }
        let hovered = cursor.is_over(layout.bounds());
        if hovered != state.hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        match (cursor.is_over(layout.bounds()), self.on_toggle.is_some()) {
            (true, true) => mouse::Interaction::Pointer,
            (true, false) => mouse::Interaction::NotAllowed,
            (false, _) => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<SwitchState>();
        let position = state.position(self.motion);
        // the look of the switch off and on, blended by where the knob is
        let look = |is_toggled| {
            let status = if self.on_toggle.is_none() {
                toggler::Status::Disabled { is_toggled }
            } else if state.hovered {
                toggler::Status::Hovered { is_toggled }
            } else {
                toggler::Status::Active { is_toggled }
            };
            toggler::default(theme, status)
        };
        let (off, on) = (look(false), look(true));
        let bounds = layout.bounds();
        let radius = bounds.height / 2.0;
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    radius: radius.into(),
                    width: off.background_border_width,
                    color: crate::theme::mix(
                        off.background_border_color,
                        on.background_border_color,
                        position,
                    ),
                },
                ..Quad::default()
            },
            crate::theme::mix(color_of(off.background), color_of(on.background), position),
        );
        let padding = (off.padding_ratio * bounds.height).round();
        let knob = bounds.height - 2.0 * padding;
        let travel = bounds.width - bounds.height;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x + padding + travel * position,
                    y: bounds.y + padding,
                    width: knob,
                    height: knob,
                },
                border: Border {
                    radius: (knob / 2.0).into(),
                    width: off.foreground_border_width,
                    color: crate::theme::mix(
                        off.foreground_border_color,
                        on.foreground_border_color,
                        position,
                    ),
                },
                ..Quad::default()
            },
            crate::theme::mix(color_of(off.foreground), color_of(on.foreground), position),
        );
    }
}

impl<'view, Toggle: Fn(bool) -> Message + 'view> From<Switch<Toggle>> for El<'view> {
    fn from(switch: Switch<Toggle>) -> Self {
        Element::new(switch)
    }
}

// ---- a notice dropping in ----

/// `content` dropping in from above the window when it first shows, the
/// way niri brings in its config notification
/// (`config-notification-open-close`): with that animation's spring, a
/// bouncy one by default.
pub fn drop_in<'view>(content: impl Into<El<'view>>, motion: Option<Played>) -> El<'view> {
    Element::new(DropIn {
        content: content.into(),
        motion,
    })
}

struct DropIn<'view> {
    content: El<'view>,
    motion: Option<Played>,
}

#[derive(Default)]
struct DropInState {
    since: Option<Instant>,
    now: Option<Instant>,
}

impl DropIn<'_> {
    /// How far the content is drawn from where it is laid out: from above
    /// the window's top edge down to its place, past it while it bounces.
    fn offset(&self, state: &DropInState, bounds: Rectangle) -> Vector {
        let rest = along(self.motion, state.since, state.now);
        Vector::new(0.0, -(bounds.y + bounds.height + 8.0) * (1.0 - rest))
    }
}

impl Widget<Message, Theme, Renderer> for DropIn<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DropInState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DropInState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // the content's tree is the one child `children` gave
        let Some(content) = tree.children.first_mut() else {
            return layout::Node::new(Size::ZERO);
        };
        self.content
            .as_widget_mut()
            .layout(content, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        if let Some(content) = tree.children.first_mut() {
            self.content
                .as_widget_mut()
                .operate(content, layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let state = tree.state.downcast_mut::<DropInState>();
            // its first frame is where it starts from
            let since = *state.since.get_or_insert(*now);
            state.now = Some(*now);
            if moving(self.motion, Some(since), *now) {
                shell.request_redraw();
            }
        }
        let offset = self.offset(tree.state.downcast_ref(), layout.bounds());
        let Some(content) = tree.children.first_mut() else {
            return;
        };
        self.content.as_widget_mut().update(
            content,
            event,
            layout,
            cursor - offset,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let offset = self.offset(tree.state.downcast_ref(), layout.bounds());
        let Some(content) = tree.children.first() else {
            return mouse::Interaction::default();
        };
        self.content.as_widget().mouse_interaction(
            content,
            layout,
            cursor - offset,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let offset = self.offset(tree.state.downcast_ref(), layout.bounds());
        let Some(content) = tree.children.first() else {
            return;
        };
        renderer.with_translation(offset, |renderer| {
            self.content.as_widget().draw(
                content,
                renderer,
                theme,
                style,
                layout,
                cursor - offset,
                viewport,
            );
        });
    }
}

// ---- a countdown ----

/// A bar running down `seconds_left` of `total`, smoothly between the
/// seconds it is told, in the theme's `pending` color. With motion off it
/// steps once a second.
pub fn countdown(seconds_left: u32, total: u32, motion: Option<Played>) -> El<'static> {
    Element::new(Countdown {
        seconds_left,
        total: total.max(1),
        smooth: motion.is_some(),
    })
}

struct Countdown {
    seconds_left: u32,
    total: u32,
    smooth: bool,
}

#[derive(Default)]
struct CountdownState {
    /// The second it last showed, and when that second began.
    shown: Option<(u32, Instant)>,
    now: Option<Instant>,
}

impl Countdown {
    fn fraction(&self, state: &CountdownState) -> f32 {
        let gone = match (self.smooth, state.shown, state.now) {
            (true, Some((_, began)), Some(now)) => {
                now.saturating_duration_since(began).as_secs_f32().min(1.0)
            }
            _ => 0.0,
        };
        ((self.seconds_left as f32 - gone) / self.total as f32).clamp(0.0, 1.0)
    }
}

impl Widget<Message, Theme, Renderer> for Countdown {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<CountdownState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(CountdownState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(3.0))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.width(Length::Fill).height(3.0).max())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let state = tree.state.downcast_mut::<CountdownState>();
            let began = match state.shown {
                Some((second, began)) if second == self.seconds_left => began,
                Some(_) | None => *now,
            };
            state.shown = Some((self.seconds_left, began));
            state.now = Some(*now);
            // the bar loses one of its pixels at a time: a frame only when
            // it has; the next second's tick redraws by itself
            let width = layout.bounds().width.max(1.0);
            let per_pixel = self.total as f32 / width;
            let gone = now.saturating_duration_since(began).as_secs_f32();
            if self.smooth && self.seconds_left > 0 && gone < 1.0 {
                shell.request_redraw_at(*now + std::time::Duration::from_secs_f32(per_pixel));
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let colors = crate::theme::colors(theme);
        let bounds = layout.bounds();
        renderer.fill_quad(
            Quad {
                bounds,
                ..Quad::default()
            },
            colors.line,
        );
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    width: bounds.width * self.fraction(tree.state.downcast_ref()),
                    ..bounds
                },
                ..Quad::default()
            },
            colors.pending,
        );
    }
}
