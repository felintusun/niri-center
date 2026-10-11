//! The few line icons the app draws: on 16 by 16 grids, in the muted text
//! color (or a status color), inside buttons that also carry a tooltip or a
//! label saying what they do, or beside the words of a toast.

use iced::widget::canvas::{self, Canvas, Frame, Path, Program, Stroke};
use iced::{Point, Rectangle, Renderer, Theme, mouse};

use crate::{El, Message};

/// A canvas drawing kept from frame to frame until what it shows changes:
/// a redraw for every hover would tessellate it again each time, which the
/// browser demo's software renderer feels most.
#[derive(Default)]
pub struct Kept {
    cache: canvas::Cache,
    /// What the kept drawing was made from, hashed.
    made_from: std::cell::Cell<u64>,
}

impl Kept {
    /// The drawing of `what` (anything it depends on besides its size),
    /// drawn again only when that changed.
    pub fn draw(
        &self,
        renderer: &Renderer,
        size: iced::Size,
        what: impl std::hash::Hash,
        draw: impl FnOnce(&mut Frame),
    ) -> canvas::Geometry {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        let made_from = BuildHasherDefault::<DefaultHasher>::default().hash_one(what);
        if self.made_from.replace(made_from) != made_from {
            self.cache.clear();
        }
        self.cache.draw(renderer, size, draw)
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
    /// Fold the sidebar to icons, and open it again.
    Sidebar,
    /// Make something new: a rule.
    Add,
    /// Between the steps of a way back up: `All apps > Firefox`.
    Chevron,
}

struct Drawing {
    icon: Icon,
    color: fn(&crate::theme::Colors) -> iced::Color,
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
            for (x, y) in rest {
                path.line_to(Point::new(x * scale, y * scale));
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
            Icon::Sidebar => vec![
                rect(1.5, 2.5, 13.0, 11.0, scale),
                line(&[(6.0, 2.5), (6.0, 13.5)]),
            ],
            Icon::Add => vec![
                line(&[(8.0, 3.0), (8.0, 13.0)]),
                line(&[(3.0, 8.0), (13.0, 8.0)]),
            ],
            Icon::Chevron => vec![line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)])],
        }
    }
}

impl Program<Message, Theme, Renderer> for Drawing {
    type State = Kept;

    fn draw(
        &self,
        kept: &Kept,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let color = (self.color)(crate::theme::colors(theme));
        let what = (self.icon, color.into_rgba8());
        vec![kept.draw(renderer, bounds.size(), what, |frame| {
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
pub fn icon<'a>(icon: Icon, size: f32) -> El<'a> {
    tinted(icon, size, |c| c.muted)
}

/// `icon` in the text color: a sidebar entry's.
pub fn strong<'a>(icon: Icon, size: f32) -> El<'a> {
    tinted(icon, size, |c| c.text)
}

/// `icon` in one of the theme's colors.
pub fn tinted<'a>(
    icon: Icon,
    size: f32,
    color: fn(&crate::theme::Colors) -> iced::Color,
) -> El<'a> {
    Canvas::new(Drawing { icon, color })
        .width(size)
        .height(size)
        .into()
}
