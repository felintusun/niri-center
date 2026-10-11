//! What the app starts from: where your config is, the disk actor that
//! reads it, what is installed, and the app's own preferences. The browser
//! demo starts from `demo` instead.

use std::path::PathBuf;

use crate::disk::actor::Actor;
use crate::disk::{Disk, Folders};
use crate::session::{Platform, SaveWhen};
use crate::store::Store;
use crate::system::SystemData;
use crate::xdg::Places;

/// The config to edit, yours or in the browser demo niri's default; `None`
/// when no config location is known.
pub fn config_path() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    return Some(crate::demo::path());
    #[cfg(not(target_arch = "wasm32"))]
    Store::default_path()
}

/// The disk actor for where the app runs: the real files, with the app's
/// own folders where XDG puts them, or the browser demo's files in memory.
/// Without a home folder there is no config to find and nowhere to keep
/// anything: nothing is read or written.
pub fn disk_actor(platform: Platform) -> Actor {
    match platform {
        Platform::Desktop => match Places::find() {
            Some(places) => Actor::new(Disk::files(Folders::of(&places)), Some(places.history)),
            None => Actor::new(Disk::memory([]), None),
        },
        Platform::Browser | Platform::InMemory => Actor::new(crate::demo::disk(), None),
    }
}

/// What is installed: read from this system, or the demo's sample.
pub fn system() -> SystemData {
    #[cfg(target_arch = "wasm32")]
    return crate::demo::system();
    #[cfg(not(target_arch = "wasm32"))]
    SystemData::load()
}

/// Apply the Landlock sandbox where it applies, and say how it went, for
/// the Files page.
///
/// Called once iced holds its GPU and display handles: the sandbox checks
/// files opened later, so applying it earlier denies them.
pub fn sandbox(platform: Platform, store: &Store) -> Option<String> {
    match platform {
        Platform::Browser => Some(
            "This demo runs in your browser on a bundled config and writes no files.".to_string(),
        ),
        Platform::InMemory => None,
        Platform::Desktop => crate::sandbox::apply(&crate::disk::config_dirs(store.path()))
            .ok()
            .map(|status| status.describe()),
    }
}

/// When edits are saved, as the Files page set it.
pub(crate) fn save_when() -> SaveWhen {
    if cfg!(target_arch = "wasm32") {
        return SaveWhen::AfterChange;
    }
    Places::find()
        .and_then(|places| crate::prefs::get(&places.preferences, "save"))
        .as_deref()
        .and_then(SaveWhen::read)
        .unwrap_or_default()
}

/// Keep when edits are saved, for the next start.
pub fn set_save_when(when: SaveWhen) -> std::io::Result<()> {
    let places = Places::find().ok_or(std::io::ErrorKind::NotFound)?;
    crate::prefs::set(&places.preferences, "save", when.word())
}
