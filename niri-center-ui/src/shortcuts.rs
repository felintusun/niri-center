//! The app's own keyboard shortcuts. What the search finds, and the keys
//! the help card lists, are `niri_center::search`.
//!
//! Shortcuts deliberately use Ctrl (and F1, Escape, Alt+Left and the
//! arrows): niri grabs Mod combos before the app ever sees them, and bare
//! letters would collide with typing into the search and binding fields.

use iced::keyboard::{self, Key, key::Named};

/// App-level keyboard shortcuts.
#[derive(Debug, Clone)]
pub enum Shortcut {
    Search,
    Goto(usize),
    Previous,
    Next,
    ToggleSource,
    /// Fold the sidebar to icons, or open it.
    ToggleSidebar,
    /// Open the config column as a window of its own.
    PopOutSource,
    /// In that window: close it, the column goes back into the app.
    PutBack,
    Help,
    /// Save an edit that is still waiting for its moment.
    Save,
    Undo,
    /// Enter: keeps a display change on trial.
    Confirm,
    Close,
    /// Alt+Left: back up one level on a page that goes in, as Escape does.
    Back,
    Up,
    Down,
    /// Text typed with no field focused: it starts a search.
    Type(String),
}

/// Map a raw keyboard event to a shortcut. Runs on every event, captured or
/// not, so Escape still closes the search while its text field has focus;
/// typed text counts only when no field took it.
pub fn from_event(event: &keyboard::Event, captured: bool) -> Option<Shortcut> {
    let keyboard::Event::KeyPressed {
        key,
        modifiers,
        text,
        ..
    } = event
    else {
        return None;
    };
    let ctrl = modifiers.command();
    match key.as_ref() {
        Key::Named(Named::F1) => Some(Shortcut::Help),
        Key::Named(Named::Escape) => Some(Shortcut::Close),
        // a text field's own Enter (submit) is not the app's
        Key::Named(Named::Enter) if !captured => Some(Shortcut::Confirm),
        Key::Named(Named::ArrowLeft) if modifiers.alt() => Some(Shortcut::Back),
        Key::Named(Named::ArrowUp) => Some(Shortcut::Up),
        Key::Named(Named::ArrowDown) => Some(Shortcut::Down),
        Key::Named(Named::PageUp) if ctrl => Some(Shortcut::Previous),
        Key::Named(Named::PageDown) if ctrl => Some(Shortcut::Next),
        Key::Character(c) if ctrl => match c {
            "k" | "f" => Some(Shortcut::Search),
            "?" => Some(Shortcut::Help),
            "." => Some(Shortcut::ToggleSource),
            "b" => Some(Shortcut::ToggleSidebar),
            "n" | "N" if modifiers.shift() => Some(Shortcut::PopOutSource),
            "w" => Some(Shortcut::PutBack),
            "s" => Some(Shortcut::Save),
            // Ctrl+Z in a text field is about its text, not the config
            "z" if !captured => Some(Shortcut::Undo),
            "0" => Some(Shortcut::Goto(9)),
            digit => digit
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=9).contains(n))
                .map(|n| Shortcut::Goto(n - 1)),
        },
        Key::Character(_) if !captured && !modifiers.alt() && !modifiers.logo() => text
            .as_ref()
            .filter(|typed| typed.chars().all(|c| !c.is_control()) && !typed.trim().is_empty())
            .map(|typed| Shortcut::Type(typed.to_string())),
        Key::Named(_) | Key::Character(_) | Key::Unidentified => None,
    }
}
