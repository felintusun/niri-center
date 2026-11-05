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
use crate::words::{Words, who};

/// The headings settings are grouped under on their pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Touchpad,
    Tabs,
    Keyboard,
    Shadow,
    Mouse,
    FocusRing,
    Columns,
    Border,
    Spacing,
    HotCorners,
    Blur,
    WindowSwitcher,
    Cursor,
    PointerAndFocus,
    Overview,
    X11Apps,
    Workspaces,
    TabletAndTouchscreen,
    ShortcutOverview,
    InsertHint,
    AllAnimations,
    Windows,
    Screenshots,
    PowerButton,
    Clipboard,
    Background,
}

impl Section {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Touchpad => "Touchpad",
            Self::Tabs => "Tabs",
            Self::Keyboard => "Keyboard",
            Self::Shadow => "Shadow",
            Self::Mouse => "Mouse",
            Self::FocusRing => "Focus ring",
            Self::Columns => "Columns",
            Self::Border => "Border",
            Self::Spacing => "Spacing",
            Self::HotCorners => "Hot corners",
            Self::Blur => "Blur",
            Self::WindowSwitcher => "Window switcher",
            Self::Cursor => "Cursor",
            Self::PointerAndFocus => "Pointer and focus",
            Self::Overview => "Overview",
            Self::X11Apps => "X11 apps",
            Self::Workspaces => "Workspaces",
            Self::TabletAndTouchscreen => "Tablet and touchscreen",
            Self::ShortcutOverview => "Shortcut overview",
            Self::InsertHint => "Insert hint",
            Self::AllAnimations => "All animations",
            Self::Windows => "Windows",
            Self::Screenshots => "Screenshots",
            Self::PowerButton => "Power button",
            Self::Clipboard => "Clipboard",
            Self::Background => "Background",
        }
    }
}

impl std::fmt::Display for Section {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(self.title())
    }
}

/// One editable setting.
pub struct Spec {
    pub page: Page,
    /// The heading it is grouped under on its page; none for a page's
    /// first, unheaded settings.
    pub section: Option<Section>,
    pub label: Words,
    /// One plain sentence, or empty.
    pub help: Words,
    pub chain: &'static [Block<'static>],
    /// The node holding the value (unused for [`Kind::OnOff`]).
    pub child: Words,
    pub kind: Kind,
    /// What niri does when the config leaves this unset, in the words the
    /// control shows (`16`, `never`). Empty when niri leaves it to the device.
    /// For [`Kind::Gradient`], the gradient a new one starts from.
    pub default: Words,
    /// A setting that must be on for this one to matter.
    pub requires: Option<Setting>,
    /// A node beside this one that niri uses instead when it is set: a
    /// color's gradient.
    pub overridden_by: Words,
    /// What else the row should say, worked out from the config: facts
    /// that tie this setting to others.
    pub note: Option<fn(&Store) -> Option<Note>>,
}

/// Fields most rows leave alone.
const BASE: Spec = Spec {
    page: Page::Layout,
    section: None,
    label: Words(""),
    help: Words(""),
    chain: TOP,
    child: Words(""),
    kind: Kind::Switch,
    default: Words(""),
    requires: None,
    overridden_by: Words(""),
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
    Choice(&'static [(Words, Words)]),
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
        example: Words,
    },
    /// A property on a flag node: `focus-follows-mouse max-scroll-amount="0%"`.
    Prop {
        name: Words,
        example: Words,
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
        example: Words,
    },
    /// `screenshot-path`: a path with strftime codes, or `null`.
    Screenshot,
}

impl Kind {
    /// A stored value in the words its control shows: a choice's label, On
    /// or Off. `None` where the value is its own word.
    pub fn word(self, value: &str) -> Option<&'static str> {
        match self {
            Self::Choice(choices) => choices
                .iter()
                .find(|(code, _)| code.as_str() == value)
                .map(|(_, label)| label.as_str()),
            Self::Bool => match value {
                "true" => Some("On"),
                "false" => Some("Off"),
                _ => None,
            },
            // only those two have words of their own
            Self::Switch
            | Self::Enabled
            | Self::OnOff
            | Self::Integer { .. }
            | Self::Float { .. }
            | Self::Color
            | Self::Gradient
            | Self::Text { .. }
            | Self::Prop { .. }
            | Self::Sizes { .. }
            | Self::Layouts
            | Self::XkbOptions
            | Self::CursorTheme
            | Self::Output
            | Self::File { .. }
            | Self::Screenshot => None,
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

const TOP: &[Block] = &[];
const LAYOUT: &[Block] = &[Block::named("layout")];
const STRUTS: &[Block] = &[Block::named("layout"), Block::named("struts")];
const FOCUS_RING: &[Block] = &[Block::named("layout"), Block::named("focus-ring")];
const BORDER: &[Block] = &[Block::named("layout"), Block::named("border")];
const SHADOW: &[Block] = &[Block::named("layout"), Block::named("shadow")];
const TABS: &[Block] = &[Block::named("layout"), Block::named("tab-indicator")];
const INSERT_HINT: &[Block] = &[Block::named("layout"), Block::named("insert-hint")];
const INPUT: &[Block] = &[Block::named("input")];
const KEYBOARD: &[Block] = &[Block::named("input"), Block::named("keyboard")];
const XKB: &[Block] = &[
    Block::named("input"),
    Block::named("keyboard"),
    Block::named("xkb"),
];
const TOUCHPAD: &[Block] = &[Block::named("input"), Block::named("touchpad")];
const MOUSE: &[Block] = &[Block::named("input"), Block::named("mouse")];
const TABLET: &[Block] = &[Block::named("input"), Block::named("tablet")];
const TOUCH: &[Block] = &[Block::named("input"), Block::named("touch")];
const CURSOR: &[Block] = &[Block::named("cursor")];
const ANIMATIONS: &[Block] = &[Block::named("animations")];
const HOT_CORNERS: &[Block] = &[Block::named("gestures"), Block::named("hot-corners")];
const OVERVIEW: &[Block] = &[Block::named("overview")];
const WORKSPACE_SHADOW: &[Block] = &[Block::named("overview"), Block::named("workspace-shadow")];
const RECENT: &[Block] = &[Block::named("recent-windows")];
const RECENT_HIGHLIGHT: &[Block] = &[Block::named("recent-windows"), Block::named("highlight")];
const BLUR: &[Block] = &[Block::named("blur")];
const HOTKEY_OVERLAY: &[Block] = &[Block::named("hotkey-overlay")];
const CLIPBOARD: &[Block] = &[Block::named("clipboard")];
const XWAYLAND: &[Block] = &[Block::named("xwayland-satellite")];

const ACCEL_PROFILES: &[(Words, Words)] = &[
    (Words("adaptive"), Words("Adaptive")),
    (Words("flat"), Words("Flat")),
];
const SCROLL_METHODS: &[(Words, Words)] = &[
    (Words("two-finger"), Words("Two fingers")),
    (Words("edge"), Words("Along the edge")),
    (Words("on-button-down"), Words("Button held")),
    (Words("no-scroll"), Words("Off")),
];
const SIDES: &[(Words, Words)] = &[
    (Words("left"), Words("Left")),
    (Words("right"), Words("Right")),
    (Words("top"), Words("Top")),
    (Words("bottom"), Words("Bottom")),
];
const THIRDS: &str = "proportion 0.33333; proportion 0.5; proportion 0.66667";

// Order within a page is the order on screen.
table! {
    /// A setting the app shows a row for.
    Setting, REGISTRY;
    // ---- Layout ----
    LayoutPresetColumnWidths => Spec {
        page: Page::Layout,
        section: Some(Section::Columns),
        label: Words("Widths to step through"),
        help: Words("Switch preset column width (Mod+R) cycles through these."),
        chain: LAYOUT,
        child: Words("preset-column-widths"),
        kind: Kind::Sizes {
            axis: Axis::Width,
            many: true,
        },
        default: Words(THIRDS),
        ..BASE
    },
    LayoutDefaultColumnWidth => Spec {
        page: Page::Layout,
        section: Some(Section::Columns),
        label: Words("New columns open at"),
        help: Words("Window rules can set their own width."),
        chain: LAYOUT,
        child: Words("default-column-width"),
        kind: Kind::Sizes {
            axis: Axis::Width,
            many: false,
        },
        default: Words("proportion 0.5"),
        ..BASE
    },
    LayoutPresetWindowHeights => Spec {
        page: Page::Layout,
        section: Some(Section::Columns),
        label: Words("Heights to step through"),
        help: Words("Switch preset window height (Mod+Shift+R) cycles through these."),
        chain: LAYOUT,
        child: Words("preset-window-heights"),
        kind: Kind::Sizes {
            axis: Axis::Height,
            many: true,
        },
        default: Words(THIRDS),
        ..BASE
    },
    LayoutCenterFocusedColumn => Spec {
        page: Page::Layout,
        section: Some(Section::Columns),
        label: Words("Center the focused column"),
        chain: LAYOUT,
        child: Words("center-focused-column"),
        kind: Kind::Choice(&[
            (Words("never"), Words("Never")),
            (Words("always"), Words("Always")),
            (Words("on-overflow"), Words("When it does not fit")),
        ]),
        default: Words("never"),
        ..BASE
    },
    LayoutAlwaysCenterSingleColumn => Spec {
        page: Page::Layout,
        section: Some(Section::Columns),
        label: Words("Center a lone column"),
        chain: LAYOUT,
        child: Words("always-center-single-column"),
        ..BASE
    },
    LayoutEmptyWorkspaceAboveFirst => Spec {
        page: Page::Workspaces,
        section: Some(Section::Workspaces),
        label: Words("Keep an empty workspace above the first"),
        chain: LAYOUT,
        child: Words("empty-workspace-above-first"),
        ..BASE
    },
    LayoutDefaultColumnDisplay => Spec {
        page: Page::Layout,
        section: Some(Section::Columns),
        label: Words("New columns show their windows"),
        chain: LAYOUT,
        child: Words("default-column-display"),
        kind: Kind::Choice(&[(Words("normal"), Words("Stacked")), (Words("tabbed"), Words("As tabs"))]),
        default: Words("normal"),
        ..BASE
    },
    LayoutGaps => Spec {
        page: Page::Layout,
        section: Some(Section::Spacing),
        label: Words("Gaps"),
        help: Words("Space between windows and around the screen edge, in logical pixels."),
        chain: LAYOUT,
        child: Words("gaps"),
        kind: Kind::Float {
            min: 0.0,
            max: 256.0,
        },
        default: Words("16"),
        ..BASE
    },
    LayoutStrutsLeft => Spec {
        page: Page::Layout,
        section: Some(Section::Spacing),
        label: Words("Extra space on the left"),
        help: Words("Room kept free at the edge, so the next column can peek in."),
        chain: STRUTS,
        child: Words("left"),
        kind: Kind::Integer { min: 0, max: 2000 },
        default: Words("0"),
        ..BASE
    },
    LayoutStrutsRight => Spec {
        page: Page::Layout,
        section: Some(Section::Spacing),
        label: Words("Extra space on the right"),
        chain: STRUTS,
        child: Words("right"),
        kind: Kind::Integer { min: 0, max: 2000 },
        default: Words("0"),
        ..BASE
    },
    LayoutStrutsTop => Spec {
        page: Page::Layout,
        section: Some(Section::Spacing),
        label: Words("Extra space at the top"),
        chain: STRUTS,
        child: Words("top"),
        kind: Kind::Integer { min: 0, max: 2000 },
        default: Words("0"),
        ..BASE
    },
    LayoutStrutsBottom => Spec {
        page: Page::Layout,
        section: Some(Section::Spacing),
        label: Words("Extra space at the bottom"),
        chain: STRUTS,
        child: Words("bottom"),
        kind: Kind::Integer { min: 0, max: 2000 },
        default: Words("0"),
        ..BASE
    },
    LayoutFocusRing => Spec {
        page: Page::Layout,
        section: Some(Section::FocusRing),
        label: Words("Focus ring"),
        help: Words("Drawn around the focused window, behind it."),
        chain: FOCUS_RING,
        kind: Kind::OnOff,
        default: Words("on"),
        note: Some(urgency_note),
        ..BASE
    },
    LayoutFocusRingWidth => Spec {
        page: Page::Layout,
        section: Some(Section::FocusRing),
        label: Words("Width"),
        chain: FOCUS_RING,
        child: Words("width"),
        kind: Kind::Integer { min: 0, max: 64 },
        default: Words("4"),
        requires: Some(Setting::LayoutFocusRing),
        ..BASE
    },
    LayoutFocusRingActiveColor => Spec {
        page: Page::Layout,
        section: Some(Section::FocusRing),
        label: Words("Active color"),
        chain: FOCUS_RING,
        child: Words("active-color"),
        kind: Kind::Color,
        default: Words("#7fc8ff"),
        requires: Some(Setting::LayoutFocusRing),
        overridden_by: Words("active-gradient"),
        ..BASE
    },
    LayoutFocusRingActiveGradient => Spec {
        page: Page::Layout,
        section: Some(Section::FocusRing),
        label: Words("Active gradient"),
        help: Words("Used instead of the active color when set."),
        chain: FOCUS_RING,
        child: Words("active-gradient"),
        kind: Kind::Gradient,
        default: Words("from=\"#80c8ff\" to=\"#bbddff\" angle=45"),
        requires: Some(Setting::LayoutFocusRing),
        ..BASE
    },
    LayoutFocusRingInactiveColor => Spec {
        page: Page::Layout,
        section: Some(Section::FocusRing),
        label: Words("Inactive color"),
        help: Words("Used on outputs that do not have focus."),
        chain: FOCUS_RING,
        child: Words("inactive-color"),
        kind: Kind::Color,
        default: Words("#505050"),
        requires: Some(Setting::LayoutFocusRing),
        overridden_by: Words("inactive-gradient"),
        ..BASE
    },
    LayoutFocusRingUrgentColor => Spec {
        page: Page::Layout,
        section: Some(Section::FocusRing),
        label: Words("Urgent color"),
        help: Words("For windows that ask for attention."),
        chain: FOCUS_RING,
        child: Words("urgent-color"),
        kind: Kind::Color,
        default: Words("#9b0000"),
        requires: Some(Setting::LayoutFocusRing),
        overridden_by: Words("urgent-gradient"),
        ..BASE
    },
    LayoutBorder => Spec {
        page: Page::Layout,
        section: Some(Section::Border),
        label: Words("Border"),
        help: Words("Like the focus ring, but on every window and taking up space."),
        chain: BORDER,
        kind: Kind::OnOff,
        default: Words("off"),
        note: Some(urgency_note),
        ..BASE
    },
    LayoutBorderWidth => Spec {
        page: Page::Layout,
        section: Some(Section::Border),
        label: Words("Width"),
        chain: BORDER,
        child: Words("width"),
        kind: Kind::Integer { min: 0, max: 64 },
        default: Words("4"),
        requires: Some(Setting::LayoutBorder),
        ..BASE
    },
    LayoutBorderActiveColor => Spec {
        page: Page::Layout,
        section: Some(Section::Border),
        label: Words("Active color"),
        chain: BORDER,
        child: Words("active-color"),
        kind: Kind::Color,
        default: Words("#ffc87f"),
        requires: Some(Setting::LayoutBorder),
        overridden_by: Words("active-gradient"),
        ..BASE
    },
    LayoutBorderActiveGradient => Spec {
        page: Page::Layout,
        section: Some(Section::Border),
        label: Words("Active gradient"),
        help: Words("Used instead of the active color when set."),
        chain: BORDER,
        child: Words("active-gradient"),
        kind: Kind::Gradient,
        default: Words("from=\"#ffbb66\" to=\"#ffc880\" angle=45"),
        requires: Some(Setting::LayoutBorder),
        ..BASE
    },
    LayoutBorderInactiveColor => Spec {
        page: Page::Layout,
        section: Some(Section::Border),
        label: Words("Inactive color"),
        chain: BORDER,
        child: Words("inactive-color"),
        kind: Kind::Color,
        default: Words("#505050"),
        requires: Some(Setting::LayoutBorder),
        overridden_by: Words("inactive-gradient"),
        ..BASE
    },
    LayoutBorderUrgentColor => Spec {
        page: Page::Layout,
        section: Some(Section::Border),
        label: Words("Urgent color"),
        help: Words("For windows that ask for attention."),
        chain: BORDER,
        child: Words("urgent-color"),
        kind: Kind::Color,
        default: Words("#9b0000"),
        requires: Some(Setting::LayoutBorder),
        overridden_by: Words("urgent-gradient"),
        ..BASE
    },
    LayoutShadow => Spec {
        page: Page::Layout,
        section: Some(Section::Shadow),
        label: Words("Shadow"),
        chain: SHADOW,
        kind: Kind::OnOff,
        default: Words("off"),
        ..BASE
    },
    LayoutShadowSoftness => Spec {
        page: Page::Layout,
        section: Some(Section::Shadow),
        label: Words("Softness"),
        help: Words("Blur radius in logical pixels."),
        chain: SHADOW,
        child: Words("softness"),
        kind: Kind::Integer { min: 0, max: 256 },
        default: Words("30"),
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowSpread => Spec {
        page: Page::Layout,
        section: Some(Section::Shadow),
        label: Words("Spread"),
        chain: SHADOW,
        child: Words("spread"),
        kind: Kind::Integer { min: 0, max: 256 },
        default: Words("5"),
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowColor => Spec {
        page: Page::Layout,
        section: Some(Section::Shadow),
        label: Words("Color"),
        chain: SHADOW,
        child: Words("color"),
        kind: Kind::Color,
        default: Words("#00000077"),
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowInactiveColor => Spec {
        page: Page::Layout,
        section: Some(Section::Shadow),
        label: Words("Color of unfocused windows"),
        chain: SHADOW,
        child: Words("inactive-color"),
        kind: Kind::Color,
        default: Words("the same color"),
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutShadowDrawBehindWindow => Spec {
        page: Page::Layout,
        section: Some(Section::Shadow),
        label: Words("Draw behind the window too"),
        help: Words("For windows with see-through corners of their own."),
        chain: SHADOW,
        child: Words("draw-behind-window"),
        kind: Kind::Bool,
        default: Words("false"),
        requires: Some(Setting::LayoutShadow),
        ..BASE
    },
    LayoutTabIndicator => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Tab indicator"),
        help: Words("The strip beside a column that shows its windows as tabs."),
        chain: TABS,
        kind: Kind::OnOff,
        default: Words("on"),
        ..BASE
    },
    LayoutTabIndicatorHideWhenSingleTab => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Hide it for a single tab"),
        chain: TABS,
        child: Words("hide-when-single-tab"),
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorPlaceWithinColumn => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Draw it inside the column"),
        chain: TABS,
        child: Words("place-within-column"),
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorPosition => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Side"),
        chain: TABS,
        child: Words("position"),
        kind: Kind::Choice(SIDES),
        default: Words("left"),
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorWidth => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Width"),
        chain: TABS,
        child: Words("width"),
        kind: Kind::Integer { min: 0, max: 64 },
        default: Words("4"),
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorGap => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Gap to the window"),
        chain: TABS,
        child: Words("gap"),
        kind: Kind::Float {
            min: -64.0,
            max: 64.0,
        },
        default: Words("5"),
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorCornerRadius => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Corner radius"),
        chain: TABS,
        child: Words("corner-radius"),
        kind: Kind::Float {
            min: 0.0,
            max: 64.0,
        },
        default: Words("0"),
        requires: Some(Setting::LayoutTabIndicator),
        ..BASE
    },
    LayoutTabIndicatorActiveColor => Spec {
        page: Page::Layout,
        section: Some(Section::Tabs),
        label: Words("Active color"),
        chain: TABS,
        child: Words("active-color"),
        kind: Kind::Color,
        default: Words("the focus ring's"),
        requires: Some(Setting::LayoutTabIndicator),
        overridden_by: Words("active-gradient"),
        ..BASE
    },
    LayoutInsertHint => Spec {
        page: Page::Layout,
        section: Some(Section::InsertHint),
        label: Words("Insert hint"),
        help: Words("Shows where a dragged window will land."),
        chain: INSERT_HINT,
        kind: Kind::OnOff,
        default: Words("on"),
        ..BASE
    },
    LayoutInsertHintColor => Spec {
        page: Page::Layout,
        section: Some(Section::InsertHint),
        label: Words("Color"),
        chain: INSERT_HINT,
        child: Words("color"),
        kind: Kind::Color,
        default: Words("#7fc8ff80"),
        requires: Some(Setting::LayoutInsertHint),
        ..BASE
    },
    LayoutBackgroundColor => Spec {
        page: Page::Layout,
        section: Some(Section::Background),
        label: Words("Workspace background"),
        help: Words("Shown where no window or wallpaper covers the workspace."),
        chain: LAYOUT,
        child: Words("background-color"),
        kind: Kind::Color,
        default: Words("#404040"),
        ..BASE
    },
    // ---- Input ----
    InputKeyboardXkbLayout => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Layouts"),
        help: Words("The first one is active at startup. With none, niri uses the system layout."),
        chain: XKB,
        child: Words("layout"),
        kind: Kind::Layouts,
        ..BASE
    },
    InputKeyboardXkbVariant => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Variants"),
        help: Words("One per layout, in the same order, comma separated."),
        chain: XKB,
        child: Words("variant"),
        kind: Kind::Text {
            example: Words("nodeadkeys"),
        },
        ..BASE
    },
    InputKeyboardXkbOptions => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Options"),
        help: Words("For example a key that switches layouts."),
        chain: XKB,
        child: Words("options"),
        kind: Kind::XkbOptions,
        ..BASE
    },
    InputModKey => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Mod key"),
        help: Words("The key that Mod stands for in your shortcuts."),
        chain: INPUT,
        child: Words("mod-key"),
        kind: Kind::Choice(&[
            (Words("Super"), Words("Super")),
            (Words("Alt"), Words("Alt")),
            (Words("Ctrl"), Words("Ctrl")),
            (Words("Mod3"), Words("Mod3")),
            (Words("Mod5"), Words("Mod5")),
        ]),
        default: Words("Super"),
        ..BASE
    },
    InputKeyboardRepeatDelay => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Repeat delay"),
        help: Words("Milliseconds a key is held before it repeats."),
        chain: KEYBOARD,
        child: Words("repeat-delay"),
        kind: Kind::Integer {
            min: 100,
            max: 2000,
        },
        default: Words("600"),
        ..BASE
    },
    InputKeyboardRepeatRate => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Repeat rate"),
        help: Words("Characters per second while a key repeats."),
        chain: KEYBOARD,
        child: Words("repeat-rate"),
        kind: Kind::Integer { min: 1, max: 200 },
        default: Words("25"),
        ..BASE
    },
    InputKeyboardNumlock => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Turn on Num Lock at startup"),
        chain: KEYBOARD,
        child: Words("numlock"),
        ..BASE
    },
    InputKeyboardTrackLayout => Spec {
        page: Page::Input,
        section: Some(Section::Keyboard),
        label: Words("Remember the layout"),
        chain: KEYBOARD,
        child: Words("track-layout"),
        kind: Kind::Choice(&[(Words("global"), Words("For all windows")), (Words("window"), Words("Per window"))]),
        default: Words("global"),
        ..BASE
    },
    InputTouchpad => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Touchpad"),
        chain: TOUCHPAD,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    InputTouchpadTap => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Tap to click"),
        chain: TOUCHPAD,
        child: Words("tap"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadNaturalScroll => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Natural scrolling"),
        help: Words("Content follows your fingers."),
        chain: TOUCHPAD,
        child: Words("natural-scroll"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDwt => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Ignore the touchpad while typing"),
        chain: TOUCHPAD,
        child: Words("dwt"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDwtp => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Ignore the touchpad while the pointing stick moves"),
        chain: TOUCHPAD,
        child: Words("dwtp"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDrag => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Tap and drag"),
        help: Words("Tap, then keep the finger down to drag."),
        chain: TOUCHPAD,
        child: Words("drag"),
        kind: Kind::Bool,
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDragLock => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Keep dragging after lifting the finger"),
        chain: TOUCHPAD,
        child: Words("drag-lock"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadAccelSpeed => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Pointer speed"),
        help: Words("From -1 (slowest) to 1 (fastest)."),
        chain: TOUCHPAD,
        child: Words("accel-speed"),
        kind: Kind::Float {
            min: -1.0,
            max: 1.0,
        },
        default: Words("0"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadAccelProfile => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Acceleration"),
        chain: TOUCHPAD,
        child: Words("accel-profile"),
        kind: Kind::Choice(ACCEL_PROFILES),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadScrollMethod => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Scrolling"),
        chain: TOUCHPAD,
        child: Words("scroll-method"),
        kind: Kind::Choice(SCROLL_METHODS),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadScrollFactor => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Scroll speed"),
        help: Words("Multiplies how far each swipe scrolls."),
        chain: TOUCHPAD,
        child: Words("scroll-factor"),
        kind: Kind::Float {
            min: 0.0,
            max: 10.0,
        },
        default: Words("1"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadTapButtonMap => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Tapping with more fingers"),
        help: Words("Which click a two- or three-finger tap makes."),
        chain: TOUCHPAD,
        child: Words("tap-button-map"),
        kind: Kind::Choice(&[
            (Words("left-right-middle"), Words("2 fingers right, 3 middle")),
            (Words("left-middle-right"), Words("2 fingers middle, 3 right")),
        ]),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadClickMethod => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Right-click by"),
        chain: TOUCHPAD,
        child: Words("click-method"),
        kind: Kind::Choice(&[
            (Words("button-areas"), Words("Bottom right corner")),
            (Words("clickfinger"), Words("Two fingers")),
        ]),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadLeftHanded => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Left-handed"),
        help: Words("Swaps the left and right buttons."),
        chain: TOUCHPAD,
        child: Words("left-handed"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadMiddleEmulation => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Middle-click by pressing left and right together"),
        chain: TOUCHPAD,
        child: Words("middle-emulation"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputTouchpadDisabledOnExternalMouse => Spec {
        page: Page::Input,
        section: Some(Section::Touchpad),
        label: Words("Turn off while a mouse is connected"),
        chain: TOUCHPAD,
        child: Words("disabled-on-external-mouse"),
        requires: Some(Setting::InputTouchpad),
        ..BASE
    },
    InputMouseNaturalScroll => Spec {
        page: Page::Input,
        section: Some(Section::Mouse),
        label: Words("Natural scrolling"),
        chain: MOUSE,
        child: Words("natural-scroll"),
        ..BASE
    },
    InputMouseAccelSpeed => Spec {
        page: Page::Input,
        section: Some(Section::Mouse),
        label: Words("Pointer speed"),
        help: Words("From -1 (slowest) to 1 (fastest)."),
        chain: MOUSE,
        child: Words("accel-speed"),
        kind: Kind::Float {
            min: -1.0,
            max: 1.0,
        },
        default: Words("0"),
        ..BASE
    },
    InputMouseAccelProfile => Spec {
        page: Page::Input,
        section: Some(Section::Mouse),
        label: Words("Acceleration"),
        chain: MOUSE,
        child: Words("accel-profile"),
        kind: Kind::Choice(ACCEL_PROFILES),
        ..BASE
    },
    InputMouseScrollFactor => Spec {
        page: Page::Input,
        section: Some(Section::Mouse),
        label: Words("Scroll speed"),
        help: Words("Multiplies how far each wheel step scrolls."),
        chain: MOUSE,
        child: Words("scroll-factor"),
        kind: Kind::Float {
            min: 0.0,
            max: 10.0,
        },
        default: Words("1"),
        ..BASE
    },
    InputMouseLeftHanded => Spec {
        page: Page::Input,
        section: Some(Section::Mouse),
        label: Words("Left-handed"),
        help: Words("Swaps the left and right buttons."),
        chain: MOUSE,
        child: Words("left-handed"),
        ..BASE
    },
    InputMouseMiddleEmulation => Spec {
        page: Page::Input,
        section: Some(Section::Mouse),
        label: Words("Middle-click by pressing left and right together"),
        chain: MOUSE,
        child: Words("middle-emulation"),
        ..BASE
    },
    InputTabletMapToOutput => Spec {
        page: Page::Input,
        section: Some(Section::TabletAndTouchscreen),
        label: Words("The tablet covers"),
        chain: TABLET,
        child: Words("map-to-output"),
        kind: Kind::Output,
        default: Words("every display"),
        ..BASE
    },
    InputTouchMapToOutput => Spec {
        page: Page::Input,
        section: Some(Section::TabletAndTouchscreen),
        label: Words("The touchscreen is"),
        chain: TOUCH,
        child: Words("map-to-output"),
        kind: Kind::Output,
        default: Words("built-in"),
        ..BASE
    },
    InputFocusFollowsMouse => Spec {
        page: Page::Cursor,
        section: Some(Section::PointerAndFocus),
        label: Words("Focus follows the pointer"),
        chain: INPUT,
        child: Words("focus-follows-mouse"),
        ..BASE
    },
    InputFocusFollowsMouseMaxScrollAmount => Spec {
        page: Page::Cursor,
        section: Some(Section::PointerAndFocus),
        label: Words("How far focusing may scroll"),
        help: Words("At 0% only fully visible windows take focus; the view never moves."),
        chain: INPUT,
        child: Words("focus-follows-mouse"),
        kind: Kind::Prop {
            name: Words("max-scroll-amount"),
            example: Words("0%"),
        },
        default: Words("no limit"),
        requires: Some(Setting::InputFocusFollowsMouse),
        ..BASE
    },
    InputWarpMouseToFocus => Spec {
        page: Page::Cursor,
        section: Some(Section::PointerAndFocus),
        label: Words("Move the pointer to newly focused windows"),
        chain: INPUT,
        child: Words("warp-mouse-to-focus"),
        ..BASE
    },
    InputWorkspaceAutoBackAndForth => Spec {
        page: Page::Workspaces,
        section: Some(Section::Workspaces),
        label: Words("Switching to the current workspace goes back"),
        help: Words("Focusing the workspace you are on returns to the previous one."),
        chain: INPUT,
        child: Words("workspace-auto-back-and-forth"),
        ..BASE
    },
    InputDisablePowerKeyHandling => Spec {
        page: Page::Input,
        section: Some(Section::PowerButton),
        label: Words("Leave the power button to the system"),
        help: Words("niri then ignores it, and logind decides what it does."),
        chain: INPUT,
        child: Words("disable-power-key-handling"),
        ..BASE
    },
    CursorXcursorTheme => Spec {
        page: Page::Cursor,
        section: Some(Section::Cursor),
        label: Words("Theme"),
        chain: CURSOR,
        child: Words("xcursor-theme"),
        kind: Kind::CursorTheme,
        default: Words("default"),
        ..BASE
    },
    CursorXcursorSize => Spec {
        page: Page::Cursor,
        section: Some(Section::Cursor),
        label: Words("Size"),
        chain: CURSOR,
        child: Words("xcursor-size"),
        kind: Kind::Integer { min: 8, max: 256 },
        default: Words("24"),
        ..BASE
    },
    CursorHideWhenTyping => Spec {
        page: Page::Cursor,
        section: Some(Section::Cursor),
        label: Words("Hide while typing"),
        chain: CURSOR,
        child: Words("hide-when-typing"),
        ..BASE
    },
    CursorHideAfterInactiveMs => Spec {
        page: Page::Cursor,
        section: Some(Section::Cursor),
        label: Words("Hide after inactivity"),
        help: Words("Milliseconds without movement."),
        chain: CURSOR,
        child: Words("hide-after-inactive-ms"),
        kind: Kind::Integer {
            min: 0,
            max: 3_600_000,
        },
        default: Words("never"),
        ..BASE
    },
    // ---- Animations ----
    Animations => Spec {
        page: Page::Animations,
        section: Some(Section::AllAnimations),
        label: Words("Animations"),
        chain: ANIMATIONS,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    AnimationsSlowdown => Spec {
        page: Page::Animations,
        section: Some(Section::AllAnimations),
        label: Words("Slowdown"),
        help: Words("Multiplies every duration: 2 makes them twice as slow."),
        chain: ANIMATIONS,
        child: Words("slowdown"),
        kind: Kind::Float {
            min: 0.0,
            max: 100.0,
        },
        default: Words("1"),
        requires: Some(Setting::Animations),
        ..BASE
    },
    // ---- General ----
    PreferNoCsd => Spec {
        page: Page::Environment,
        section: Some(Section::Windows),
        label: Words("Ask apps to drop their own title bars"),
        help: Words("Apps that agree draw no title bars, and niri's ring and border then frame them rather than show through them."),
        child: Words("prefer-no-csd"),
        ..BASE
    },
    GesturesHotCorners => Spec {
        page: Page::Gestures,
        section: Some(Section::HotCorners),
        label: Words("Hot corners"),
        help: Words("Push the pointer into a corner to open the overview."),
        chain: HOT_CORNERS,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    GesturesHotCornersTopLeft => Spec {
        page: Page::Gestures,
        section: Some(Section::HotCorners),
        label: Words("Top left"),
        chain: HOT_CORNERS,
        child: Words("top-left"),
        requires: Some(Setting::GesturesHotCorners),
        note: Some(fallback_corner_note),
        ..BASE
    },
    GesturesHotCornersTopRight => Spec {
        page: Page::Gestures,
        section: Some(Section::HotCorners),
        label: Words("Top right"),
        chain: HOT_CORNERS,
        child: Words("top-right"),
        requires: Some(Setting::GesturesHotCorners),
        ..BASE
    },
    GesturesHotCornersBottomLeft => Spec {
        page: Page::Gestures,
        section: Some(Section::HotCorners),
        label: Words("Bottom left"),
        chain: HOT_CORNERS,
        child: Words("bottom-left"),
        requires: Some(Setting::GesturesHotCorners),
        ..BASE
    },
    GesturesHotCornersBottomRight => Spec {
        page: Page::Gestures,
        section: Some(Section::HotCorners),
        label: Words("Bottom right"),
        chain: HOT_CORNERS,
        child: Words("bottom-right"),
        requires: Some(Setting::GesturesHotCorners),
        ..BASE
    },
    OverviewZoom => Spec {
        page: Page::Overview,
        section: Some(Section::Overview),
        label: Words("Zoom"),
        help: Words("How small workspaces get in the overview."),
        chain: OVERVIEW,
        child: Words("zoom"),
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: Words("0.5"),
        ..BASE
    },
    OverviewBackdropColor => Spec {
        page: Page::Overview,
        section: Some(Section::Overview),
        label: Words("Backdrop"),
        help: Words("Shown around the workspaces. The Displays page sets it per display."),
        chain: OVERVIEW,
        child: Words("backdrop-color"),
        kind: Kind::Color,
        default: Words(BACKDROP),
        ..BASE
    },
    OverviewWorkspaceShadow => Spec {
        page: Page::Overview,
        section: Some(Section::Overview),
        label: Words("Workspace shadow"),
        chain: WORKSPACE_SHADOW,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    RecentWindows => Spec {
        page: Page::Overview,
        section: Some(Section::WindowSwitcher),
        label: Words("Window switcher"),
        help: Words("Alt+Tab and Mod+Tab show your recent windows."),
        chain: RECENT,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    RecentWindowsOpenDelayMs => Spec {
        page: Page::Overview,
        section: Some(Section::WindowSwitcher),
        label: Words("Delay before it shows"),
        help: Words("Milliseconds. A quick Alt+Tab switches without showing it."),
        chain: RECENT,
        child: Words("open-delay-ms"),
        kind: Kind::Integer { min: 0, max: 5000 },
        default: Words("150"),
        requires: Some(Setting::RecentWindows),
        ..BASE
    },
    RecentWindowsDebounceMs => Spec {
        page: Page::Overview,
        section: Some(Section::WindowSwitcher),
        label: Words("Time to count as recent"),
        help: Words("Milliseconds a window must keep focus to join the list."),
        chain: RECENT,
        child: Words("debounce-ms"),
        kind: Kind::Integer {
            min: 0,
            max: 10_000,
        },
        default: Words("750"),
        requires: Some(Setting::RecentWindows),
        ..BASE
    },
    RecentWindowsHighlightActiveColor => Spec {
        page: Page::Overview,
        section: Some(Section::WindowSwitcher),
        label: Words("Highlight color"),
        chain: RECENT_HIGHLIGHT,
        child: Words("active-color"),
        kind: Kind::Color,
        default: Words("#999999ff"),
        requires: Some(Setting::RecentWindows),
        ..BASE
    },
    Blur => Spec {
        page: Page::Layout,
        section: Some(Section::Blur),
        label: Words("Blur"),
        help: Words("Behind windows and panels that ask for it, and those a window rule blurs."),
        chain: BLUR,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    BlurPasses => Spec {
        page: Page::Layout,
        section: Some(Section::Blur),
        label: Words("Passes"),
        help: Words("More passes blur more and cost more."),
        chain: BLUR,
        child: Words("passes"),
        kind: Kind::Integer { min: 1, max: 8 },
        default: Words("3"),
        requires: Some(Setting::Blur),
        ..BASE
    },
    BlurOffset => Spec {
        page: Page::Layout,
        section: Some(Section::Blur),
        label: Words("Offset"),
        help: Words("How far each pass reaches."),
        chain: BLUR,
        child: Words("offset"),
        kind: Kind::Float {
            min: 0.0,
            max: 20.0,
        },
        default: Words("3"),
        requires: Some(Setting::Blur),
        ..BASE
    },
    BlurNoise => Spec {
        page: Page::Layout,
        section: Some(Section::Blur),
        label: Words("Noise"),
        help: Words("A little grain hides banding."),
        chain: BLUR,
        child: Words("noise"),
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: Words("0.02"),
        requires: Some(Setting::Blur),
        ..BASE
    },
    BlurSaturation => Spec {
        page: Page::Layout,
        section: Some(Section::Blur),
        label: Words("Saturation"),
        chain: BLUR,
        child: Words("saturation"),
        kind: Kind::Float { min: 0.0, max: 2.0 },
        default: Words("1.5"),
        requires: Some(Setting::Blur),
        ..BASE
    },
    ScreenshotPath => Spec {
        page: Page::Screenshots,
        section: Some(Section::Screenshots),
        label: Words("Save screenshots to"),
        help: Words("Codes such as %Y-%m-%d become the date. niri creates the last folder if needed."),
        child: Words("screenshot-path"),
        kind: Kind::Screenshot,
        default: Words("~/Pictures/Screenshots/Screenshot from %Y-%m-%d %H-%M-%S.png"),
        ..BASE
    },
    HotkeyOverlaySkipAtStartup => Spec {
        page: Page::Overview,
        section: Some(Section::ShortcutOverview),
        label: Words("Skip the shortcut overview at startup"),
        chain: HOTKEY_OVERLAY,
        child: Words("skip-at-startup"),
        ..BASE
    },
    HotkeyOverlayHideNotBound => Spec {
        page: Page::Overview,
        section: Some(Section::ShortcutOverview),
        label: Words("Leave out actions that have no key"),
        chain: HOTKEY_OVERLAY,
        child: Words("hide-not-bound"),
        ..BASE
    },
    ClipboardDisablePrimary => Spec {
        page: Page::Environment,
        section: Some(Section::Clipboard),
        label: Words("Turn off middle-click paste"),
        help: Words("Disables the primary selection."),
        chain: CLIPBOARD,
        child: Words("disable-primary"),
        ..BASE
    },
    XwaylandSatellite => Spec {
        page: Page::Environment,
        section: Some(Section::X11Apps),
        label: Words("Run X11 apps"),
        help: Words("Through xwayland-satellite, started when an X11 app needs it."),
        chain: XWAYLAND,
        child: Words("off"),
        kind: Kind::Enabled,
        ..BASE
    },
    XwaylandSatellitePath => Spec {
        page: Page::Environment,
        section: Some(Section::X11Apps),
        label: Words("Program"),
        chain: XWAYLAND,
        child: Words("path"),
        kind: Kind::File {
            example: Words("/usr/bin/xwayland-satellite"),
        },
        default: Words("xwayland-satellite"),
        requires: Some(Setting::XwaylandSatellite),
        ..BASE
    },
}

/// niri's backdrop color, for the overview and for each display.
const BACKDROP: &str = "#262626";

const TRANSFORMS: &[(Words, Words)] = &[
    (Words("normal"), Words("Normal")),
    (Words("90"), Words("Turned 90 degrees")),
    (Words("180"), Words("Upside down")),
    (Words("270"), Words("Turned 270 degrees")),
    (Words("flipped"), Words("Mirrored")),
    (Words("flipped-90"), Words("Mirrored, turned 90 degrees")),
    (Words("flipped-180"), Words("Mirrored, upside down")),
    (Words("flipped-270"), Words("Mirrored, turned 270 degrees")),
];

table! {
    /// A setting of one display, in its `output "<name>"` block. The
    /// Displays page fills in the chain for the display it shows.
    OutputSetting, OUTPUT_SETTINGS;
    Mode => Spec {
        page: Page::Displays,
        label: Words("Resolution and refresh rate"),
        child: Words("mode"),
        kind: Kind::Text {
            example: Words("1920x1080@60"),
        },
        ..BASE
    },
    Scale => Spec {
        page: Page::Displays,
        label: Words("Scale"),
        child: Words("scale"),
        kind: Kind::Float {
            min: 0.25,
            max: 10.0,
        },
        ..BASE
    },
    Transform => Spec {
        page: Page::Displays,
        label: Words("Rotation"),
        child: Words("transform"),
        kind: Kind::Choice(TRANSFORMS),
        default: Words("normal"),
        ..BASE
    },
    VariableRefreshRate => Spec {
        page: Page::Displays,
        label: Words("Variable refresh rate"),
        child: Words("variable-refresh-rate"),
        kind: Kind::Switch,
        ..BASE
    },
    BackdropColor => Spec {
        page: Page::Displays,
        label: Words("Backdrop"),
        help: Words("Shown around workspaces in the overview."),
        child: Words("backdrop-color"),
        kind: Kind::Color,
        default: Words(BACKDROP),
        ..BASE
    },
    Off => Spec {
        page: Page::Displays,
        label: Words("Turn this display off"),
        child: Words("off"),
        kind: Kind::Switch,
        ..BASE
    },
}

/// Settings a display or a workspace can set for itself, in its own
/// `output` or `workspace` block (niri 25.11).
struct Scope {
    /// The global block the settings live in: `layout`.
    global: Words,
    /// Only these of its children; all when empty.
    children: &'static [Words],
    /// The blocks that can set them for themselves.
    within: &'static [Words],
    /// The global block appears again inside (`output { layout { gaps } }`),
    /// or the setting stands in it directly (`output { backdrop-color }`).
    repeats_block: bool,
}

const SCOPES: [Scope; 2] = [
    Scope {
        global: Words("layout"),
        children: &[],
        within: &[Words("output"), Words("workspace")],
        repeats_block: true,
    },
    Scope {
        global: Words("overview"),
        children: &[Words("backdrop-color")],
        within: &[Words("output")],
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
        scope.global.as_str() == first.name
            && (scope.children.is_empty() || scope.children.iter().any(|name| **name == *child))
    }) else {
        return Vec::new();
    };
    let inside: Vec<Block> = if scope.repeats_block {
        std::iter::once(Block::named(scope.global.as_str()))
            .chain(rest.iter().cloned())
            .collect()
    } else {
        rest.to_vec()
    };
    let within: Vec<&str> = scope.within.iter().map(|name| name.as_str()).collect();
    store.overrides(&within, &inside, child)
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
                parts.push(self.child.as_str());
                parts.push(name.as_str());
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
            | Kind::Screenshot => parts.push(self.child.as_str()),
        }
        parts.join(" › ")
    }

    pub fn read(&self, store: &Store) -> Reading {
        read(store, self.chain, self.child.as_str(), self.kind)
    }

    pub fn write(&self, store: &mut Store, reading: &Reading) -> Result<(), StoreError> {
        write(store, self.chain, self.child.as_str(), self.kind, reading)
    }

    /// Whether niri uses this setting with the config as it is: an on/off
    /// section that is off makes the rows that depend on it moot.
    pub fn is_on(&self, store: &Store) -> bool {
        match self.read(store) {
            Reading::Switch(on) => on,
            Reading::OnOff(OnOff::On) | Reading::Value(_) => true,
            Reading::OnOff(OnOff::Off) => false,
            Reading::OnOff(OnOff::Default) => *self.default == *"on",
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
            Kind::Gradient | Kind::Screenshot => Reading::Value(Some(self.default.to_string())),
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
                .and_then(|node| node.props.iter().find(|prop| prop.name == *name))
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
        ("proportion", Ok(proportion)) => {
            for (fraction, label) in [
                (1.0 / 4.0, "1/4"),
                (1.0 / 3.0, "1/3"),
                (0.5, "1/2"),
                (2.0 / 3.0, "2/3"),
                (3.0 / 4.0, "3/4"),
                (1.0, "Full"),
            ] {
                if (proportion - fraction).abs() < 0.001 {
                    return label.to_string();
                }
            }
            format!("{}%", (proportion * 100.0).round())
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
            .filter(|number| (min..=max).contains(number))
            .map(|number| Some(number.to_string()))
            .ok_or_else(|| format!("Enter a whole number from {min} to {max}.")),
        Kind::Float { min, max } => input
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite() && (min..=max).contains(number))
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
            .filter(|number| number.is_finite() && *number >= 0.0)
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
            "proportion" => value.parse::<f64>().is_ok_and(|proportion| {
                proportion.is_finite() && proportion > 0.0 && proportion <= 1.0
            }),
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

/// The settings of `page` that the config sets to something, in table
/// order: what going back to niri's defaults for the page takes out.
pub fn set_on(store: &Store, page: Page) -> impl Iterator<Item = Setting> + '_ {
    Setting::ALL.iter().copied().filter(move |setting| {
        let spec = setting.spec();
        spec.page == page && spec.read(store) != unset(spec.kind)
    })
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
    write(store, chain, spec.child.as_str(), spec.kind, &reading).map_err(|error| error.to_string())
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
    if !setting.overridden_by.is_empty() && store.is_set(chain, &setting.overridden_by) {
        let instead = REGISTRY
            .iter()
            .find(|other| (other.chain, other.child) == (setting.chain, setting.overridden_by))
            .map_or(setting.overridden_by, |other| other.label);
        notes.push(Note::info(format!("{instead} is used instead.")));
    }
    notes.extend(setting.note.and_then(|note| note(store)));
    let overrides: Vec<String> = overrides(store, chain, &setting.child)
        .iter()
        .map(ToString::to_string)
        .collect();
    if !overrides.is_empty() {
        notes.push(Note::info(format!(
            "{} its own value, used there instead.",
            who(&overrides, "sets", "set")
        )));
    }
    if let Some(file) = store.setting_file(chain, &setting.child)
        && let Some(owner) = managed_by(store, file)
    {
        notes.push(Note::warn(format!(
            "This value is in {}, which {owner} rewrites, so a change here may not last.",
            store.file_label(file)
        )));
    }
    if setting.kind == Kind::CursorTheme
        && let Reading::Value(Some(theme_name)) = read(store, chain, &setting.child, setting.kind)
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
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
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
    let typed = default.starts_with(|character: char| {
        character.is_ascii_digit() || matches!(character, '#' | '-' | '.' | '~' | '/')
    });
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
