//! Every simple setting the app edits, as data.
//!
//! One table says where each setting lives in the config, what kind of value
//! it takes, what niri does when it is unset, and how its row reads. The
//! settings pages, the search, the "Was" markers and the history's words are all
//! generated from it, and a test stages every entry and runs
//! `niri validate`, so a misspelled key fails the suite.
//!
//! The rows and the widgets that edit each kind of value are the UI's.

use std::collections::HashMap;
use std::path::Path;

use crate::color::{Gradient, is_color};
use crate::kdl::{ValueKind, quoted};
use crate::outputs::OutputInfo;
use crate::page::Page;
use crate::store::{Block, Store, StoreError};
use crate::system::SystemData;
use crate::words::who;

/// One editable setting.
pub struct Spec {
    pub page: Page,
    pub section: &'static str,
    pub label: &'static str,
    /// One plain sentence, or empty.
    pub help: &'static str,
    pub chain: &'static [Block<'static>],
    /// The node holding the value (unused for [`Kind::OnOff`]).
    pub child: &'static str,
    pub kind: Kind,
    /// What niri does when the config leaves this unset, in the words the
    /// control shows (`16`, `never`). Empty when niri leaves it to the device.
    /// For [`Kind::Gradient`], the gradient a new one starts from.
    pub default: &'static str,
    /// A setting that must be on for this one to matter.
    pub requires: Option<Setting>,
    /// A node beside this one that niri uses instead when it is set: a
    /// color's gradient.
    pub overridden_by: &'static str,
    /// What else the row should say, worked out from the config: facts
    /// that tie this setting to others.
    pub note: Option<fn(&Store) -> Option<Note>>,
}

/// Fields most rows leave alone.
const BASE: Spec = Spec {
    page: Page::Layout,
    section: "",
    label: "",
    help: "",
    chain: TOP,
    child: "",
    kind: Kind::Switch,
    default: "",
    requires: None,
    overridden_by: "",
    note: None,
};

/// A table of settings and the enum that names its rows, made from one
/// list so the two cannot drift apart: a name's number is its row.
macro_rules! table {
    ($(#[$meta:meta])* $name:ident, $table:ident; $($variant:ident => $spec:expr,)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)*
        }

        pub const $table: &[Spec] = &[$($spec,)*];

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            pub const fn spec(self) -> &'static Spec {
                &$table[self as usize]
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A presence flag such as `tap`.
    Switch,
    /// A section's `off` flag, shown the positive way: on means no `off`.
    Enabled,
    /// A block's `on` / `off` flags; with neither, niri's default applies.
    OnOff,
    /// One of a few strings, written quoted: `(value, label)`.
    Choice(&'static [(&'static str, &'static str)]),
    /// `true` or `false`, written bare: `drag false`.
    Bool,
    Integer {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
    },
    Color,
    /// The props of a gradient node: `from="#f00" to="#00f" angle=45`.
    Gradient,
    Text {
        example: &'static str,
    },
    /// A property on a flag node: `focus-follows-mouse max-scroll-amount="0%"`.
    Prop {
        name: &'static str,
        example: &'static str,
    },
    /// Column widths or window heights: `proportion 0.5`, `fixed 800`.
    Sizes {
        axis: Axis,
        /// A list to cycle through, or the one size new windows get.
        many: bool,
    },
    /// xkb layout codes, comma separated.
    Layouts,
    /// xkb options, comma separated.
    XkbOptions,
    /// An installed cursor theme.
    CursorTheme,
    /// The name of a connected output.
    Output,
    /// A program or file path; offers Browse.
    File {
        example: &'static str,
    },
    /// `screenshot-path`: a path with strftime codes, or `null`.
    Screenshot,
}

impl Kind {
    /// A stored value in the words its control shows: a choice's label, On
    /// or Off. `None` where the value is its own word.
    pub fn word(self, value: &str) -> Option<&'static str> {
        #[allow(clippy::wildcard_enum_match_arm)] // only these two have words of their own
        match self {
            Self::Choice(choices) => choices
                .iter()
                .find(|(code, _)| *code == value)
                .map(|(_, label)| *label),
            Self::Bool => match value {
                "true" => Some("On"),
                "false" => Some("Off"),
                _ => None,
            },
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Width,
    Height,
}

/// `on` / `off` state of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnOff {
    Default,
    On,
    Off,
}

/// A setting's value as the config holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Switch(bool),
    OnOff(OnOff),
    /// Decoded text; `None` when unset (niri's default applies).
    Value(Option<String>),
}

/// What a control asks for.
#[derive(Debug, Clone)]
pub enum Edit {
    Switch(bool),
    OnOff(OnOff),
    /// A pick from a list; `None` is "niri default", which removes the setting.
    Choice(Option<String>),
    /// Text typed into a field; staged as soon as it is valid.
    Draft(String),
    /// Take the setting out of the config, so niri's default applies.
    Unset,
}

/// Shown in pick lists for "remove the setting".
pub const NIRI_DEFAULT: &str = "Default";

/// What `screenshot-path` holds when screenshots are not saved.
pub const NO_FILE: &str = "null";

const TOP: &[Block<'static>] = &[];
const LAYOUT: &[Block<'static>] = &[Block::named("layout")];
const STRUTS: &[Block<'static>] = &[Block::named("layout"), Block::named("struts")];
const FOCUS_RING: &[Block<'static>] = &[Block::named("layout"), Block::named("focus-ring")];
const BORDER: &[Block<'static>] = &[Block::named("layout"), Block::named("border")];
const SHADOW: &[Block<'static>] = &[Block::named("layout"), Block::named("shadow")];
const TABS: &[Block<'static>] = &[Block::named("layout"), Block::named("tab-indicator")];
const INSERT_HINT: &[Block<'static>] = &[Block::named("layout"), Block::named("insert-hint")];
const INPUT: &[Block<'static>] = &[Block::named("input")];
const KEYBOARD: &[Block<'static>] = &[Block::named("input"), Block::named("keyboard")];
const XKB: &[Block<'static>] = &[
    Block::named("input"),
    Block::named("keyboard"),
    Block::named("xkb"),
];
const TOUCHPAD: &[Block<'static>] = &[Block::named("input"), Block::named("touchpad")];
const MOUSE: &[Block<'static>] = &[Block::named("input"), Block::named("mouse")];
const TABLET: &[Block<'static>] = &[Block::named("input"), Block::named("tablet")];
const TOUCH: &[Block<'static>] = &[Block::named("input"), Block::named("touch")];
const CURSOR: &[Block<'static>] = &[Block::named("cursor")];
const ANIMATIONS: &[Block<'static>] = &[Block::named("animations")];
const HOT_CORNERS: &[Block<'static>] = &[Block::named("gestures"), Block::named("hot-corners")];
const OVERVIEW: &[Block<'static>] = &[Block::named("overview")];
const WORKSPACE_SHADOW: &[Block<'static>] =
    &[Block::named("overview"), Block::named("workspace-shadow")];
const RECENT: &[Block<'static>] = &[Block::named("recent-windows")];
const RECENT_HIGHLIGHT: &[Block<'static>] =
    &[Block::named("recent-windows"), Block::named("highlight")];
const BLUR: &[Block<'static>] = &[Block::named("blur")];
const HOTKEY_OVERLAY: &[Block<'static>] = &[Block::named("hotkey-overlay")];
const CLIPBOARD: &[Block<'static>] = &[Block::named("clipboard")];
const XWAYLAND: &[Block<'static>] = &[Block::named("xwayland-satellite")];

const ACCEL_PROFILES: &[(&str, &str)] = &[("adaptive", "Adaptive"), ("flat", "Flat")];
const SCROLL_METHODS: &[(&str, &str)] = &[
    ("two-finger", "Two fingers"),
    ("edge", "Along the edge"),
    ("on-button-down", "Button held"),
    ("no-scroll", "Off"),
];
const SIDES: &[(&str, &str)] = &[
    ("left", "Left"),
    ("right", "Right"),
    ("top", "Top"),
    ("bottom", "Bottom"),
];
const THIRDS: &str = "proportion 0.33333; proportion 0.5; proportion 0.66667";

// Order within a page is the order on screen.
table! {
    /// A setting the app shows a row for.
    Setting, REGISTRY;
    // ---- Layout ----
    LayoutPresetColumnWidths => Spec {
        page: Page::Layout,
        section: "Columns",
        label: "Widths to step through",
        help: "Switch preset column width (Mod+R) cycles through these.",
        chain: LAYOUT,
        child: "preset-column-widths",
        kind: Kind::Sizes {
            axis: Axis::Width,
            many: true,
        },
        default: THIRDS,
        ..BASE
    },
    LayoutDefaultColumnWidth => Spec {
        page: Page::Layout,
        section: "Columns",
        label: "New columns open at",
        help: "Window rules can set their own width.",
        chain: LAYOUT,
        child: "default-column-width",
        kind: Kind::Sizes {
            axis: Axis::Width,
            many: false,
        },
        default: "proportion 0.5",
        ..BASE
    },
    LayoutPresetWindowHeights => Spec {
        page: Page::Layout,
        section: "Columns",
        label: "Heights to step through",
        help: "Switch preset window height (Mod+Shift+R) cycles through these.",
        chain: LAYOUT,
        child: "preset-window-heights",
        kind: Kind::Sizes {
            axis: Axis::Height,
            many: true,
        },
        default: THIRDS,
        ..BASE
    },
    LayoutCenterFocusedColumn => Spec {
        page: Page::Layout,
        section: "Columns",
        label: "Center the focused column",
        chain: LAYOUT,
        child: "center-focused-column",
        kind: Kind::Choice(&[
            ("never", "Never"),
            ("always", "Always"),
            ("on-overflow", "When it does not fit"),
        ]),
        default: "never",
        ..BASE
    },
    LayoutAlwaysCenterSingleColumn => Spec {
        page: Page::Layout,
        section: "Columns",
        label: "Center a lone column",
        chain: LAYOUT,
        child: "always-center-single-column",
        ..BASE
    },
    LayoutEmptyWorkspaceAboveFirst => Spec {
        page: Page::Workspaces,
        section: "Workspaces",
        label: "Keep an empty workspace above the first",
        chain: LAYOUT,
        child: "empty-workspace-above-first",
        ..BASE
    },
    LayoutDefaultColumnDisplay => Spec {
        page: Page::Layout,
        section: "Columns",
        label: "New columns show their windows",
        chain: LAYOUT,
        child: "default-column-display",
        kind: Kind::Choice(&[("normal", "Stacked"), ("tabbed", "As tabs")]),
        default: "normal",
        ..BASE
    },
    LayoutGaps => Spec {
        page: Page::Layout,
        section: "Spacing",
        label: "Gaps",
        help: "Space between windows and around the screen edge, in logical pixels.",
        chain: LAYOUT,
        child: "gaps",
        kind: Kind::Float {
            min: 0.0,
            max: 256.0,
        },
        default: "16",
        ..BASE
    },
    LayoutStrutsLeft => Spec {
        page: Page::Layout,
        section: "Spacing",
        label: "Extra space on the left",
        help: "Room kept free at the edge, so the next column can peek in.",
        chain: STRUTS,
        child: "left",
        kind: Kind::Integer { min: 0, max: 2000 },
        default: "0",
        ..BASE
    },
    LayoutStrutsRight => Spec {
        page: Page::Layout,
        section: "Spacing",
        label: "Extra space on the right",
        chain: STRUTS,
        child: "right",
        kind: Kind::Integer { min: 0, max: 2000 },
        default: "0",
        ..BASE
    },
    LayoutStrutsTop => Spec {
        page: Page::Layout,
        section: "Spacing",
        label: "Extra space at the top",
        chain: STRUTS,
        child: "top",
        kind: Kind::Integer { min: 0, max: 2000 },
        default: "0",
        ..BASE
    },
    LayoutStrutsBottom => Spec {
        page: Page::Layout,
        section: "Spacing",
        label: "Extra space at the bottom",
        chain: STRUTS,
        child: "bottom",
        kind: Kind::Integer { min: 0, max: 2000 },
        default: "0",
        ..BASE
    },
    LayoutFocusRing => Spec {
        page: Page::Layout,
        section: "Focus ring",
        label: "Focus ring",
        help: "Drawn around the focused window, behind it.",
        chain: FOCUS_RING,
        kind: Kind::OnOff,
        default: "on",
        note: Some(urgency_note),
        ..BASE
    },
    LayoutFocusRingWidth => Spec {
        page: Page::Layout,
        section: "Focus ring",
        label: "Width",
        chain: FOCUS_RING,
        child: "width",
        kind: Kind::Integer { min: 0, max: 64 },
        default: "4",
        requires: Some(Setting::LayoutFocusRing),
        ..BASE
    },
    LayoutFocusRingActiveColor => Spec {
        page: Page::Layout,
        section: "Focus ring",
        label: "Active color",
        chain: FOCUS_RING,
        child: "active-color",
        kind: Kind::Color,
        default: "#7fc8ff",
        requires: Some(Setting::LayoutFocusRing),
        overridden_by: "active-gradient",
        ..BASE
    },
    LayoutFocusRingActiveGradient => Spec {
        page: Page::Layout,
        section: "Focus ring",
        label: "Active gradient",
        help: "Used instead of the active color when set.",
        chain: FOCUS_RING,
        child: "active-gradient",
        kind: Kind::Gradient,
        default: "from=\"#80c8ff\" to=\"#bbddff\" angle=45",
        requires: Some(Setting::LayoutFocusRing),
        ..BASE
    },
    LayoutFocusRingInactiveColor => Spec {
        page: Page::Layout,
        section: "Focus ring",
        label: "Inactive color",
        help: "Used on outputs that do not have focus.",
        chain: FOCUS_RING,
        child: "inactive-color",
        kind: Kind::Color,
        default: "#505050",
        requires: Some(Setting::LayoutFocusRing),
        overridden_by: "inactive-gradient",
        ..BASE
    },
    LayoutFocusRingUrgentColor => Spec {
        page: Page::Layout,
        section: "Focus ring",
        label: "Urgent color",
        help: "For windows that ask for attention.",
        chain: FOCUS_RING,
        child: "urgent-color",
        kind: Kind::Color,
        default: "#9b0000",
        requires: Some(Setting::LayoutFocusRing),
        overridden_by: "urgent-gradient",
        ..BASE
    },
    LayoutBorder => Spec {
        page: Page::Layout,
        section: "Border",
        label: "Border",
        help: "Like the focus ring, but on every window and taking up space.",
        chain: BORDER,
        kind: Kind::OnOff,
        default: "off",
        note: Some(urgency_note),
        ..BASE
    },
    LayoutBorderWidth => Spec {
        page: Page::Layout,
        section: "Border",
        label: "Width",
        chain: BORDER,
        child: "width",
        kind: Kind::Integer { min: 0, max: 64 },
        default: "4",
        requires: Some(Setting::LayoutBorder),
        ..BASE
    },
    LayoutBorderActiveColor => Spec {
        page: Page::Layout,
        section: "Border",
        label: "Active color",
        chain: BORDER,
        child: "active-color",
        kind: Kind::Color,
        default: "#ffc87f",
        requires: Some(Setting::LayoutBorder),
        overridden_by: "active-gradient",
        ..BASE
    },
    LayoutBorderActiveGradient => Spec {
        page: Page::Layout,
        section: "Border",
        label: "Active gradient",
        help: "Used instead of the active color when set.",
        chain: BORDER,
        child: "active-gradient",
        kind: Kind::Gradient,
        default: "from=\"#ffbb66\" to=\"#ffc880\" angle=45",
        requires: Some(Setting::LayoutBorder),
        ..BASE
    },
    LayoutBorderInactiveColor => Spec {
        page: Page::Layout,
        section: "Border",
        label: "Inactive color",
        chain: BORDER,
        child: "inactive-color",
        kind: Kind::Color,
        default: "#505050",
        requires: Some(Setting::LayoutBorder),
        overridden_by: "inactive-gradient",
        ..BASE
    },
    LayoutBorderUrgentColor => Spec {
        page: Page::Layout,
        section: "Border",
        label: "Urgent color",
        help: "For windows that ask for attention.",
        chain: BORDER,
        child: "urgent-color",
        kind: Kind::Color,
        default: "#9b0000",
        requires: Some(Setting::LayoutBorder),
        overridden_by: "urgent-gradient",
        ..BASE
    },
    LayoutShadow => Spec {
        page: Page::Layout,
        section: "Shadow",
        label: "Shadow",
        chain: SHADOW,
        kind: Kind::OnOff,
        default: "off",
        ..BASE
    },
    LayoutShadowSoftness => Spec {
        page: Page::Layout,
        section: "Shadow",
        label: "Softness",
        help: "Blur radius in logical pixels.",
        chain: SHADOW,
        child: "softness",
        kind: Kind::Integer { min: 0, max: 256 },
        default: "30",
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowSpread => Spec {
        page: Page::Layout,
        section: "Shadow",
        label: "Spread",
        chain: SHADOW,
        child: "spread",
        kind: Kind::Integer { min: 0, max: 256 },
        default: "5",
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowColor => Spec {
        page: Page::Layout,
        section: "Shadow",
        label: "Color",
        chain: SHADOW,
        child: "color",
        kind: Kind::Color,
        default: "#00000077",
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowInactiveColor => Spec {
        page: Page::Layout,
        section: "Shadow",
        label: "Color of unfocused windows",
        chain: SHADOW,
        child: "inactive-color",
        kind: Kind::Color,
        default: "the same color",
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowDrawBehindWindow => Spec {
        page: Page::Layout,
        section: "Shadow",
        label: "Draw behind the window too",
        help: "For windows with see-through corners of their own.",
        chain: SHADOW,
        child: "draw-behind-window",
        kind: Kind::Bool,
        default: "false",
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutTabIndicator => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Tab indicator",
        help: "The strip beside a column that shows its windows as tabs.",
        chain: TABS,
        kind: Kind::OnOff,
        default: "on",
        ..BASE
    },
    LayoutTabIndicatorHideWhenSingleTab => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Hide it for a single tab",
        chain: TABS,
        child: "hide-when-single-tab",
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorPlaceWithinColumn => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Draw it inside the column",
        chain: TABS,
        child: "place-within-column",
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorPosition => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Side",
        chain: TABS,
        child: "position",
        kind: Kind::Choice(SIDES),
        default: "left",
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorWidth => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Width",
        chain: TABS,
        child: "width",
        kind: Kind::Integer { min: 0, max: 64 },
        default: "4",
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorGap => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Gap to the window",
        chain: TABS,
        child: "gap",
        kind: Kind::Float {
            min: -64.0,
            max: 64.0,
        },
        default: "5",
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorCornerRadius => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Corner radius",
        chain: TABS,
        child: "corner-radius",
        kind: Kind::Float {
            min: 0.0,
            max: 64.0,
        },
        default: "0",
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorActiveColor => Spec {
        page: Page::Layout,
        section: "Tabs",
        label: "Active color",
        chain: TABS,
        child: "active-color",
        kind: Kind::Color,
        default: "the focus ring's",
        requires: Some(Setting::LayoutTabIndicator),
        overridden_by: "active-gradient",
        ..BASE
    },
    LayoutInsertHint => Spec {
        page: Page::Layout,
        section: "Insert hint",
        label: "Insert hint",
        help: "Shows where a dragged window will land.",
        chain: INSERT_HINT,
        kind: Kind::OnOff,
        default: "on",
        ..BASE
    },
    LayoutInsertHintColor => Spec {
        page: Page::Layout,
        section: "Insert hint",
        label: "Color",
        chain: INSERT_HINT,
        child: "color",
        kind: Kind::Color,
        default: "#7fc8ff80",
        requires: Some(Setting::LayoutInsertHint),
        ..BASE
    },
    LayoutBackgroundColor => Spec {
        page: Page::Layout,
        section: "Background",
        label: "Workspace background",
        help: "Shown where no window or wallpaper covers the workspace.",
        chain: LAYOUT,
        child: "background-color",
        kind: Kind::Color,
        default: "#404040",
        ..BASE
    },
    // ---- Input ----
    InputKeyboardXkbLayout => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Layouts",
        help: "The first one is active at startup. With none, niri uses the system layout.",
        chain: XKB,
        child: "layout",
        kind: Kind::Layouts,
        ..BASE
    },
    InputKeyboardXkbVariant => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Variants",
        help: "One per layout, in the same order, comma separated.",
        chain: XKB,
        child: "variant",
        kind: Kind::Text {
            example: "nodeadkeys",
        },
        ..BASE
    },
    InputKeyboardXkbOptions => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Options",
        help: "For example a key that switches layouts.",
        chain: XKB,
        child: "options",
        kind: Kind::XkbOptions,
        ..BASE
    },
    InputModKey => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Mod key",
        help: "The key that Mod stands for in your shortcuts.",
        chain: INPUT,
        child: "mod-key",
        kind: Kind::Choice(&[
            ("Super", "Super"),
            ("Alt", "Alt"),
            ("Ctrl", "Ctrl"),
            ("Mod3", "Mod3"),
            ("Mod5", "Mod5"),
        ]),
        default: "Super",
        ..BASE
    },
    InputKeyboardRepeatDelay => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Repeat delay",
        help: "Milliseconds a key is held before it repeats.",
        chain: KEYBOARD,
        child: "repeat-delay",
        kind: Kind::Integer {
            min: 100,
            max: 2000,
        },
        default: "600",
        ..BASE
    },
    InputKeyboardRepeatRate => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Repeat rate",
        help: "Characters per second while a key repeats.",
        chain: KEYBOARD,
        child: "repeat-rate",
        kind: Kind::Integer { min: 1, max: 200 },
        default: "25",
        ..BASE
    },
    InputKeyboardNumlock => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Turn on Num Lock at startup",
        chain: KEYBOARD,
        child: "numlock",
        ..BASE
    },
    InputKeyboardTrackLayout => Spec {
        page: Page::Input,
        section: "Keyboard",
        label: "Remember the layout",
        chain: KEYBOARD,
        child: "track-layout",
        kind: Kind::Choice(&[("global", "For all windows"), ("window", "Per window")]),
        default: "global",
        ..BASE
    },
    InputTouchpad => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Touchpad",
        chain: TOUCHPAD,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    InputTouchpadTap => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Tap to click",
        chain: TOUCHPAD,
        child: "tap",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadNaturalScroll => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Natural scrolling",
        help: "Content follows your fingers.",
        chain: TOUCHPAD,
        child: "natural-scroll",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDwt => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Ignore the touchpad while typing",
        chain: TOUCHPAD,
        child: "dwt",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDwtp => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Ignore the touchpad while the pointing stick moves",
        chain: TOUCHPAD,
        child: "dwtp",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDrag => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Tap and drag",
        help: "Tap, then keep the finger down to drag.",
        chain: TOUCHPAD,
        child: "drag",
        kind: Kind::Bool,
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDragLock => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Keep dragging after lifting the finger",
        chain: TOUCHPAD,
        child: "drag-lock",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadAccelSpeed => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Pointer speed",
        help: "From -1 (slowest) to 1 (fastest).",
        chain: TOUCHPAD,
        child: "accel-speed",
        kind: Kind::Float {
            min: -1.0,
            max: 1.0,
        },
        default: "0",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadAccelProfile => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Acceleration",
        chain: TOUCHPAD,
        child: "accel-profile",
        kind: Kind::Choice(ACCEL_PROFILES),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadScrollMethod => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Scrolling",
        chain: TOUCHPAD,
        child: "scroll-method",
        kind: Kind::Choice(SCROLL_METHODS),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadScrollFactor => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Scroll speed",
        help: "Multiplies how far each swipe scrolls.",
        chain: TOUCHPAD,
        child: "scroll-factor",
        kind: Kind::Float {
            min: 0.0,
            max: 10.0,
        },
        default: "1",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadTapButtonMap => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Tapping with more fingers",
        help: "Which click a two- or three-finger tap makes.",
        chain: TOUCHPAD,
        child: "tap-button-map",
        kind: Kind::Choice(&[
            ("left-right-middle", "2 fingers right, 3 middle"),
            ("left-middle-right", "2 fingers middle, 3 right"),
        ]),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadClickMethod => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Right-click by",
        chain: TOUCHPAD,
        child: "click-method",
        kind: Kind::Choice(&[
            ("button-areas", "Bottom right corner"),
            ("clickfinger", "Two fingers"),
        ]),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadLeftHanded => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Left-handed",
        help: "Swaps the left and right buttons.",
        chain: TOUCHPAD,
        child: "left-handed",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadMiddleEmulation => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Middle-click by pressing left and right together",
        chain: TOUCHPAD,
        child: "middle-emulation",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDisabledOnExternalMouse => Spec {
        page: Page::Input,
        section: "Touchpad",
        label: "Turn off while a mouse is connected",
        chain: TOUCHPAD,
        child: "disabled-on-external-mouse",
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputMouseNaturalScroll => Spec {
        page: Page::Input,
        section: "Mouse",
        label: "Natural scrolling",
        chain: MOUSE,
        child: "natural-scroll",
        ..BASE
    },
    InputMouseAccelSpeed => Spec {
        page: Page::Input,
        section: "Mouse",
        label: "Pointer speed",
        help: "From -1 (slowest) to 1 (fastest).",
        chain: MOUSE,
        child: "accel-speed",
        kind: Kind::Float {
            min: -1.0,
            max: 1.0,
        },
        default: "0",
        ..BASE
    },
    InputMouseAccelProfile => Spec {
        page: Page::Input,
        section: "Mouse",
        label: "Acceleration",
        chain: MOUSE,
        child: "accel-profile",
        kind: Kind::Choice(ACCEL_PROFILES),
        ..BASE
    },
    InputMouseScrollFactor => Spec {
        page: Page::Input,
        section: "Mouse",
        label: "Scroll speed",
        help: "Multiplies how far each wheel step scrolls.",
        chain: MOUSE,
        child: "scroll-factor",
        kind: Kind::Float {
            min: 0.0,
            max: 10.0,
        },
        default: "1",
        ..BASE
    },
    InputMouseLeftHanded => Spec {
        page: Page::Input,
        section: "Mouse",
        label: "Left-handed",
        help: "Swaps the left and right buttons.",
        chain: MOUSE,
        child: "left-handed",
        ..BASE
    },
    InputMouseMiddleEmulation => Spec {
        page: Page::Input,
        section: "Mouse",
        label: "Middle-click by pressing left and right together",
        chain: MOUSE,
        child: "middle-emulation",
        ..BASE
    },
    InputTabletMapToOutput => Spec {
        page: Page::Input,
        section: "Tablet and touchscreen",
        label: "The tablet covers",
        chain: TABLET,
        child: "map-to-output",
        kind: Kind::Output,
        default: "every display",
        ..BASE
    },
    InputTouchMapToOutput => Spec {
        page: Page::Input,
        section: "Tablet and touchscreen",
        label: "The touchscreen is",
        chain: TOUCH,
        child: "map-to-output",
        kind: Kind::Output,
        default: "built-in",
        ..BASE
    },
    InputFocusFollowsMouse => Spec {
        page: Page::Cursor,
        section: "Pointer and focus",
        label: "Focus follows the pointer",
        chain: INPUT,
        child: "focus-follows-mouse",
        ..BASE
    },
    InputFocusFollowsMouseMaxScrollAmount => Spec {
        page: Page::Cursor,
        section: "Pointer and focus",
        label: "How far focusing may scroll",
        help: "At 0% only fully visible windows take focus; the view never moves.",
        chain: INPUT,
        child: "focus-follows-mouse",
        kind: Kind::Prop {
            name: "max-scroll-amount",
            example: "0%",
        },
        default: "no limit",
        requires: Some(Setting::InputFocusFollowsMouse),
        ..BASE
    },
    InputWarpMouseToFocus => Spec {
        page: Page::Cursor,
        section: "Pointer and focus",
        label: "Move the pointer to newly focused windows",
        chain: INPUT,
        child: "warp-mouse-to-focus",
        ..BASE
    },
    InputWorkspaceAutoBackAndForth => Spec {
        page: Page::Workspaces,
        section: "Workspaces",
        label: "Switching to the current workspace goes back",
        help: "Focusing the workspace you are on returns to the previous one.",
        chain: INPUT,
        child: "workspace-auto-back-and-forth",
        ..BASE
    },
    InputDisablePowerKeyHandling => Spec {
        page: Page::Input,
        section: "Power button",
        label: "Leave the power button to the system",
        help: "niri then ignores it, and logind decides what it does.",
        chain: INPUT,
        child: "disable-power-key-handling",
        ..BASE
    },
    CursorXcursorTheme => Spec {
        page: Page::Cursor,
        section: "Cursor",
        label: "Theme",
        chain: CURSOR,
        child: "xcursor-theme",
        kind: Kind::CursorTheme,
        default: "default",
        ..BASE
    },
    CursorXcursorSize => Spec {
        page: Page::Cursor,
        section: "Cursor",
        label: "Size",
        chain: CURSOR,
        child: "xcursor-size",
        kind: Kind::Integer { min: 8, max: 256 },
        default: "24",
        ..BASE
    },
    CursorHideWhenTyping => Spec {
        page: Page::Cursor,
        section: "Cursor",
        label: "Hide while typing",
        chain: CURSOR,
        child: "hide-when-typing",
        ..BASE
    },
    CursorHideAfterInactiveMs => Spec {
        page: Page::Cursor,
        section: "Cursor",
        label: "Hide after inactivity",
        help: "Milliseconds without movement.",
        chain: CURSOR,
        child: "hide-after-inactive-ms",
        kind: Kind::Integer {
            min: 0,
            max: 3_600_000,
        },
        default: "never",
        ..BASE
    },
    // ---- Animations ----
    Animations => Spec {
        page: Page::Animations,
        section: "All animations",
        label: "Animations",
        chain: ANIMATIONS,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    AnimationsSlowdown => Spec {
        page: Page::Animations,
        section: "All animations",
        label: "Slowdown",
        help: "Multiplies every duration: 2 makes them twice as slow.",
        chain: ANIMATIONS,
        child: "slowdown",
        kind: Kind::Float {
            min: 0.0,
            max: 100.0,
        },
        default: "1",
        requires: Some(Setting::Animations),
        ..BASE
    },
    // ---- General ----
    PreferNoCsd => Spec {
        page: Page::Environment,
        section: "Windows",
        label: "Ask apps to drop their own title bars",
        help: "Apps that agree draw no title bars, and niri's ring and border then frame them rather than show through them.",
        child: "prefer-no-csd",
        ..BASE
    },
    GesturesHotCorners => Spec {
        page: Page::Gestures,
        section: "Hot corners",
        label: "Hot corners",
        help: "Push the pointer into a corner to open the overview.",
        chain: HOT_CORNERS,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    GesturesHotCornersTopLeft => Spec {
        page: Page::Gestures,
        section: "Hot corners",
        label: "Top left",
        chain: HOT_CORNERS,
        child: "top-left",
        requires: Some(Setting::GesturesHotCorners),
        note: Some(fallback_corner_note),
        ..BASE
    },
    GesturesHotCornersTopRight => Spec {
        page: Page::Gestures,
        section: "Hot corners",
        label: "Top right",
        chain: HOT_CORNERS,
        child: "top-right",
        requires: Some(Setting::GesturesHotCorners),
        ..BASE
    },
    GesturesHotCornersBottomLeft => Spec {
        page: Page::Gestures,
        section: "Hot corners",
        label: "Bottom left",
        chain: HOT_CORNERS,
        child: "bottom-left",
        requires: Some(Setting::GesturesHotCorners),
        ..BASE
    },
    GesturesHotCornersBottomRight => Spec {
        page: Page::Gestures,
        section: "Hot corners",
        label: "Bottom right",
        chain: HOT_CORNERS,
        child: "bottom-right",
        requires: Some(Setting::GesturesHotCorners),
        ..BASE
    },
    OverviewZoom => Spec {
        page: Page::Overview,
        section: "Overview",
        label: "Zoom",
        help: "How small workspaces get in the overview.",
        chain: OVERVIEW,
        child: "zoom",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "0.5",
        ..BASE
    },
    OverviewBackdropColor => Spec {
        page: Page::Overview,
        section: "Overview",
        label: "Backdrop",
        help: "Shown around the workspaces. The Displays page sets it per display.",
        chain: OVERVIEW,
        child: "backdrop-color",
        kind: Kind::Color,
        default: BACKDROP,
        ..BASE
    },
    OverviewWorkspaceShadow => Spec {
        page: Page::Overview,
        section: "Overview",
        label: "Workspace shadow",
        chain: WORKSPACE_SHADOW,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    RecentWindows => Spec {
        page: Page::Overview,
        section: "Window switcher",
        label: "Window switcher",
        help: "Alt+Tab and Mod+Tab show your recent windows.",
        chain: RECENT,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    RecentWindowsOpenDelayMs => Spec {
        page: Page::Overview,
        section: "Window switcher",
        label: "Delay before it shows",
        help: "Milliseconds. A quick Alt+Tab switches without showing it.",
        chain: RECENT,
        child: "open-delay-ms",
        kind: Kind::Integer { min: 0, max: 5000 },
        default: "150",
        requires: Some(Setting::RecentWindows),
        ..BASE
    },
    RecentWindowsDebounceMs => Spec {
        page: Page::Overview,
        section: "Window switcher",
        label: "Time to count as recent",
        help: "Milliseconds a window must keep focus to join the list.",
        chain: RECENT,
        child: "debounce-ms",
        kind: Kind::Integer {
            min: 0,
            max: 10_000,
        },
        default: "750",
        requires: Some(Setting::RecentWindows),
        ..BASE
    },
    RecentWindowsHighlightActiveColor => Spec {
        page: Page::Overview,
        section: "Window switcher",
        label: "Highlight color",
        chain: RECENT_HIGHLIGHT,
        child: "active-color",
        kind: Kind::Color,
        default: "#999999ff",
        requires: Some(Setting::RecentWindows),
        ..BASE
    },
    Blur => Spec {
        page: Page::Layout,
        section: "Blur",
        label: "Blur",
        help: "Behind windows and panels that ask for it, and those a window rule blurs.",
        chain: BLUR,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    BlurPasses => Spec {
        page: Page::Layout,
        section: "Blur",
        label: "Passes",
        help: "More passes blur more and cost more.",
        chain: BLUR,
        child: "passes",
        kind: Kind::Integer { min: 1, max: 8 },
        default: "3",
        requires: Some(Setting::Blur),
        ..BASE
    },
    BlurOffset => Spec {
        page: Page::Layout,
        section: "Blur",
        label: "Offset",
        help: "How far each pass reaches.",
        chain: BLUR,
        child: "offset",
        kind: Kind::Float {
            min: 0.0,
            max: 20.0,
        },
        default: "3",
        requires: Some(Setting::Blur),
        ..BASE
    },
    BlurNoise => Spec {
        page: Page::Layout,
        section: "Blur",
        label: "Noise",
        help: "A little grain hides banding.",
        chain: BLUR,
        child: "noise",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "0.02",
        requires: Some(Setting::Blur),
        ..BASE
    },
    BlurSaturation => Spec {
        page: Page::Layout,
        section: "Blur",
        label: "Saturation",
        chain: BLUR,
        child: "saturation",
        kind: Kind::Float { min: 0.0, max: 2.0 },
        default: "1.5",
        requires: Some(Setting::Blur),
        ..BASE
    },
    ScreenshotPath => Spec {
        page: Page::Screenshots,
        section: "Screenshots",
        label: "Save screenshots to",
        help: "Codes such as %Y-%m-%d become the date. niri creates the last folder if needed.",
        child: "screenshot-path",
        kind: Kind::Screenshot,
        default: "~/Pictures/Screenshots/Screenshot from %Y-%m-%d %H-%M-%S.png",
        ..BASE
    },
    HotkeyOverlaySkipAtStartup => Spec {
        page: Page::Overview,
        section: "Shortcut overview",
        label: "Skip the shortcut overview at startup",
        chain: HOTKEY_OVERLAY,
        child: "skip-at-startup",
        ..BASE
    },
    HotkeyOverlayHideNotBound => Spec {
        page: Page::Overview,
        section: "Shortcut overview",
        label: "Leave out actions that have no key",
        chain: HOTKEY_OVERLAY,
        child: "hide-not-bound",
        ..BASE
    },
    ClipboardDisablePrimary => Spec {
        page: Page::Environment,
        section: "Clipboard",
        label: "Turn off middle-click paste",
        help: "Disables the primary selection.",
        chain: CLIPBOARD,
        child: "disable-primary",
        ..BASE
    },
    XwaylandSatellite => Spec {
        page: Page::Environment,
        section: "X11 apps",
        label: "Run X11 apps",
        help: "Through xwayland-satellite, started when an X11 app needs it.",
        chain: XWAYLAND,
        child: "off",
        kind: Kind::Enabled,
        ..BASE
    },
    XwaylandSatellitePath => Spec {
        page: Page::Environment,
        section: "X11 apps",
        label: "Program",
        chain: XWAYLAND,
        child: "path",
        kind: Kind::File {
            example: "/usr/bin/xwayland-satellite",
        },
        default: "xwayland-satellite",
        requires: Some(Setting::XwaylandSatellite),
        ..BASE
    },
}

/// niri's backdrop color, for the overview and for each display.
const BACKDROP: &str = "#262626";

const TRANSFORMS: &[(&str, &str)] = &[
    ("normal", "Normal"),
    ("90", "Turned 90 degrees"),
    ("180", "Upside down"),
    ("270", "Turned 270 degrees"),
    ("flipped", "Mirrored"),
    ("flipped-90", "Mirrored, turned 90 degrees"),
    ("flipped-180", "Mirrored, upside down"),
    ("flipped-270", "Mirrored, turned 270 degrees"),
];

table! {
    /// A setting of one display, in its `output "<name>"` block. The
    /// Displays page fills in the chain for the display it shows.
    OutputSetting, OUTPUT_SETTINGS;
    Mode => Spec {
        page: Page::Displays,
        label: "Resolution and refresh rate",
        child: "mode",
        kind: Kind::Text {
            example: "1920x1080@60",
        },
        ..BASE
    },
    Scale => Spec {
        page: Page::Displays,
        label: "Scale",
        child: "scale",
        kind: Kind::Float {
            min: 0.25,
            max: 10.0,
        },
        ..BASE
    },
    Transform => Spec {
        page: Page::Displays,
        label: "Rotation",
        child: "transform",
        kind: Kind::Choice(TRANSFORMS),
        default: "normal",
        ..BASE
    },
    VariableRefreshRate => Spec {
        page: Page::Displays,
        label: "Variable refresh rate",
        child: "variable-refresh-rate",
        kind: Kind::Switch,
        ..BASE
    },
    BackdropColor => Spec {
        page: Page::Displays,
        label: "Backdrop",
        help: "Shown around workspaces in the overview.",
        child: "backdrop-color",
        kind: Kind::Color,
        default: BACKDROP,
        ..BASE
    },
    Off => Spec {
        page: Page::Displays,
        label: "Turn this display off",
        child: "off",
        kind: Kind::Switch,
        ..BASE
    },
}

/// Settings a display or a workspace can set for itself, in its own
/// `output` or `workspace` block (niri 25.11).
struct Scope {
    /// The global block the settings live in: `layout`.
    global: &'static str,
    /// Only these of its children; all when empty.
    children: &'static [&'static str],
    /// The blocks that can set them for themselves.
    within: &'static [&'static str],
    /// The global block appears again inside (`output { layout { gaps } }`),
    /// or the setting stands in it directly (`output { backdrop-color }`).
    repeats_block: bool,
}

const SCOPES: [Scope; 2] = [
    Scope {
        global: "layout",
        children: &[],
        within: &["output", "workspace"],
        repeats_block: true,
    },
    Scope {
        global: "overview",
        children: &["backdrop-color"],
        within: &["output"],
        repeats_block: false,
    },
];

/// The displays and workspaces that set `child` of `chain` for
/// themselves, in blocks of their own ([`SCOPES`]).
pub fn overrides(store: &Store, chain: &[Block], child: &str) -> Vec<crate::store::Override> {
    let Some((first, rest)) = chain.split_first() else {
        return Vec::new();
    };
    let Some(scope) = SCOPES.iter().find(|scope| {
        scope.global == first.name && (scope.children.is_empty() || scope.children.contains(&child))
    }) else {
        return Vec::new();
    };
    let inside: Vec<Block> = if scope.repeats_block {
        std::iter::once(Block::named(scope.global))
            .chain(rest.iter().cloned())
            .collect()
    } else {
        rest.to_vec()
    };
    store.overrides(scope.within, &inside, child)
}

/// Where one display's settings live: its `output "<name>"` block.
pub const fn output_chain(output: &str) -> [Block<'_>; 1] {
    [Block::with_arg("output", output)]
}

/// Where a control keeps the text typed into it, and what is wrong with
/// that text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DraftKey {
    Setting(Setting),
    /// One display's setting, by the display's name.
    Output(String, OutputSetting),
    /// One display's position, along one axis.
    Position(String, Coordinate),
}

/// One axis of a display's position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Coordinate {
    X,
    Y,
}

impl Spec {
    /// Where the setting lives, as it reads in the file: `layout › gaps`.
    pub fn path(&self) -> String {
        let mut parts: Vec<&str> = self.chain.iter().map(|block| block.name).collect();
        match self.kind {
            Kind::OnOff => parts.push("on / off"),
            Kind::Prop { name, .. } => {
                parts.push(self.child);
                parts.push(name);
            }
            Kind::Switch
            | Kind::Enabled
            | Kind::Choice(_)
            | Kind::Bool
            | Kind::Integer { .. }
            | Kind::Float { .. }
            | Kind::Color
            | Kind::Gradient
            | Kind::Text { .. }
            | Kind::Sizes { .. }
            | Kind::Layouts
            | Kind::XkbOptions
            | Kind::CursorTheme
            | Kind::Output
            | Kind::File { .. }
            | Kind::Screenshot => parts.push(self.child),
        }
        parts.join(" › ")
    }

    pub fn read(&self, store: &Store) -> Reading {
        read(store, self.chain, self.child, self.kind)
    }

    pub fn write(&self, store: &mut Store, reading: &Reading) -> Result<(), StoreError> {
        write(store, self.chain, self.child, self.kind, reading)
    }

    /// Whether niri uses this setting with the config as it is: an on/off
    /// section that is off makes the rows that depend on it moot.
    pub fn is_on(&self, store: &Store) -> bool {
        match self.read(store) {
            Reading::Switch(on) => on,
            Reading::OnOff(OnOff::On) => true,
            Reading::OnOff(OnOff::Off) => false,
            Reading::OnOff(OnOff::Default) => self.default == "on",
            Reading::Value(_) => true,
        }
    }

    /// A value that is valid for this setting, for the niri-validate sweep.
    pub fn sample(&self) -> Reading {
        match self.kind {
            Kind::Switch | Kind::Enabled => Reading::Switch(true),
            Kind::OnOff => Reading::OnOff(OnOff::On),
            Kind::Choice(options) => Reading::Value(options.last().map(|(v, _)| (*v).to_string())),
            Kind::Bool => Reading::Value(Some("true".to_string())),
            Kind::Integer { min, max } => Reading::Value(Some(((min + max) / 2).to_string())),
            Kind::Float { min, max } => {
                Reading::Value(Some(format!("{:.2}", f64::midpoint(min, max))))
            }
            Kind::Color => Reading::Value(Some("#336699".to_string())),
            Kind::Gradient => Reading::Value(Some(self.default.to_string())),
            Kind::Text { example } | Kind::Prop { example, .. } | Kind::File { example } => {
                Reading::Value(Some(example.to_string()))
            }
            Kind::Sizes { many: true, .. } => {
                Reading::Value(Some("proportion 0.25; fixed 800".to_string()))
            }
            Kind::Sizes { many: false, .. } => Reading::Value(Some("proportion 0.75".to_string())),
            Kind::Layouts => Reading::Value(Some("us,de".to_string())),
            Kind::XkbOptions => Reading::Value(Some("ctrl:nocaps".to_string())),
            Kind::CursorTheme => Reading::Value(Some("Adwaita".to_string())),
            Kind::Output => Reading::Value(Some("DP-1".to_string())),
            Kind::Screenshot => Reading::Value(Some(self.default.to_string())),
        }
    }
}

/// A setting's value in `store`, by where it lives and what kind it is.
pub fn read(store: &Store, chain: &[Block], child: &str, kind: Kind) -> Reading {
    match kind {
        Kind::Switch => Reading::Switch(store.is_set(chain, child)),
        Kind::Enabled => Reading::Switch(!store.is_set(chain, child)),
        Kind::OnOff => Reading::OnOff(
            match (store.is_set(chain, "on"), store.is_set(chain, "off")) {
                (true, false) => OnOff::On,
                (false, true) => OnOff::Off,
                // both set across files: writing either choice normalises the block
                (true, true) | (false, false) => OnOff::Default,
            },
        ),
        Kind::Prop { name, .. } => Reading::Value(
            store
                .setting_node(chain, child)
                .and_then(|node| node.props.iter().find(|prop| prop.name == name))
                .map(|prop| prop.value.kind.as_str().to_string()),
        ),
        Kind::Gradient => Reading::Value(store.setting_node(chain, child).map(|node| {
            node.props
                .iter()
                .map(|prop| format!("{}={}", prop.name, value_text(&prop.value.kind)))
                .collect::<Vec<_>>()
                .join(" ")
        })),
        Kind::Sizes { .. } => Reading::Value(
            store
                .sizes(chain, child)
                .map(|entries| join_sizes(&entries)),
        ),
        Kind::Choice(_)
        | Kind::Bool
        | Kind::Integer { .. }
        | Kind::Float { .. }
        | Kind::Color
        | Kind::Text { .. }
        | Kind::Layouts
        | Kind::XkbOptions
        | Kind::CursorTheme
        | Kind::Output
        | Kind::File { .. }
        | Kind::Screenshot => Reading::Value(store.setting(chain, child)),
    }
}

/// A KDL value as it would be written again.
fn value_text(kind: &ValueKind) -> String {
    match kind {
        ValueKind::Quoted(text) => quoted(text),
        ValueKind::Bare(text) | ValueKind::Number(text) => text.clone(),
    }
}

pub fn write(
    store: &mut Store,
    chain: &[Block],
    child: &str,
    kind: Kind,
    reading: &Reading,
) -> Result<(), StoreError> {
    match reading {
        Reading::Switch(on) if kind == Kind::Enabled => store.set_flag_setting(chain, child, !on),
        Reading::Switch(on) => store.set_flag_setting(chain, child, *on),
        Reading::OnOff(state) => {
            let (on, off) = match state {
                OnOff::Default => (false, false),
                OnOff::On => (true, false),
                OnOff::Off => (false, true),
            };
            store.set_flag_setting(chain, "off", off)?;
            store.set_flag_setting(chain, "on", on)
        }
        Reading::Value(None) => match kind {
            // the flag stays; only its property goes
            Kind::Prop { .. } if store.is_set(chain, child) => store.set_setting(chain, child, ""),
            Kind::Prop { .. } => Ok(()),
            Kind::Sizes { .. } => store.set_sizes(chain, child, None),
            Kind::Switch
            | Kind::Enabled
            | Kind::OnOff
            | Kind::Choice(_)
            | Kind::Bool
            | Kind::Integer { .. }
            | Kind::Float { .. }
            | Kind::Color
            | Kind::Gradient
            | Kind::Text { .. }
            | Kind::Layouts
            | Kind::XkbOptions
            | Kind::CursorTheme
            | Kind::Output
            | Kind::File { .. }
            | Kind::Screenshot => store.unset_setting(chain, child),
        },
        Reading::Value(Some(value)) => match kind {
            Kind::Integer { .. } | Kind::Float { .. } | Kind::Bool | Kind::Gradient => {
                store.set_setting(chain, child, value)
            }
            Kind::Prop { name, .. } => {
                store.set_setting(chain, child, &format!("{name}={}", quoted(value)))
            }
            Kind::Sizes { .. } => store.set_sizes(chain, child, Some(&sizes(value))),
            Kind::Screenshot if value == NO_FILE => store.set_setting(chain, child, NO_FILE),
            Kind::Choice(_)
            | Kind::Color
            | Kind::Text { .. }
            | Kind::Layouts
            | Kind::XkbOptions
            | Kind::CursorTheme
            | Kind::Output
            | Kind::File { .. }
            | Kind::Screenshot
            | Kind::Switch
            | Kind::Enabled
            | Kind::OnOff => store.set_setting(chain, child, &quoted(value)),
        },
    }
}

/// `proportion 0.5; fixed 800` as `(name, value)` pairs.
pub fn sizes(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (name, value) = entry.split_once(' ').unwrap_or((entry, ""));
            (name.trim().to_string(), value.trim().to_string())
        })
        .collect()
}

pub fn join_sizes(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(name, value)| format!("{name} {value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A size as people say it: `1/3`, `50%`, `800 px`.
pub fn size_label(name: &str, value: &str) -> String {
    match (name, value.parse::<f64>()) {
        ("proportion", Ok(p)) => {
            for (fraction, label) in [
                (1.0 / 4.0, "1/4"),
                (1.0 / 3.0, "1/3"),
                (0.5, "1/2"),
                (2.0 / 3.0, "2/3"),
                (3.0 / 4.0, "3/4"),
                (1.0, "Full"),
            ] {
                if (p - fraction).abs() < 0.001 {
                    return label.to_string();
                }
            }
            format!("{}%", (p * 100.0).round())
        }
        ("fixed", Ok(px)) => format!("{px} px"),
        _ => format!("{name} {value}"),
    }
}

/// Check typed text: `Ok(None)` for empty (unset), the value to write, or a
/// message saying what is accepted.
pub fn parse(kind: Kind, input: &str) -> Result<Option<String>, String> {
    let input = input.trim();
    if let Kind::Sizes { many, .. } = kind {
        return parse_sizes(input, many).map(Some);
    }
    if input.is_empty() {
        return Ok(None);
    }
    match kind {
        Kind::Integer { min, max } => input
            .parse::<i64>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .map(|n| Some(n.to_string()))
            .ok_or_else(|| format!("Enter a whole number from {min} to {max}.")),
        Kind::Float { min, max } => input
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && (min..=max).contains(n))
            // written as typed: these settings take `24` as well as `24.5`
            // (only spring props insist on a decimal point; the core writes those)
            .map(|_| Some(input.to_string()))
            .ok_or_else(|| format!("Enter a number from {min} to {max}.")),
        Kind::Color => is_color(input)
            .then(|| Some(input.to_string()))
            .ok_or_else(|| {
                "niri does not read this as a color. Try #7fc8ff, rgb(127, 200, 255) or a CSS name."
                    .to_string()
            }),
        Kind::Gradient => Gradient::parse(input)
            .filter(Gradient::is_valid)
            .map(|_| Some(input.to_string()))
            .ok_or_else(|| "Both ends need a color niri can read.".to_string()),
        Kind::Bool if matches!(input, "true" | "false") => Ok(Some(input.to_string())),
        Kind::Bool => Err("Choose on or off.".to_string()),
        Kind::Prop { .. } => input
            .strip_suffix('%')
            .and_then(|number| number.trim().parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|_| Some(input.to_string()))
            .ok_or_else(|| "Enter a percentage such as 0% or 50%.".to_string()),
        Kind::Text { .. }
        | Kind::Choice(_)
        | Kind::Switch
        | Kind::Enabled
        | Kind::OnOff
        | Kind::Sizes { .. }
        | Kind::Layouts
        | Kind::XkbOptions
        | Kind::CursorTheme
        | Kind::Output
        | Kind::File { .. }
        | Kind::Screenshot => Ok(Some(input.to_string())),
    }
}

fn parse_sizes(input: &str, many: bool) -> Result<String, String> {
    let entries = sizes(input);
    if many && entries.is_empty() {
        return Err("Keep at least one size.".to_string());
    }
    for (name, value) in &entries {
        let ok = match name.as_str() {
            "proportion" => value
                .parse::<f64>()
                .is_ok_and(|p| p.is_finite() && p > 0.0 && p <= 1.0),
            "fixed" => value.parse::<u32>().is_ok_and(|px| px > 0),
            _ => false,
        };
        if !ok {
            return Err(
                "A part of the screen is a number above 0 and at most 1, a fixed size whole pixels."
                    .to_string(),
            );
        }
    }
    // niri reads a proportion as a float only: `1.0`, never `1`
    let entries: Vec<(String, String)> = entries
        .into_iter()
        .map(|(name, value)| {
            if name == "proportion" && !value.contains('.') {
                (name, format!("{value}.0"))
            } else {
                (name, value)
            }
        })
        .collect();
    Ok(join_sizes(&entries))
}

/// Text typed into setting fields, and what is wrong with it.
#[derive(Debug, Clone, Default)]
pub struct Drafts {
    typed: HashMap<DraftKey, String>,
    errors: HashMap<DraftKey, String>,
}

impl Drafts {
    /// What was typed under `key`, while it is not the setting's value.
    pub fn typed(&self, key: &DraftKey) -> Option<&String> {
        self.typed.get(key)
    }

    /// Why what was typed under `key` cannot be written.
    pub fn error(&self, key: &DraftKey) -> Option<&String> {
        self.errors.get(key)
    }

    /// Keep `text` as typed under `key`, and why it cannot be written yet,
    /// or that it can (`None`).
    pub fn keep(&mut self, key: &DraftKey, text: String, error: Option<String>) {
        self.typed.insert(key.clone(), text);
        match error {
            Some(error) => self.errors.insert(key.clone(), error),
            None => self.errors.remove(key),
        };
    }

    /// Forget what was typed under `key`: the value comes from elsewhere.
    pub fn forget(&mut self, key: &DraftKey) {
        self.typed.remove(key);
        self.errors.remove(key);
    }

    /// Forget everything typed: the config's text was replaced underneath.
    pub fn clear(&mut self) {
        self.typed.clear();
        self.errors.clear();
    }

    /// Turn a control edit into the reading to write. Text is kept under
    /// `key`, and text that does not read yet gives `None`, nothing to
    /// write, with its error kept.
    pub fn reading(&mut self, key: &DraftKey, kind: Kind, edit: Edit) -> Option<Reading> {
        match edit {
            Edit::Switch(on) => Some(Reading::Switch(on)),
            Edit::OnOff(on_off) => Some(Reading::OnOff(on_off)),
            Edit::Choice(choice) => {
                self.forget(key);
                Some(Reading::Value(choice))
            }
            Edit::Draft(input) => {
                let parsed = parse(kind, &input);
                match parsed {
                    Ok(value) => {
                        self.keep(key, input, None);
                        Some(Reading::Value(value))
                    }
                    Err(message) => {
                        self.keep(key, input, Some(message));
                        None
                    }
                }
            }
            Edit::Unset => {
                self.forget(key);
                Some(unset(kind))
            }
        }
    }
}

/// What a setting reads when the config leaves it out.
pub const fn unset(kind: Kind) -> Reading {
    match kind {
        Kind::Switch => Reading::Switch(false),
        Kind::Enabled => Reading::Switch(true),
        Kind::OnOff => Reading::OnOff(OnOff::Default),
        Kind::Choice(_)
        | Kind::Bool
        | Kind::Integer { .. }
        | Kind::Float { .. }
        | Kind::Color
        | Kind::Gradient
        | Kind::Text { .. }
        | Kind::Prop { .. }
        | Kind::Sizes { .. }
        | Kind::Layouts
        | Kind::XkbOptions
        | Kind::CursorTheme
        | Kind::Output
        | Kind::File { .. }
        | Kind::Screenshot => Reading::Value(None),
    }
}

/// Apply one control edit of `setting` to `store`, keeping text typed
/// into its field in `drafts`.
pub fn edit(
    store: &mut Store,
    drafts: &mut Drafts,
    setting: Setting,
    edit: Edit,
) -> Result<(), String> {
    let spec = setting.spec();
    edit_in(
        store,
        drafts,
        spec,
        spec.chain,
        &DraftKey::Setting(setting),
        edit,
    )
}

/// [`edit`] for `spec` where it lives at `chain` (one display's block),
/// with its drafts kept under `key`.
pub fn edit_in(
    store: &mut Store,
    drafts: &mut Drafts,
    spec: &Spec,
    chain: &[Block],
    key: &DraftKey,
    edit: Edit,
) -> Result<(), String> {
    let Some(reading) = drafts.reading(key, spec.kind, edit) else {
        return Ok(());
    };
    write(store, chain, spec.child, spec.kind, &reading).map_err(|e| e.to_string())
}

/// Whether a display other than `output` is connected and on. Turning off
/// the last one that is would leave nothing on screen to turn it back on
/// with.
pub fn others_on(store: &Store, connected: &[OutputInfo], output: &str) -> bool {
    connected
        .iter()
        .filter(|other| other.name != output)
        .any(|other| other.rect.is_some() && !store.is_set(&output_chain(&other.name), "off"))
}

/// [`edit`] for one display's `setting`, in its `output` block. The last
/// display that is on stays on.
pub fn edit_output(
    store: &mut Store,
    drafts: &mut Drafts,
    connected: &[OutputInfo],
    output: &str,
    setting: OutputSetting,
    edit: Edit,
) -> Result<(), String> {
    if setting == OutputSetting::Off
        && matches!(edit, Edit::Switch(true))
        && !others_on(store, connected, output)
    {
        return Err(format!(
            "{output} is the only display that is on, so it stays on."
        ));
    }
    edit_in(
        store,
        drafts,
        setting.spec(),
        &output_chain(output),
        &DraftKey::Output(output.to_string(), setting),
        edit,
    )
}

/// A line under a row's label: a fact about the setting, or a warning.
#[derive(Debug, Clone)]
pub struct Note {
    pub text: String,
    pub warn: bool,
}

impl Note {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            warn: false,
        }
    }

    pub fn warn(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            warn: true,
        }
    }
}

/// The settings that mark a window asking for attention, in its urgent
/// color.
const URGENCY_MARKS: [Setting; 2] = [Setting::LayoutFocusRing, Setting::LayoutBorder];

/// With neither the focus ring nor the border, and no rule of its own, a
/// window that asks for attention looks like any other.
fn urgency_note(store: &Store) -> Option<Note> {
    let marked = URGENCY_MARKS
        .iter()
        .any(|setting| setting.spec().is_on(store))
        || {
            let (name, yes) = crate::rules::When::AsksForAttention.prop();
            store.has_rule_for_state(&name, yes)
        };
    (!marked).then(|| {
        Note::warn(
            "With the focus ring and the border off, a window asking for attention looks like any other. A window rule for windows that ask for attention can mark them.",
        )
    })
}

/// The hot corners; with none chosen, niri uses the first.
const CORNERS: [Setting; 4] = [
    Setting::GesturesHotCornersTopLeft,
    Setting::GesturesHotCornersTopRight,
    Setting::GesturesHotCornersBottomLeft,
    Setting::GesturesHotCornersBottomRight,
];

fn fallback_corner_note(store: &Store) -> Option<Note> {
    CORNERS
        .iter()
        .all(|corner| !corner.spec().is_on(store))
        .then(|| Note::info("niri uses this corner now, since no corner is chosen."))
}

/// What a row should say beyond its label: settings it depends on, values
/// that override it, and where the value lives when that matters.
/// `chain` is where the setting lives: its own, or one display's block.
/// `off_parent` is the setting it depends on, when that is off.
pub fn notes(
    store: &Store,
    system: &SystemData,
    setting: &Spec,
    chain: &[Block],
    off_parent: Option<&Spec>,
) -> Vec<Note> {
    let mut notes = Vec::new();
    if let Some(parent) = off_parent {
        notes.push(Note::info(format!(
            "Only used while {} is on.",
            parent.label
        )));
    }
    if !setting.overridden_by.is_empty() && store.is_set(chain, setting.overridden_by) {
        let instead = REGISTRY
            .iter()
            .find(|other| (other.chain, other.child) == (setting.chain, setting.overridden_by))
            .map_or(setting.overridden_by, |other| other.label);
        notes.push(Note::info(format!("{instead} is used instead.")));
    }
    notes.extend(setting.note.and_then(|note| note(store)));
    let overrides: Vec<String> = overrides(store, chain, setting.child)
        .iter()
        .map(ToString::to_string)
        .collect();
    if !overrides.is_empty() {
        notes.push(Note::info(format!(
            "{} its own value, used there instead.",
            who(&overrides, "sets", "set")
        )));
    }
    if let Some(file) = store.setting_file(chain, setting.child)
        && let Some(owner) = managed_by(store, file)
    {
        notes.push(Note::warn(format!(
            "This value is in {}, which {owner} rewrites, so a change here may not last.",
            store.file_label(file)
        )));
    }
    if setting.kind == Kind::CursorTheme
        && let Reading::Value(Some(theme_name)) = read(store, chain, setting.child, setting.kind)
        && !system.cursor_themes.is_empty()
        && !system.cursor_themes.contains(&theme_name)
    {
        notes.push(Note::warn(format!(
            "{theme_name} is not installed, so niri falls back to the default cursors."
        )));
    }
    notes
}

/// A desktop shell that writes niri config files of its own, in a folder
/// beside config.kdl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    DankMaterialShell,
    Noctalia,
}

impl Shell {
    const ALL: [Self; 2] = [Self::DankMaterialShell, Self::Noctalia];

    /// The folder beside config.kdl it writes into.
    const fn folder(self) -> &'static str {
        match self {
            Self::DankMaterialShell => "dms",
            Self::Noctalia => "noctalia",
        }
    }
}

/// A program that writes a config file on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    Shell(Shell),
    /// A file that says it was generated.
    Generator,
}

impl std::fmt::Display for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Shell(Shell::DankMaterialShell) => "DankMaterialShell",
            Self::Shell(Shell::Noctalia) => "Noctalia",
            Self::Generator => "the program that generated it",
        })
    }
}

/// Which program writes `file` on its own, judging by where it lives or
/// what its first lines say.
pub fn managed_by(store: &Store, file: &Path) -> Option<Writer> {
    if file == store.path() {
        return None;
    }
    let relative = file.strip_prefix(store.config_dir()).unwrap_or(file);
    let folder = relative.components().next()?.as_os_str();
    if let Some(shell) = Shell::ALL
        .into_iter()
        .find(|shell| folder == shell.folder())
    {
        return Some(Writer::Shell(shell));
    }
    // the text the store loaded: no disk read while the page draws
    let head: String = store
        .text_of(file)
        .unwrap_or_default()
        .lines()
        .take(5)
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    (head.contains("generated") && (head.contains("do not edit") || head.contains("overwritten")))
        .then_some(Writer::Generator)
}

/// "Default" plus what that is, when known, read as part of the sentence:
/// "Default (on)", but "Default (Super)" for a key's name.
pub fn default_label(default: &str) -> String {
    const NAMES: [&str; 6] = ["Super", "Alt", "Ctrl", "Shift", "Mod", "Hyper"];
    if default.is_empty() || default.eq_ignore_ascii_case(NIRI_DEFAULT) {
        return NIRI_DEFAULT.to_string();
    }
    let mut letters = default.chars();
    let shown = match (letters.next(), letters.next()) {
        (Some(first), Some(second))
            if first.is_uppercase() && second.is_lowercase() && !NAMES.contains(&default) =>
        {
            first
                .to_lowercase()
                .chain(default.chars().skip(1))
                .collect()
        }
        _ => default.to_string(),
    };
    format!("{NIRI_DEFAULT} ({shown})")
}

/// A field's placeholder for niri's default: a value as it would be typed
/// (`Default 16`, `Default #7fc8ff`), words in parentheses like the lists
/// (`Default (no limit)`).
pub fn default_placeholder(default: &str) -> String {
    let typed = default
        .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '#' | '-' | '.' | '~' | '/'));
    if typed {
        format!("{NIRI_DEFAULT} {default}")
    } else {
        default_label(default)
    }
}

/// Where the setting is written, as `config.kdl:38`; empty when it is not.
pub fn line_link(store: &Store, chain: &[Block], child: &str) -> String {
    store
        .setting_line(chain, child)
        .map_or_else(String::new, |(file, line)| {
            format!("{}:{line}", store.file_label(file))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_and_join() {
        assert_eq!(
            parse_sizes("proportion 0.5;  fixed 800", true),
            Ok("proportion 0.5; fixed 800".to_string())
        );
        assert!(parse_sizes("", true).is_err());
        assert_eq!(parse_sizes("", false), Ok(String::new()));
        assert!(parse_sizes("proportion 1.5", true).is_err());
        // niri rejects `proportion 1`
        assert_eq!(
            parse_sizes("proportion 1", true),
            Ok("proportion 1.0".to_string())
        );
        assert!(parse_sizes("fixed -3", true).is_err());
        assert_eq!(size_label("proportion", "0.33333"), "1/3");
        assert_eq!(size_label("proportion", "0.4"), "40%");
        assert_eq!(size_label("fixed", "800"), "800 px");
    }

    #[test]
    fn every_dependency_is_a_switch_on_the_same_page() {
        for &setting in Setting::ALL {
            let spec = setting.spec();
            let Some(parent) = spec.requires else {
                continue;
            };
            assert_eq!(parent.spec().page, spec.page, "{setting:?}");
            assert!(
                matches!(
                    parent.spec().kind,
                    Kind::Switch | Kind::Enabled | Kind::OnOff
                ),
                "{setting:?} depends on something that is not a switch",
            );
        }
    }
}
