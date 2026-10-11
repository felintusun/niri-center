//! Colors and widget styles: dark and light, each with a high-contrast
//! variant for when the desktop asks for more contrast.
//!
//! The accents are niri's own defaults — the focus ring's `#7fc8ff` and the
//! border's `#ffc87f` (default-config.kdl, `layout > focus-ring` and
//! `layout > border`) — so the app speaks the compositor's visual language
//! instead of a toolkit default. Amber marks the latest change, the way niri
//! marks the border of the active window.
//!
//! Every color a widget draws comes from here, including the ones iced
//! derives for its built-in widgets ([`extended`]): text clears WCAG AA
//! (4.5:1) on everything it sits on, AAA (7:1) in high contrast, and the
//! outlines of fields, buttons and toggles clear 3:1 (WCAG 1.4.11). Tests
//! check each pairing.

use iced::theme::Mode;
use iced::theme::palette::{self, Pair};
use iced::widget::{button, container, text, text_input};
use iced::{Background, Border, Color, Font, Theme, color};

/// Config paths, keys, KDL and diffs. The browser demo names the face it
/// bundles, since a browser has no monospace font to fall back on.
#[cfg(not(target_arch = "wasm32"))]
pub const MONO: Font = Font::MONOSPACE;
#[cfg(target_arch = "wasm32")]
pub const MONO: Font = Font::with_name("Fira Mono");

/// Which of the app's color sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Palette {
    Dark,
    Light,
    HighContrastDark,
    HighContrastLight,
}

impl Palette {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dark => "niri-center dark",
            Self::Light => "niri-center light",
            Self::HighContrastDark => "niri-center dark, high contrast",
            Self::HighContrastLight => "niri-center light, high contrast",
        }
    }
}

/// One complete set of colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub palette: Palette,
    pub dark: bool,
    pub background: Color,
    /// Grouped rows, cards, the sidebar's selected entry.
    pub surface: Color,
    /// Hairlines between rows: decoration, not needed to find anything.
    pub line: Color,
    pub text: Color,
    pub muted: Color,
    /// Config identifiers: key combos, paths, KDL values.
    pub code: Color,
    pub accent: Color,
    /// The latest change, and what was there before it.
    pub pending: Color,
    pub error: Color,
    pub success: Color,
    /// Canvas fill behind keyboards, monitors and curves.
    pub canvas: Color,
    /// A button's fill, and its hovered and pressed shades.
    pub control: Color,
    pub control_hover: Color,
    pub control_pressed: Color,
    /// The outline of anything you can click or type into: 3:1 against the
    /// page and surfaces.
    pub edge: Color,
    /// Text on an accent fill (primary buttons, toggles) and an error fill.
    pub on_accent: Color,
    pub on_error: Color,
    /// KDL in the source pane and in diffs: node names, property names,
    /// strings, numbers and keywords. Comments and punctuation are muted.
    pub syntax_node: Color,
    pub syntax_property: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
}

pub const DARK: Colors = Colors {
    palette: Palette::Dark,
    dark: true,
    background: color!(0x1c1e23),
    surface: color!(0x282b32),
    line: color!(0x3b404b),
    text: color!(0xe6e8ec),
    muted: color!(0xa6adb9),
    code: color!(0xc5cfde),
    accent: color!(0x7fc8ff),
    pending: color!(0xffc87f),
    error: color!(0xff9890),
    success: color!(0x8fd9a0),
    canvas: color!(0x141519),
    control: color!(0x363a44),
    control_hover: color!(0x40454f),
    control_pressed: color!(0x4b515d),
    edge: color!(0x7e8695),
    on_accent: color!(0x0e1116),
    on_error: color!(0x0e1116),
    syntax_node: color!(0x8cc4f0),
    syntax_property: color!(0xd2b4f0),
    syntax_string: color!(0xa8d8a0),
    syntax_number: color!(0xf0bf86),
};

pub const LIGHT: Colors = Colors {
    palette: Palette::Light,
    dark: false,
    background: color!(0xeceef1),
    surface: color!(0xffffff),
    line: color!(0xd0d4db),
    text: color!(0x1d2025),
    muted: color!(0x565c68),
    code: color!(0x2c3747),
    accent: color!(0x1565b0),
    pending: color!(0x8f5200),
    error: color!(0xb3261e),
    success: color!(0x1c7537),
    canvas: color!(0xe4e7eb),
    control: color!(0xe3e6ea),
    control_hover: color!(0xd7dbe1),
    control_pressed: color!(0xc9ced6),
    edge: color!(0x7a818d),
    on_accent: color!(0xffffff),
    on_error: color!(0xffffff),
    syntax_node: color!(0x155e9e),
    syntax_property: color!(0x7a3e9d),
    syntax_string: color!(0x2f6b2a),
    syntax_number: color!(0x975000),
};

/// For a desktop that asks for more contrast: near black and white, AAA
/// text, and outlines as strong as text.
pub const HIGH_CONTRAST_DARK: Colors = Colors {
    palette: Palette::HighContrastDark,
    dark: true,
    background: color!(0x000000),
    surface: color!(0x141414),
    line: color!(0x8a8a8a),
    text: color!(0xffffff),
    muted: color!(0xe0e0e0),
    code: color!(0xffffff),
    accent: color!(0x9ad6ff),
    pending: color!(0xffd591),
    error: color!(0xffa8a8),
    success: color!(0xa6f0b8),
    canvas: color!(0x000000),
    control: color!(0x1f1f1f),
    control_hover: color!(0x303030),
    control_pressed: color!(0x424242),
    edge: color!(0xffffff),
    on_accent: color!(0x000000),
    on_error: color!(0x000000),
    syntax_node: color!(0x9ad6ff),
    syntax_property: color!(0xe8ccff),
    syntax_string: color!(0xb8f0b0),
    syntax_number: color!(0xffd591),
};

pub const HIGH_CONTRAST_LIGHT: Colors = Colors {
    palette: Palette::HighContrastLight,
    dark: false,
    background: color!(0xf0f0f0),
    surface: color!(0xffffff),
    line: color!(0x767676),
    text: color!(0x000000),
    muted: color!(0x262626),
    code: color!(0x000000),
    accent: color!(0x0a4675),
    pending: color!(0x573300),
    error: color!(0x850e0e),
    success: color!(0x0d4a20),
    canvas: color!(0xe8e8e8),
    control: color!(0xe6e6e6),
    control_hover: color!(0xd6d6d6),
    control_pressed: color!(0xc4c4c4),
    edge: color!(0x000000),
    on_accent: color!(0xffffff),
    on_error: color!(0xffffff),
    syntax_node: color!(0x0a4675),
    syntax_property: color!(0x551d74),
    syntax_string: color!(0x0d4a20),
    syntax_number: color!(0x573300),
};

pub const ALL: [&Colors; 4] = [&DARK, &LIGHT, &HIGH_CONTRAST_DARK, &HIGH_CONTRAST_LIGHT];

/// `a` moved `t` (0–1) of the way towards `b`.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(
        (b.r - a.r).mul_add(t, a.r),
        (b.g - a.g).mul_add(t, a.g),
        (b.b - a.b).mul_add(t, a.b),
    )
}

/// WCAG 2.x contrast between two colors, 1 to 21.
pub fn contrast(foreground: Color, background: Color) -> f32 {
    fn channel(value: f32) -> f32 {
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    fn luminance(color: Color) -> f32 {
        0.2126f32.mul_add(
            channel(color.r),
            0.7152f32.mul_add(channel(color.g), 0.0722 * channel(color.b)),
        )
    }
    let (a, b) = (luminance(foreground), luminance(background));
    let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Whichever of the theme's text colors reads best on `fill`: for labels
/// on shades drawn in canvases, where a fixed text color would not hold up.
pub fn readable_on(c: &Colors, fill: Color) -> Color {
    [c.text, c.on_accent, c.background]
        .into_iter()
        .max_by(|a, b| contrast(*a, fill).total_cmp(&contrast(*b, fill)))
        .unwrap_or(c.text)
}

/// The colors for the desktop's color scheme (dark when unknown) and its
/// contrast preference.
pub const fn pick(mode: Option<Mode>, high_contrast: bool) -> &'static Colors {
    match (mode, high_contrast) {
        (Some(Mode::Light), false) => &LIGHT,
        (Some(Mode::Light), true) => &HIGH_CONTRAST_LIGHT,
        (Some(Mode::Dark | Mode::None) | None, false) => &DARK,
        (Some(Mode::Dark | Mode::None) | None, true) => &HIGH_CONTRAST_DARK,
    }
}

/// The app theme for the desktop's color scheme and contrast preference.
pub fn for_mode(mode: Option<Mode>, high_contrast: bool) -> Theme {
    let colors = pick(mode, high_contrast);
    Theme::custom_with_fn(
        colors.palette.name(),
        iced::theme::Palette {
            background: colors.background,
            text: colors.text,
            primary: colors.accent,
            success: colors.success,
            warning: colors.pending,
            danger: colors.error,
        },
        |_| extended(colors),
    )
}

/// What iced's built-in widgets draw with (fields, pick lists, toggles,
/// checkboxes, sliders, scrollbars, menus), set from audited colors instead
/// of iced's generated shades. Fields fill with the page color and outline
/// in `edge`; menus and pick lists fill with `control`. iced draws field
/// and pick list placeholders in `secondary`, so that is the muted text
/// color: our own buttons do not use it.
pub fn extended(c: &Colors) -> palette::Extended {
    let pair = |color, text| Pair { color, text };
    palette::Extended {
        background: palette::Background {
            base: pair(c.background, c.text),
            weakest: pair(c.surface, c.text),
            weaker: pair(c.surface, c.text),
            weak: pair(c.control, c.text),
            neutral: pair(c.control_hover, c.text),
            strong: pair(c.edge, c.background),
            stronger: pair(c.edge, c.background),
            strongest: pair(c.text, c.background),
        },
        primary: palette::Primary {
            base: pair(c.accent, c.on_accent),
            weak: pair(mix(c.accent, c.background, 0.6), c.text),
            strong: pair(mix(c.accent, c.text, 0.25), c.on_accent),
        },
        secondary: palette::Secondary {
            base: pair(c.muted, c.background),
            weak: pair(c.muted, c.background),
            strong: pair(c.muted, c.background),
        },
        success: palette::Success {
            base: pair(c.success, c.on_accent),
            weak: pair(mix(c.success, c.background, 0.6), c.text),
            strong: pair(c.success, c.on_accent),
        },
        warning: palette::Warning {
            base: pair(c.pending, c.on_accent),
            weak: pair(mix(c.pending, c.background, 0.6), c.text),
            strong: pair(c.pending, c.on_accent),
        },
        danger: palette::Danger {
            base: pair(c.error, c.on_error),
            weak: pair(mix(c.error, c.background, 0.6), c.text),
            strong: pair(mix(c.error, c.text, 0.2), c.on_error),
        },
        is_dark: c.dark,
    }
}

/// The colors behind `theme`.
pub fn colors(theme: &Theme) -> &'static Colors {
    // known by their palette: called for every widget drawn, so nothing
    // is allocated
    let palette = theme.palette();
    ALL.into_iter()
        .find(|colors| {
            (colors.background, colors.text, colors.accent)
                == (palette.background, palette.text, palette.primary)
        })
        .unwrap_or_else(|| {
            if theme.extended_palette().is_dark {
                &DARK
            } else {
                &LIGHT
            }
        })
}

fn with(color: fn(&Colors) -> Color) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(color(colors(theme))),
    }
}

pub fn muted(theme: &Theme) -> text::Style {
    with(|c| c.muted)(theme)
}
pub fn code(theme: &Theme) -> text::Style {
    with(|c| c.code)(theme)
}
pub fn accent(theme: &Theme) -> text::Style {
    with(|c| c.accent)(theme)
}
pub fn pending(theme: &Theme) -> text::Style {
    with(|c| c.pending)(theme)
}
pub fn error(theme: &Theme) -> text::Style {
    with(|c| c.error)(theme)
}
/// A shadow, on the desktop. In a browser with neither WebGPU nor WebGL the
/// software renderer draws a blurred shadow as a solid slab, neither clips it to a
/// scrolled area nor repaints where one was, and which renderer a browser
/// gets is known only at run time, so there everything stays flat and edged.
const fn lift(shadow: iced::Shadow) -> iced::Shadow {
    if cfg!(target_arch = "wasm32") {
        iced::Shadow {
            color: Color::TRANSPARENT,
            offset: iced::Vector::ZERO,
            blur_radius: 0.0,
        }
    } else {
        shadow
    }
}

/// A group of setting rows: one card, edged, lifted off the page in light.
pub fn group(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(c.surface)),
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 8.0.into(),
        },
        // light lifts cards off the page the way GNOME and KDE do
        shadow: if c.dark {
            iced::Shadow::default()
        } else {
            lift(iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 3.0,
            })
        },
        ..container::Style::default()
    }
}

/// A row the latest change touched: tinted amber.
pub fn changed_row(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(Color {
            a: if c.dark { 0.12 } else { 0.14 },
            ..c.pending
        })),
        ..container::Style::default()
    }
}

/// The amber bar at a changed row's left edge, the way niri marks the
/// active window's border.
pub fn changed_bar(theme: &Theme) -> container::Style {
    container::background(colors(theme).pending)
}

/// A line a change takes out, or the line niri cannot read.
pub fn removed_row(theme: &Theme) -> container::Style {
    container::background(Color {
        a: 0.12,
        ..colors(theme).error
    })
}

/// Something that needs doing before anything else: niri cannot read
/// the config.
pub fn problem(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(Color { a: 0.10, ..c.error })),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

/// A line the change adds, in a diff or the source.
pub fn added_row(theme: &Theme) -> container::Style {
    container::background(Color {
        a: 0.12,
        ..colors(theme).success
    })
}

pub fn success(theme: &Theme) -> text::Style {
    with(|c| c.success)(theme)
}

/// Code and diagnostics: a recessed block.
pub fn code_block(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(c.canvas)),
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

/// One choice of a segmented control: the chosen one is filled with the
/// text color.
pub fn segment(chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let (fill, ink) = match (chosen, status) {
            (true, _) => (c.accent, c.on_accent),
            (false, button::Status::Hovered | button::Status::Pressed) => (c.control_hover, c.text),
            (false, button::Status::Active | button::Status::Disabled) => (c.control, c.text),
        };
        button::Style {
            background: Some(Background::Color(fill)),
            text_color: ink,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// The outline around a strip of segments.
pub fn segments(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        border: Border {
            color: c.edge,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

/// A row in the history list; the open one is tinted with the accent.
pub fn history_item(open: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let background = match (open, status) {
            (true, _) => Some(Background::Color(Color {
                a: if c.dark { 0.12 } else { 0.14 },
                ..c.accent
            })),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(Background::Color(c.control))
            }
            (false, button::Status::Active | button::Status::Disabled) => None,
        };
        button::Style {
            background,
            text_color: c.text,
            border: Border {
                color: if open { c.accent } else { Color::TRANSPARENT },
                width: if open { 1.0 } else { 0.0 },
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// A toast: the surface, tinted and edged with the status color, raised
/// off the page by a shadow.
pub fn toast(theme: &Theme, tint: fn(&Colors) -> Color) -> container::Style {
    let c = colors(theme);
    let color = tint(c);
    container::Style {
        background: Some(Background::Color(mix(c.surface, color, 0.12))),
        text_color: Some(c.text),
        border: Border {
            color,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: lift(iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        }),
        ..container::Style::default()
    }
}

/// Tooltips and other things floating over the page.
pub fn popup(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(c.surface)),
        text_color: Some(c.text),
        border: Border {
            color: c.edge,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

/// The header search: an outlined box around an icon and a bare field.
pub fn search_box(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        border: Border {
            color: c.edge,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

/// A text field drawn by its surroundings (the search box).
pub fn bare_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        ..text_input::default(theme, status)
    }
}

/// Scrollbars: a quiet scroller on no rail, brighter only while used.
pub fn scrollbar(
    theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::{Rail, Scroller, Status, Style};
    let c = colors(theme);
    let (vertical, horizontal) = match status {
        Status::Active { .. } => (mix(c.line, c.edge, 0.5), mix(c.line, c.edge, 0.5)),
        Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => (
            if is_vertical_scrollbar_hovered {
                c.muted
            } else {
                c.edge
            },
            if is_horizontal_scrollbar_hovered {
                c.muted
            } else {
                c.edge
            },
        ),
        Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => (
            if is_vertical_scrollbar_dragged {
                c.text
            } else {
                c.edge
            },
            if is_horizontal_scrollbar_dragged {
                c.text
            } else {
                c.edge
            },
        ),
    };
    let rail = |color: Color| Rail {
        background: None,
        border: Border::default(),
        scroller: Scroller {
            background: Background::Color(color),
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
        },
    };
    Style {
        container: container::Style::default(),
        vertical_rail: rail(vertical),
        horizontal_rail: rail(horizontal),
        gap: None,
        auto_scroll: iced::widget::scrollable::default(theme, status).auto_scroll,
    }
}

/// The band behind the search and the tabs, like a header bar.
pub fn header_band(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(if c.palette == Palette::Dark {
            mix(c.background, c.surface, 0.5)
        } else {
            c.surface
        })),
        ..container::Style::default()
    }
}

/// A hairline between header, tabs and columns.
pub fn hairline(theme: &Theme) -> container::Style {
    container::background(colors(theme).line)
}

/// A text-only button in the accent color, for inline actions (Revert).
pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    button::Style {
        background: None,
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => c.text,
            button::Status::Active => c.accent,
            button::Status::Disabled => c.muted,
        },
        ..button::Style::default()
    }
}

/// A whole list row that opens on click: quiet until hovered.
pub fn list_row(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(c.surface)),
            button::Status::Active | button::Status::Disabled => None,
        },
        text_color: c.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

/// A button's look from its fill and text, with an outline that clears 3:1
/// so it reads as a button on any surface.
fn filled(fill: Color, text: Color, edge: Color) -> button::Style {
    button::Style {
        background: Some(Background::Color(fill)),
        text_color: text,
        border: Border {
            color: edge,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// A disabled button: no fill, a quiet outline and muted text, which stays
/// readable (muted clears AA) instead of fading out.
fn unavailable(c: &Colors) -> button::Style {
    button::Style {
        background: None,
        text_color: c.muted,
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..button::Style::default()
    }
}

/// The everyday button.
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    match status {
        button::Status::Active => filled(c.control, c.text, c.edge),
        button::Status::Hovered => filled(c.control_hover, c.text, c.edge),
        button::Status::Pressed => filled(c.control_pressed, c.text, c.edge),
        button::Status::Disabled => unavailable(c),
    }
}

/// The one action a screen is for: Add, Keep, Import.
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let strong = mix(c.accent, c.text, 0.25);
    match status {
        button::Status::Active => filled(c.accent, c.on_accent, c.accent),
        button::Status::Hovered | button::Status::Pressed => filled(strong, c.on_accent, strong),
        button::Status::Disabled => unavailable(c),
    }
}

/// Discard and remove-everything actions.
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    // in dark a full red fill outweighs the primary button: tinted instead
    if c.palette == Palette::Dark {
        let fill = mix(c.surface, c.error, 0.16);
        let hover = mix(c.surface, c.error, 0.26);
        return match status {
            button::Status::Active => filled(fill, c.error, c.error),
            button::Status::Hovered | button::Status::Pressed => filled(hover, c.error, c.error),
            button::Status::Disabled => unavailable(c),
        };
    }
    let strong = mix(c.error, c.text, 0.2);
    match status {
        button::Status::Active => filled(c.error, c.on_error, c.error),
        button::Status::Hovered | button::Status::Pressed => filled(strong, c.on_error, strong),
        button::Status::Disabled => unavailable(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_is_known_by_its_palette() {
        for (mode, high_contrast) in [
            (Some(Mode::Dark), false),
            (Some(Mode::Light), false),
            (Some(Mode::Dark), true),
            (Some(Mode::Light), true),
        ] {
            let theme = for_mode(mode, high_contrast);
            assert_eq!(colors(&theme).palette, pick(mode, high_contrast).palette);
        }
    }
}
