//! Every animation niri has, with its default, the easing curves niri
//! accepts, and the Animations page's editor.

use crate::store::Store;
use crate::words::short_number;

/// An easing curve niri accepts (verified against `niri validate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Curve {
    Linear,
    EaseOutQuad,
    EaseOutCubic,
    EaseOutExpo,
    /// Takes four control points.
    CubicBezier,
}

impl Curve {
    pub const ALL: [Self; 5] = [
        Self::Linear,
        Self::EaseOutQuad,
        Self::EaseOutCubic,
        Self::EaseOutExpo,
        Self::CubicBezier,
    ];

    /// The name niri reads in the config.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::EaseOutQuad => "ease-out-quad",
            Self::EaseOutCubic => "ease-out-cubic",
            Self::EaseOutExpo => "ease-out-expo",
            Self::CubicBezier => "cubic-bezier",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseOutQuad => "Ease out, gentle",
            Self::EaseOutCubic => "Ease out",
            Self::EaseOutExpo => "Ease out, sharp",
            Self::CubicBezier => "Custom curve",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|curve| curve.name() == name)
    }

    /// Control points (x1, y1, x2, y2) drawing a named curve. The custom
    /// curve's are the user's.
    pub const fn control_points(self) -> Option<(f32, f32, f32, f32)> {
        match self {
            Self::Linear => Some((0.0, 0.0, 1.0, 1.0)),
            Self::EaseOutQuad => Some((0.25, 0.46, 0.45, 0.94)),
            Self::EaseOutCubic => Some((0.215, 0.61, 0.355, 1.0)),
            Self::EaseOutExpo => Some((0.16, 1.0, 0.3, 1.0)),
            Self::CubicBezier => None,
        }
    }
}

/// How niri animates something when the config leaves it alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    Spring {
        damping: f64,
        stiffness: f64,
        epsilon: f64,
    },
    Easing {
        curve: Curve,
        duration_ms: u32,
    },
}

/// Every animation niri has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Animation {
    WorkspaceSwitch,
    HorizontalViewMovement,
    WindowOpen,
    WindowClose,
    WindowMovement,
    WindowResize,
    OverviewOpenClose,
    RecentWindowsClose,
    ScreenshotUiOpen,
    ConfigNotificationOpenClose,
    ExitConfirmationOpenClose,
}

impl Animation {
    pub const ALL: [Self; 11] = [
        Self::WorkspaceSwitch,
        Self::HorizontalViewMovement,
        Self::WindowOpen,
        Self::WindowClose,
        Self::WindowMovement,
        Self::WindowResize,
        Self::OverviewOpenClose,
        Self::RecentWindowsClose,
        Self::ScreenshotUiOpen,
        Self::ConfigNotificationOpenClose,
        Self::ExitConfirmationOpenClose,
    ];

    /// The name niri reads in the config.
    pub const fn name(self) -> &'static str {
        match self {
            Self::WorkspaceSwitch => "workspace-switch",
            Self::HorizontalViewMovement => "horizontal-view-movement",
            Self::WindowOpen => "window-open",
            Self::WindowClose => "window-close",
            Self::WindowMovement => "window-movement",
            Self::WindowResize => "window-resize",
            Self::OverviewOpenClose => "overview-open-close",
            Self::RecentWindowsClose => "recent-windows-close",
            Self::ScreenshotUiOpen => "screenshot-ui-open",
            Self::ConfigNotificationOpenClose => "config-notification-open-close",
            Self::ExitConfirmationOpenClose => "exit-confirmation-open-close",
        }
    }

    /// What people call it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::WorkspaceSwitch => "Switching workspaces",
            Self::HorizontalViewMovement => "Scrolling the view",
            Self::WindowOpen => "Opening a window",
            Self::WindowClose => "Closing a window",
            Self::WindowMovement => "Moving a window",
            Self::WindowResize => "Resizing a window",
            Self::OverviewOpenClose => "Opening the overview",
            Self::RecentWindowsClose => "Closing the window switcher",
            Self::ScreenshotUiOpen => "Opening the screenshot tool",
            Self::ConfigNotificationOpenClose => "The config notice",
            Self::ExitConfirmationOpenClose => "The exit dialog",
        }
    }

    /// niri's default (niri-config, `impl Default for …Anim`).
    pub const fn default_motion(self) -> Motion {
        match self {
            Self::WorkspaceSwitch => spring(1.0, 1000.0, 0.0001),
            Self::WindowOpen => easing(Curve::EaseOutExpo, 150),
            Self::WindowClose => easing(Curve::EaseOutQuad, 150),
            Self::HorizontalViewMovement
            | Self::WindowMovement
            | Self::WindowResize
            | Self::OverviewOpenClose => spring(1.0, 800.0, 0.0001),
            Self::RecentWindowsClose => spring(1.0, 800.0, 0.001),
            Self::ScreenshotUiOpen => easing(Curve::EaseOutQuad, 200),
            Self::ConfigNotificationOpenClose => spring(0.6, 1000.0, 0.001),
            Self::ExitConfirmationOpenClose => spring(0.6, 500.0, 0.01),
        }
    }
}

const fn spring(damping: f64, stiffness: f64, epsilon: f64) -> Motion {
    Motion::Spring {
        damping,
        stiffness,
        epsilon,
    }
}

const fn easing(curve: Curve, duration_ms: u32) -> Motion {
    Motion::Easing { curve, duration_ms }
}

/// The Animations page's editor: the animation it shows and the values in
/// its fields, as typed.
#[derive(Debug, Clone)]
pub struct Editor {
    pub animation: Animation,
    /// Edited as a spring, or as an easing curve.
    pub spring: bool,
    pub curve: Curve,
    pub bezier: [String; 4],
    pub duration: String,
    pub damping: f32,
    pub stiffness: f32,
}

impl Default for Editor {
    /// The first animation, as niri plays it by default.
    fn default() -> Self {
        Self::load(None, Animation::ALL[0])
    }
}

impl Editor {
    /// `animation` as the config sets it, or as niri does by default when
    /// the config leaves it alone.
    pub fn load(store: Option<&Store>, animation: Animation) -> Self {
        let name = animation.name();
        let default = animation.default_motion();
        let spring = store.and_then(|s| s.spring_params(name));
        let curve = store.and_then(|s| s.easing_curve(name));
        let (default_curve, default_duration) = match default {
            Motion::Easing { curve, duration_ms } => (curve, duration_ms),
            Motion::Spring { .. } => (Curve::EaseOutCubic, 250),
        };
        let (damping, stiffness) = match (spring, default) {
            (Some((damping, stiffness, _)), _) => (damping, stiffness),
            (
                None,
                Motion::Spring {
                    damping, stiffness, ..
                },
            ) => (damping, stiffness),
            (None, Motion::Easing { .. }) => (1.0, 800.0),
        };
        let points = store
            .and_then(|s| s.bezier_points(name))
            .unwrap_or([0.25, 0.1, 0.25, 1.0]);
        Self {
            animation,
            spring: match (&spring, &curve) {
                (Some(_), _) => true,
                (None, Some(_)) => false,
                (None, None) => matches!(default, Motion::Spring { .. }),
            },
            curve: curve
                .as_deref()
                .and_then(Curve::from_name)
                .unwrap_or(default_curve),
            bezier: points.map(short_number),
            duration: store
                .and_then(|s| s.duration_ms(name))
                .unwrap_or(default_duration)
                .to_string(),
            damping: damping as f32,
            stiffness: stiffness as f32,
        }
    }

    /// The same animation, read again from `store`: after it changed under
    /// the editor.
    pub fn reload(&mut self, store: Option<&Store>) {
        *self = Self::load(store, self.animation);
    }

    /// Write the easing curve from the fields. Gives what happened, or
    /// what is wrong with the fields.
    pub fn set_curve(&self, store: &mut Store) -> Result<String, String> {
        let duration = self
            .duration
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|ms| (1..=10_000).contains(ms))
            .ok_or("Enter a duration from 1 to 10000 milliseconds.")?;
        let points = if self.curve == Curve::CubicBezier {
            let parsed: Vec<f64> = self
                .bezier
                .iter()
                .filter_map(|point| point.trim().parse::<f64>().ok())
                .collect();
            match <[f64; 4]>::try_from(parsed) {
                Ok(points)
                    if (0.0..=1.0).contains(&points[0]) && (0.0..=1.0).contains(&points[2]) =>
                {
                    Some(points)
                }
                _ => {
                    return Err(
                        "A custom curve needs four numbers; the first and third run from 0 to 1."
                            .to_string(),
                    );
                }
            }
        } else {
            None
        };
        store
            .set_easing(
                self.animation.name(),
                self.curve.name(),
                points,
                Some(duration),
            )
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "{} now eases over {duration} ms.",
            self.animation.label()
        ))
    }

    /// Write the spring in the fields, keeping the config's epsilon.
    pub fn set_spring(&self, store: &mut Store) -> Result<String, String> {
        let epsilon = store.spring_params(self.animation.name()).map_or_else(
            || match self.animation.default_motion() {
                Motion::Spring { epsilon, .. } => epsilon,
                Motion::Easing { .. } => 0.0001,
            },
            |(_, _, epsilon)| epsilon,
        );
        // two decimals and whole numbers, as the sliders show them
        let damping = (f64::from(self.damping) * 100.0).round() / 100.0;
        let stiffness = f64::from(self.stiffness).round();
        store
            .set_spring_params(self.animation.name(), damping, stiffness, epsilon)
            .map_err(|e| e.to_string())?;
        Ok(format!("{} now uses a spring.", self.animation.label()))
    }

    /// Turn the animation on or off.
    pub fn set_playing(&self, store: &mut Store, on: bool) -> Result<String, String> {
        store
            .set_animation_off(self.animation.name(), !on)
            .map_err(|e| e.to_string())?;
        let name = self.animation.label();
        Ok(if on {
            format!("{name} plays again.")
        } else {
            format!("{name} is turned off.")
        })
    }

    /// Take the animation out of the config: niri's default applies.
    pub fn reset(&self, store: &mut Store) -> Result<String, String> {
        store
            .reset_animation(self.animation.name())
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "{} is back to niri's default.",
            self.animation.label()
        ))
    }
}
