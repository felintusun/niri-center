# ADR: saved a moment after the change, with a history of exact texts

**Status:** accepted 2026-10-08, replacing review-before-apply and Preview;
amended 2026-10-09 and 2026-10-11. Held provisionally (AGENTS.md
[provisional]).

**Decision.**

- An edit is saved a moment after the last change to it
  (`session::SAVE_DELAY`), once `niri validate` accepts the whole include
  set; a change niri turns down stays unsaved with niri's words, and Undo
  drops it.
- Every save, and every edit another program makes (noticed on niri's
  config reload and at start), is a history entry in the state dir holding
  each touched file's full text before and after, so undo writes back exact
  text and stays lossless.
- History lists versions, not undoable changes (since 2026-10-09): each
  shows what its change did, and "Restore this version" writes the files
  back as they were right after it, as one new change the header's Undo
  takes back.
- Display changes (mode, scale, rotation, position) go back after
  `TRIAL_SECONDS` unless kept; a second display change joins the running
  trial, and closing mid-trial goes back.
- Saving waits while niri cannot read the config (checked at start too, so
  an old breakage is not blamed on the next change); when niri turns an
  edit down, the edits before it are saved and only it stays; files the app
  cannot write (read-only, a Nix store link) are found at load and their
  changes can be copied as a diff.
- "Save when I press Save" (Files page, kept in the preferences file) is
  the opt-in for people who want the diff first. One-off actions (adding a
  library rule, restoring a backup or snapshot, the typo fix) wait like any
  edit, so it holds for them too; only Save, closing, Undo and Restore, and
  display changes on trial save at once.
- Since 2026-10-11 every save is a job for the disk actor, which holds these
  invariants: one save in flight, sent in order; each job carries every
  file's text as saved and as staged; a file whose text on disk is no longer
  the saved one is not written over; niri checks the job's own texts; files
  are staged beside the originals and renamed only when all are staged, so
  a save is whole or nothing. An edit made while a save is written stays
  staged for the next. Closing waits for that save, and when edits cannot
  be saved the dialog says why. The store's revision moves only when a
  file's text does. The wait is a timer started with it, not a poll, and a
  history entry's file texts are read when something opens or undoes it.

**Why.** Reviewing before applying added a step most changes do not need;
History shows the diff afterwards, and Undo is one click. The per-change
undo beside "go back to before" confused people, hence versions.

**Rejected alternatives.** jj-lib for the history: 193 crates against the
app's 275, a 0.x API that breaks monthly, and no wasm build.

**Falsifiers.** People miss checking a change before niri sees it (History
shows the diff only afterwards).
