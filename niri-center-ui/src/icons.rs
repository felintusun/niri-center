//! The few line icons the app draws: on 16 by 16 grids, in the muted text
//! color (or a status color), inside buttons that also carry a tooltip or a
//! label saying what they do, or beside the words of a toast.

use iced::widget::canvas::{self, Canvas, Frame, Path, Program, Stroke};
use iced::{Point, Rectangle, Renderer, Theme, mouse};

use crate::{El, Message};

/// A canvas drawing kept from frame to frame until what it shows changes:
/// a redraw for every hover would tessellate it again each time, which the
/// browser demo's software renderer feels most. One drawing per palette,
/// so a change of theme needs none.
#[derive(Default)]
pub struct Kept {
    dark: canvas::Cache,
    light: canvas::Cache,
    high_contrast_dark: canvas::Cache,
    high_contrast_light: canvas::Cache,
    /// What the kept drawings were made from, hashed; `None` before the
    /// first frame.
    made_from: Option<u64>,
}

impl Kept {
    /// Before each frame, from the program's `update`: forget the drawings
    /// when `what` (all they show besides the theme and size) changed.
    pub fn follow(&mut self, event: &canvas::Event, what: impl std::hash::Hash) {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        let canvas::Event::Window(iced::window::Event::RedrawRequested(_)) = event else {
            return;
        };
        let made_from = BuildHasherDefault::<DefaultHasher>::default().hash_one(what);
        if self
            .made_from
            .replace(made_from)
            .is_some_and(|before| before != made_from)
        {
            for cache in [
                &self.dark,
                &self.light,
                &self.high_contrast_dark,
                &self.high_contrast_light,
            ] {
                cache.clear();
            }
        }
    }

    /// The drawing for `theme`'s palette, drawn again only after
    /// [`Self::follow`] found a change.
    pub fn draw(
        &self,
        renderer: &Renderer,
        theme: &Theme,
        size: iced::Size,
        draw: impl FnOnce(&mut Frame),
    ) -> canvas::Geometry {
        let cache = match palette_of(theme) {
            crate::theme::Palette::Dark => &self.dark,
            crate::theme::Palette::Light => &self.light,
            crate::theme::Palette::HighContrastDark => &self.high_contrast_dark,
            crate::theme::Palette::HighContrastLight => &self.high_contrast_light,
        };
        cache.draw(renderer, size, draw)
    }
}

/// The theme color an icon is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tint {
    Muted,
    Text,
    Success,
    Error,
}

impl Tint {
    fn of(self, colors: &crate::theme::Colors) -> iced::Color {
        match self {
            Self::Muted => colors.muted,
            Self::Text => colors.text,
            Self::Success => colors.success,
            Self::Error => colors.error,
        }
    }
}

/// Which colors a drawing uses: the theme's.
pub fn palette_of(theme: &Theme) -> crate::theme::Palette {
    crate::theme::colors(theme).palette
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Search,
    /// Remove an entry from a list.
    Remove,
    /// Remove a setting from the config, so niri's default applies.
    Reset,
    /// Hide the config column.
    Fold,
    /// Done: a toast for something that worked.
    Check,
    /// Something went wrong: a toast for an error.
    Alert,
    /// The sections in the sidebar.
    Keyboard,
    Display,
    Mouse,
    Pointer,
    Gesture,
    Columns,
    WindowRule,
    Workspaces,
    Overview,
    Curve,
    Startup,
    Environment,
    Camera,
    Clock,
    File,
    Folder,
    /// Open windows, one over another.
    Windows,
    /// niri's events as they come: a pulse.
    Events,
    /// The config text beside the page: `</>`.
    Source,
    /// The help card.
    Help,
    /// Fold the sidebar to icons, and open it again.
    Sidebar,
    /// Make something new: a rule.
    Add,
    /// Between the steps of a way back up: `All apps > Firefox`. Also a
    /// folded section.
    Chevron,
    /// An open section that folds.
    ChevronDown,
}

struct Drawing {
    icon: Icon,
    tint: Tint,
}

/// A rectangle on the 16 by 16 grid.
fn rect(x: f32, y: f32, width: f32, height: f32, scale: f32) -> Path {
    Path::rectangle(
        Point::new(x * scale, y * scale),
        iced::Size::new(width * scale, height * scale),
    )
}

/// Points on the 16 by 16 grid, joined into one stroke.
fn polyline(points: &[(f32, f32)], scale: f32) -> Path {
    Path::new(|path| {
        if let Some(((x, y), rest)) = points.split_first() {
            path.move_to(Point::new(x * scale, y * scale));
            for (grid_x, grid_y) in rest {
                path.line_to(Point::new(grid_x * scale, grid_y * scale));
            }
        }
    })
}

impl Drawing {
    fn strokes(&self, scale: f32) -> Vec<Path> {
        let line = |points: &[(f32, f32)]| polyline(points, scale);
        match self.icon {
            Icon::Search => vec![
                Path::circle(Point::new(7.0 * scale, 7.0 * scale), 4.5 * scale),
                line(&[(10.5, 10.5), (14.0, 14.0)]),
            ],
            Icon::Remove => vec![
                line(&[(4.0, 4.0), (12.0, 12.0)]),
                line(&[(12.0, 4.0), (4.0, 12.0)]),
            ],
            Icon::Reset => vec![
                Path::new(|path| {
                    let center = Point::new(8.0 * scale, 8.0 * scale);
                    path.arc(canvas::path::Arc {
                        center,
                        radius: 5.0 * scale,
                        start_angle: iced::Radians(std::f32::consts::PI),
                        end_angle: iced::Radians(std::f32::consts::PI * 2.75),
                    });
                }),
                line(&[(3.0, 2.5), (3.0, 8.0), (5.5, 6.0)]),
            ],
            Icon::Fold => vec![
                line(&[(5.0, 3.0), (10.0, 8.0), (5.0, 13.0)]),
                line(&[(13.0, 3.0), (13.0, 13.0)]),
            ],
            Icon::Check => vec![line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)])],
            Icon::Alert => vec![
                Path::circle(Point::new(8.0 * scale, 8.0 * scale), 6.5 * scale),
                line(&[(8.0, 4.5), (8.0, 8.5)]),
                line(&[(8.0, 11.0), (8.0, 11.2)]),
            ],
            Icon::Keyboard => vec![
                rect(1.5, 4.0, 13.0, 8.0, scale),
                line(&[(4.0, 6.5), (4.2, 6.5)]),
                line(&[(7.0, 6.5), (7.2, 6.5)]),
                line(&[(10.0, 6.5), (10.2, 6.5)]),
                line(&[(5.0, 9.5), (11.0, 9.5)]),
            ],
            Icon::Display => vec![
                rect(1.5, 2.5, 13.0, 8.5, scale),
                line(&[(8.0, 11.0), (8.0, 13.5)]),
                line(&[(5.0, 13.5), (11.0, 13.5)]),
            ],
            Icon::Mouse => vec![
                rect(4.5, 1.5, 7.0, 13.0, scale),
                line(&[(8.0, 1.5), (8.0, 6.0)]),
                line(&[(4.5, 6.0), (11.5, 6.0)]),
            ],
            Icon::Pointer => vec![line(&[
                (4.0, 2.0),
                (4.0, 13.0),
                (7.0, 10.0),
                (9.5, 14.5),
                (11.0, 13.8),
                (8.6, 9.2),
                (12.5, 9.0),
                (4.0, 2.0),
            ])],
            Icon::Gesture => vec![
                rect(1.5, 1.5, 13.0, 13.0, scale),
                line(&[(4.5, 8.5), (4.5, 4.5), (8.5, 4.5)]),
                line(&[(4.5, 4.5), (10.0, 10.0)]),
            ],
            Icon::Columns => vec![
                rect(1.5, 2.5, 4.0, 11.0, scale),
                rect(6.5, 2.5, 6.0, 11.0, scale),
                line(&[(14.5, 2.5), (14.5, 13.5)]),
            ],
            Icon::WindowRule => vec![
                rect(1.5, 2.5, 13.0, 11.0, scale),
                line(&[(1.5, 5.5), (14.5, 5.5)]),
                line(&[(4.5, 8.5), (11.5, 8.5)]),
                line(&[(4.5, 11.0), (9.0, 11.0)]),
            ],
            Icon::Workspaces => vec![
                rect(3.5, 1.5, 11.0, 7.0, scale),
                rect(1.5, 7.5, 11.0, 7.0, scale),
            ],
            Icon::Overview => vec![
                rect(1.5, 1.5, 5.5, 5.5, scale),
                rect(9.0, 1.5, 5.5, 5.5, scale),
                rect(1.5, 9.0, 5.5, 5.5, scale),
                rect(9.0, 9.0, 5.5, 5.5, scale),
            ],
            Icon::Curve => vec![
                Path::new(|path| {
                    path.move_to(Point::new(2.0 * scale, 14.0 * scale));
                    path.bezier_curve_to(
                        Point::new(7.0 * scale, 14.0 * scale),
                        Point::new(7.0 * scale, 2.0 * scale),
                        Point::new(14.0 * scale, 2.0 * scale),
                    );
                }),
                line(&[(2.0, 14.0), (2.0, 14.2)]),
            ],
            Icon::Startup => vec![
                Path::circle(Point::new(8.0 * scale, 8.0 * scale), 6.5 * scale),
                line(&[(6.5, 5.0), (11.0, 8.0), (6.5, 11.0), (6.5, 5.0)]),
            ],
            Icon::Environment => vec![
                line(&[
                    (5.5, 2.0),
                    (4.0, 3.0),
                    (4.0, 7.0),
                    (2.0, 8.0),
                    (4.0, 9.0),
                    (4.0, 13.0),
                    (5.5, 14.0),
                ]),
                line(&[
                    (10.5, 2.0),
                    (12.0, 3.0),
                    (12.0, 7.0),
                    (14.0, 8.0),
                    (12.0, 9.0),
                    (12.0, 13.0),
                    (10.5, 14.0),
                ]),
            ],
            Icon::Camera => vec![
                rect(1.5, 4.5, 13.0, 9.0, scale),
                line(&[(5.0, 4.5), (6.0, 2.5), (10.0, 2.5), (11.0, 4.5)]),
                Path::circle(Point::new(8.0 * scale, 9.0 * scale), 2.5 * scale),
            ],
            Icon::Clock => vec![
                Path::circle(Point::new(8.0 * scale, 8.0 * scale), 6.5 * scale),
                line(&[(8.0, 4.0), (8.0, 8.0), (11.0, 9.5)]),
            ],
            Icon::File => vec![
                line(&[
                    (3.5, 1.5),
                    (9.5, 1.5),
                    (12.5, 4.5),
                    (12.5, 14.5),
                    (3.5, 14.5),
                    (3.5, 1.5),
                ]),
                line(&[(9.5, 1.5), (9.5, 4.5), (12.5, 4.5)]),
            ],
            Icon::Folder => vec![line(&[
                (1.5, 3.0),
                (6.0, 3.0),
                (7.5, 4.5),
                (14.5, 4.5),
                (14.5, 13.0),
                (1.5, 13.0),
                (1.5, 3.0),
            ])],
            Icon::Windows => vec![
                rect(4.5, 1.5, 10.0, 8.0, scale),
                line(&[
                    (4.5, 6.5),
                    (1.5, 6.5),
                    (1.5, 14.5),
                    (11.5, 14.5),
                    (11.5, 9.5),
                ]),
            ],
            Icon::Events => vec![line(&[
                (1.0, 8.5),
                (4.5, 8.5),
                (6.5, 3.0),
                (9.5, 13.5),
                (11.5, 8.5),
                (15.0, 8.5),
            ])],
            Icon::Source => vec![
                line(&[(5.0, 4.0), (1.5, 8.0), (5.0, 12.0)]),
                line(&[(11.0, 4.0), (14.5, 8.0), (11.0, 12.0)]),
                line(&[(9.5, 2.5), (6.5, 13.5)]),
            ],
            Icon::Help => vec![
                Path::circle(Point::new(8.0 * scale, 8.0 * scale), 6.5 * scale),
                Path::new(|path| {
                    path.arc(canvas::path::Arc {
                        center: Point::new(8.0 * scale, 6.25 * scale),
                        radius: 2.0 * scale,
                        start_angle: iced::Radians(std::f32::consts::PI),
                        end_angle: iced::Radians(std::f32::consts::PI * 2.5),
                    });
                    path.line_to(Point::new(8.0 * scale, 9.5 * scale));
                }),
                line(&[(8.0, 11.75), (8.0, 11.95)]),
            ],
            Icon::Sidebar => vec![
                rect(1.5, 2.5, 13.0, 11.0, scale),
                line(&[(6.0, 2.5), (6.0, 13.5)]),
            ],
            Icon::Add => vec![
                line(&[(8.0, 3.0), (8.0, 13.0)]),
                line(&[(3.0, 8.0), (13.0, 8.0)]),
            ],
            Icon::Chevron => vec![line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)])],
            Icon::ChevronDown => vec![line(&[(4.0, 6.0), (8.0, 10.0), (12.0, 6.0)])],
        }
    }
}

impl Program<Message, Theme, Renderer> for Drawing {
    type State = Kept;

    fn update(
        &self,
        kept: &mut Kept,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        kept.follow(event, (self.icon, self.tint));
        None
    }

    fn draw(
        &self,
        kept: &Kept,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let color = self.tint.of(crate::theme::colors(theme));
        vec![kept.draw(renderer, theme, bounds.size(), |frame| {
            let scale = bounds.width.min(bounds.height) / 16.0;
            let stroke = Stroke::default()
                .with_width(1.5 * scale.max(1.0))
                .with_color(color)
                .with_line_cap(canvas::LineCap::Round)
                .with_line_join(canvas::LineJoin::Round);
            for path in self.strokes(scale) {
                frame.stroke(&path, stroke);
            }
        })]
    }
}

/// `icon` drawn `size` pixels square in the muted text color.
pub fn icon(icon: Icon, size: f32) -> El<'static> {
    tinted(icon, size, Tint::Muted)
}

/// `icon` in the text color: a sidebar entry's.
pub fn strong(icon: Icon, size: f32) -> El<'static> {
    tinted(icon, size, Tint::Text)
}

/// `icon` in one of the theme's colors.
pub fn tinted(icon: Icon, size: f32, tint: Tint) -> El<'static> {
    Canvas::new(Drawing { icon, tint })
        .width(size)
        .height(size)
        .into()
}
