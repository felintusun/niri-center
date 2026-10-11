//! Colors the way niri reads them, parsed with the crate and version niri
//! uses, so a field accepts exactly what niri accepts (`#rgb`, `rgb()`,
//! `hsl()`, `oklch()`, CSS names...). Also the picker's HSV and the
//! gradient props a color row edits.

use crate::kdl::{ValueKind, parse_document, quoted};

/// A color, each channel in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// Any color niri accepts.
    pub fn parse(input: &str) -> Option<Self> {
        let parsed = csscolorparser::parse(input.trim()).ok()?;
        Some(Self {
            r: parsed.r,
            g: parsed.g,
            b: parsed.b,
            a: parsed.a,
        })
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque: what gets written.
    pub fn hex(self) -> String {
        let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
        let [r, g, b, a] = [self.r, self.g, self.b, self.a].map(byte);
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

/// Whether niri reads `input` as a color.
pub fn is_color(input: &str) -> bool {
    Rgba::parse(input).is_some()
}

/// Hue in degrees, the rest in 0..=1. Kept as HSV while picking so the hue
/// survives dragging through grey.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsva {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub alpha: f32,
}

impl Default for Hsva {
    fn default() -> Self {
        Self {
            hue: 0.0,
            saturation: 0.0,
            value: 1.0,
            alpha: 1.0,
        }
    }
}

impl Hsva {
    /// `color` as HSV, keeping `previous`'s hue when `color` is grey.
    pub fn from_rgba(color: Rgba, previous: Self) -> Self {
        let max = color.r.max(color.g).max(color.b);
        let min = color.r.min(color.g).min(color.b);
        let delta = max - min;
        let hue = if delta <= f32::EPSILON {
            previous.hue
        } else if (max - color.r).abs() <= f32::EPSILON {
            60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
        } else if (max - color.g).abs() <= f32::EPSILON {
            60.0 * ((color.b - color.r) / delta + 2.0)
        } else {
            60.0 * ((color.r - color.g) / delta + 4.0)
        };
        Self {
            hue,
            saturation: if max <= f32::EPSILON {
                previous.saturation
            } else {
                delta / max
            },
            value: max,
            alpha: color.a,
        }
    }

    pub fn rgba(self) -> Rgba {
        let c = self.value * self.saturation;
        let h = self.hue.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.value - c;
        Rgba {
            r: r + m,
            g: g + m,
            b: b + m,
            a: self.alpha,
        }
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque: what gets written.
    pub fn hex(self) -> String {
        self.rgba().hex()
    }
}

/// A few colors to start from: niri's own defaults first.
pub const SWATCHES: [&str; 8] = [
    "#7fc8ff",
    "#ffc87f",
    "#505050",
    "#9b0000",
    "#00000077",
    "#ffffff",
    "#1e1e2e",
    "#a6e3a1",
];

/// A gradient's props, split into what the row edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gradient {
    pub from: String,
    pub to: String,
    pub angle: String,
    /// `relative-to="workspace-view"`: one gradient across all windows.
    pub across_view: bool,
    /// Props the row does not edit (`in="oklch longer hue"`), kept as written.
    pub rest: Vec<(String, String)>,
}

/// One end of a gradient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GradientEnd {
    From,
    To,
}

impl GradientEnd {
    pub const fn label(self) -> &'static str {
        match self {
            Self::From => "From",
            Self::To => "To",
        }
    }
}

impl Gradient {
    /// The color at `end`.
    pub const fn end(&self, end: GradientEnd) -> &String {
        match end {
            GradientEnd::From => &self.from,
            GradientEnd::To => &self.to,
        }
    }

    /// The gradient with `color` at `end`.
    pub fn with_end(&self, end: GradientEnd, color: String) -> Self {
        let mut next = self.clone();
        match end {
            GradientEnd::From => next.from = color,
            GradientEnd::To => next.to = color,
        }
        next
    }

    pub fn parse(props: &str) -> Option<Self> {
        let doc = parse_document(&format!("gradient {props}")).ok()?;
        let node = doc.roots.first()?;
        let mut gradient = Self {
            from: String::new(),
            to: String::new(),
            angle: String::new(),
            across_view: false,
            rest: Vec::new(),
        };
        for prop in &node.props {
            let value = prop.value.kind.as_str().to_string();
            match prop.name.as_str() {
                "from" => gradient.from = value,
                "to" => gradient.to = value,
                "angle" => gradient.angle = value,
                "relative-to" => gradient.across_view = value == "workspace-view",
                _ => gradient.rest.push((
                    prop.name.clone(),
                    match &prop.value.kind {
                        ValueKind::Quoted(text) => quoted(text),
                        ValueKind::Bare(text) | ValueKind::Number(text) => text.clone(),
                    },
                )),
            }
        }
        Some(gradient)
    }

    pub fn is_valid(&self) -> bool {
        is_color(&self.from)
            && is_color(&self.to)
            && (self.angle.is_empty() || self.angle.parse::<f64>().is_ok())
    }

    /// The props as written back: `from="…" to="…" angle=…`.
    pub fn text(&self) -> String {
        let mut parts = vec![
            format!("from={}", quoted(&self.from)),
            format!("to={}", quoted(&self.to)),
        ];
        if !self.angle.trim().is_empty() {
            parts.push(format!("angle={}", self.angle.trim()));
        }
        if self.across_view {
            parts.push("relative-to=\"workspace-view\"".to_string());
        }
        parts.extend(
            self.rest
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        );
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niri_color_syntaxes_parse() {
        for input in [
            "#7fc8ff",
            "#0007",
            "#7fc8ff80",
            "rebeccapurple",
            "transparent",
            "rgb(255, 127, 0)",
            "hsl(200, 80%, 70%)",
            "oklch(70% 0.1 200)",
        ] {
            assert!(is_color(input), "{input}");
        }
        assert!(!is_color("#12345"));
        assert!(!is_color("blurple"));
    }

    #[test]
    fn hsva_round_trips_through_hex() {
        for input in ["#7fc8ff", "#ffc87f", "#505050", "#9b0000", "#00000077"] {
            let color = Rgba::parse(input).unwrap();
            assert_eq!(Hsva::from_rgba(color, Hsva::default()).hex(), input);
        }
    }

    #[test]
    fn grey_keeps_the_hue_it_was_dragged_from() {
        let red = Hsva {
            hue: 0.0,
            saturation: 1.0,
            value: 1.0,
            alpha: 1.0,
        };
        let blue = Hsva { hue: 240.0, ..red };
        let grey = Rgba {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let grey = Hsva::from_rgba(grey, blue);
        assert!((grey.hue - 240.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_gradient_keeps_props_it_does_not_edit() {
        let text = r##"from="#f00f" to="#0f05" angle=45 in="oklch longer hue""##;
        let gradient = Gradient::parse(text).unwrap();
        assert!(gradient.is_valid());
        assert_eq!(gradient.text(), text);
        let across = Gradient {
            across_view: true,
            ..gradient
        };
        assert!(across.text().contains("relative-to=\"workspace-view\""));
    }
}
