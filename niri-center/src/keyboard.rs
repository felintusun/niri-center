//! Keyboards as the Keybinds page pictures them: the rows of keys on a
//! layout, by the keysym names niri binds use, and the caps' labels.

use crate::bindings::key_id;

/// The key a combo ends on: `T` in `Mod+Shift+T`.
pub fn last_key(combo: &str) -> &str {
    combo.rsplit('+').next().unwrap_or("")
}

/// Whether two key names are the same key: niri reads them in any case.
pub fn same_key(a: &str, b: &str) -> bool {
    key_id(a) == key_id(b)
}

/// A physical keyboard picture, chosen from the configured xkb layout.
///
/// Rows hold the keysym names niri binds use on that layout (niri matches
/// binds against the active layout's keysyms, so on `de` the key labelled
/// Z sits where US has Y). Tokens are matched case-insensitively against the
/// trailing token of each combo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Layout {
    #[default]
    Us,
    De,
}

impl Layout {
    /// Picture for an xkb layout code like `de` or `us(intl)`; `None` when
    /// there is no picture for it yet.
    pub fn from_xkb(code: &str) -> Option<Self> {
        match code.split('(').next().unwrap_or("").trim() {
            "us" => Some(Self::Us),
            "de" => Some(Self::De),
            _ => None,
        }
    }

    pub const fn rows(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Us => &US_ROWS,
            Self::De => &DE_ROWS,
        }
    }
}

/// Modifier and navigation rows shared by every layout.
const MOD_ROW: &[&str] = &["Ctrl", "Super", "Alt", "Space", "Alt", "Super", "Ctrl"];
const NAV_ROW: &[&str] = &[
    "Escape",
    "Print",
    "Delete",
    "Home",
    "End",
    "Page_Up",
    "Page_Down",
    "Left",
    "Down",
    "Up",
    "Right",
];

/// US ANSI.
const US_ROWS: [&[&str]; 6] = [
    &[
        "Grave",
        "1",
        "2",
        "3",
        "4",
        "5",
        "6",
        "7",
        "8",
        "9",
        "0",
        "Minus",
        "Equal",
        "BackSpace",
    ],
    &[
        "Tab",
        "Q",
        "W",
        "E",
        "R",
        "T",
        "Y",
        "U",
        "I",
        "O",
        "P",
        "BracketLeft",
        "BracketRight",
        "Backslash",
    ],
    &[
        "Caps_Lock",
        "A",
        "S",
        "D",
        "F",
        "G",
        "H",
        "J",
        "K",
        "L",
        "Semicolon",
        "Apostrophe",
        "Return",
    ],
    &[
        "Shift", "Z", "X", "C", "V", "B", "N", "M", "Comma", "Period", "Slash", "Shift",
    ],
    MOD_ROW,
    NAV_ROW,
];

/// German ISO (QWERTZ).
const DE_ROWS: [&[&str]; 6] = [
    &[
        "dead_circumflex",
        "1",
        "2",
        "3",
        "4",
        "5",
        "6",
        "7",
        "8",
        "9",
        "0",
        "ssharp",
        "dead_acute",
        "BackSpace",
    ],
    &[
        "Tab",
        "Q",
        "W",
        "E",
        "R",
        "T",
        "Z",
        "U",
        "I",
        "O",
        "P",
        "udiaeresis",
        "plus",
        "Return",
    ],
    &[
        "Caps_Lock",
        "A",
        "S",
        "D",
        "F",
        "G",
        "H",
        "J",
        "K",
        "L",
        "odiaeresis",
        "adiaeresis",
        "numbersign",
    ],
    &[
        "Shift", "less", "Y", "X", "C", "V", "B", "N", "M", "Comma", "Period", "Minus", "Shift",
    ],
    MOD_ROW,
    NAV_ROW,
];

/// Short cap label for a keysym; punctuation shows its glyph.
pub fn key_label(token: &str) -> &str {
    match token {
        "Grave" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "BackSpace" => "Backspace",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Caps_Lock" => "Caps",
        "Semicolon" => ";",
        "Apostrophe" => "'",
        "Return" => "Enter",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Escape" => "Esc",
        "Delete" => "Del",
        "Page_Up" => "PgUp",
        "Page_Down" => "PgDn",
        "Left" => "←",
        "Down" => "↓",
        "Up" => "↑",
        "Right" => "→",
        "dead_circumflex" => "^",
        "ssharp" => "ß",
        "dead_acute" => "´",
        "udiaeresis" => "Ü",
        "odiaeresis" => "Ö",
        "adiaeresis" => "Ä",
        "plus" => "+",
        "numbersign" => "#",
        "less" => "<",
        other => other,
    }
}

/// Relative width of special keys (others are 1 unit).
pub fn key_width(token: &str) -> f32 {
    match token {
        "BackSpace" | "Shift" | "Caps_Lock" | "Backslash" => 1.8,
        "Tab" | "Return" => 1.5,
        "Ctrl" | "Super" | "Alt" => 1.3,
        "Space" => 5.0,
        _ => 1.0,
    }
}
