//! `niri-center`: everything the app does that is not drawing it. The
//! config read and edited losslessly, saving with its history, the table
//! of settings, what niri and the system report, and the words for all of
//! it. `niri-center-ui` draws it with iced; nothing in here knows about
//! widgets.

pub mod animations; // every animation niri has, and the curves it takes
pub mod backup; // auto-backups before every save
pub mod bindings; // keybinding registry + conflict detection
pub mod by_app; // window rules and open windows, by the app they are for
pub mod cli; // helpers that shell out to the `niri` binary
pub mod clock; // the time now, also in the browser
pub mod color; // colors and gradients as niri reads them
pub mod config; // typed view over the KDL document
pub mod demo; // the browser demo's config, displays, windows and apps
pub mod diff; // line diffs of a change
pub mod disk; // the one part that reads and writes config files
pub mod forms; // the binding and window rule forms
pub mod highlight; // KDL coloring, for display only
pub mod history; // every change to the config, undoable
pub mod include; // `include`-file resolution and origin-file mapping
#[cfg(not(target_arch = "wasm32"))]
pub mod ipc; // wrapper around niri-ipc: outputs / windows / layers / actions
pub mod kdl; // lossless KDL document model + edit-through-splice engine
pub mod keyboard; // keyboards as the Keybinds page pictures them
pub mod library; // ready-made window rules, each naming its source
pub mod niri; // the running niri: displays, windows, its event stream
pub mod outputs; // displays: modes in words, arranging them
pub mod page; // the app's pages, and the part of the config each edits
pub mod portal; // the desktop's file chooser and contrast preference
pub mod prefs; // the app's own preferences
pub mod rules; // which window rules reach which windows
pub mod sandbox; // Landlock filesystem sandbox (best-effort, kernel-enforced)
pub mod search; // the header search, and the app's own keys
pub mod session; // saving, undo and the history
pub mod settings; // every simple setting, as data
pub mod snapshot; // named, restorable config snapshots
pub mod start; // what the app reads as it starts
pub mod store; // load / edit / validate / save
pub mod summary; // a change in words, and the history's names and times
pub mod system; // what is installed: xkb layouts, cursor themes, apps
pub mod typo; // a name niri does not know, and what it was meant to be
pub mod validate; // `niri validate` pre-commit gate
pub mod words; // counts, numbers and lists in the app's words
pub mod xdg; // where the app keeps its own files
