# ADR: a drawing-free library, plain state, two actors

**Status:** accepted 2026-10-11. Held provisionally (AGENTS.md [provisional]).

**Decision.**

- **Two crates.** `niri-center`, the library, is everything that does not
  draw: the config and its edits, saving and the history (`session.rs`), the
  settings table, the words for a change, what niri, the portal and the
  system report, the demo's world. `niri-center-ui` is iced: widgets,
  canvases, the theme, and each page's state, messages and `update` in its
  module (`niri-center-ui/src/pages/`, `niri-center-ui/src/history.rs`); `App` and `update.rs` keep only what
  pages share.
- **Plain owned state.** No `Arc`, `Mutex`, `RefCell`, `dyn` or statics
  holding app state. What pages draw from the config (rows, the library's
  matches, the apps index, the search index) is worked out when what it
  comes from moves (the config's revision, the open windows), not cached
  behind interior mutability.
- **Two actors own what blocks**, each a plain struct whose loop ends when
  every sender is gone: the disk actor (`niri-center/src/disk/actor.rs`: files,
  `niri validate`, backups, snapshots and the history) and the niri actor
  (`niri-center/src/niri.rs`: one request connection, the event stream, picks on a
  connection of their own). Channels are made at boot. `Session` is a state
  machine with an outbox of commands; tests hand those to an actor's
  `handle` and feed the replies back, with no runtime.
- **Writes go through the disk actor.** No filesystem writes from the GUI
  layer or the store: the store is text in memory, and saving, backups,
  snapshots and symlinks are the disk actor's. One exception is left to
  fix: the preferences file, written by `start::set_save_when`.

**Why.** Code that needs no iced type is tested without a window. One owner
per blocking resource removes locks and makes every transition testable as a
function.

**Falsifiers.** A page that needs state the actors cannot express as a
command; a transition bug the `handle` tests miss but a run of the app
finds.
