//! Saving, undo and the history, as the app sees them: the config being
//! edited, when to save it, and what came of it.
//!
//! A change is saved a moment after the last edit, so typing `24` is one
//! change, not two. The session never touches the disk: it queues
//! [`Command`]s for the disk actor and takes its [`Reply`]s. One save is in
//! flight at a time; edits made meanwhile stay unsaved and get their own.
//! Undo writes back the exact text a file had, so it never reformats.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::clock::Instant;
use crate::diff;
use crate::disk::SaveJob;
use crate::disk::actor::{Command, Outcome, Purpose, Refusal, Reply, Save};
use crate::history::{Entry, FileChange, Source};
use crate::settings::Drafts;
use crate::store::{Store, StoreError};
use crate::summary::{describe, moment, today};
use crate::typo::{self, Typo};

/// How long the app waits after the last edit before it saves.
const SAVE_DELAY: Duration = Duration::from_millis(700);

/// How long a display change runs before it goes back on its own: a
/// display that shows nothing can still be rescued by waiting.
pub const TRIAL_SECONDS: u32 = 15;

/// Where the app runs, and so what it may do: one value the app reads
/// everywhere, which tests set to the case they check.
///
/// The renderer is the one thing it leaves out: it is chosen when the app
/// is built.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Platform {
    /// The desktop: saves write files once niri accepts them, and the
    /// sandbox applies.
    Desktop,
    /// The browser demo: no files and no niri. A save moves the in-memory
    /// baseline, and what only the desktop app can do says so.
    Browser,
    /// Tests: in memory, like the demo, without its banner and notes.
    #[default]
    InMemory,
}

impl Platform {
    /// The platform this build runs on.
    pub const NATIVE: Self = if cfg!(target_arch = "wasm32") {
        Self::Browser
    } else {
        Self::Desktop
    };

    pub const fn writes_files(self) -> bool {
        matches!(self, Self::Desktop)
    }
}

/// Why the latest change is not on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// `niri validate` turned it down: its words.
    Rejected(String),
    /// Another program wrote this file after the app loaded it.
    ChangedOnDisk(String),
    /// The file cannot be written here (read-only, a Nix store link).
    ReadOnly(String),
    Failed(String),
}

/// Where the edits stand: what the header says, and what closing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState<'a> {
    /// Every change is saved.
    Saved,
    /// Edits wait for the pause after the latest one, or are being written.
    Saving,
    /// niri cannot read the config on disk, so saving waits until it can.
    WaitingForNiri,
    /// The latest change could not be saved.
    NotSaved(&'a Problem),
    /// Edits wait for Save, as the Files page sets it.
    OnRequest { files: usize },
}

/// niri cannot read the config on disk: what happened, niri's words once
/// it gives them, and the misspelled name they point at, worked out then.
#[derive(Debug, Clone)]
pub struct Health {
    pub message: String,
    pub details: Option<String>,
    pub typo: Option<Typo>,
}

impl Health {
    /// What is wrong, in a sentence: the misspelled name and what it was
    /// probably meant to be, when niri points at one.
    pub fn sentence(&self) -> String {
        let Some(typo) = &self.typo else {
            return self.message.clone();
        };
        let guess = typo
            .meant
            .as_ref()
            .map(|meant| format!(" Did you mean {meant}?"))
            .unwrap_or_default();
        format!(
            "Line {}: niri doesn't know {}.{guess}",
            typo.line, typo.name
        )
    }

    pub fn new(message: impl Into<String>, details: Option<String>, store: Option<&Store>) -> Self {
        let typo = store
            .zip(details.as_deref())
            .and_then(|(store, details)| typo::typo(store, details));
        Self {
            message: message.into(),
            details,
            typo,
        }
    }
}

/// A display change on trial: saved, and undone unless kept.
#[derive(Debug, Clone)]
pub struct Trial {
    /// The history entry of the first change on trial: going back goes to
    /// before it, so every display change made during the trial goes too.
    pub back_to: u128,
    pub what: String,
    pub seconds_left: u32,
}

/// A version of the config in the history: the start of the history, or
/// how it was right after a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// Before the oldest change the history holds.
    Start,
    /// Right after the change made at this time.
    After(u128),
}

/// What went wrong beside a save or a restore.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Saved, but the history could not be written: {0}")]
    HistoryNotWritten(String),
    #[error("Could not go back: {0}")]
    CouldNotGoBack(StoreError),
}

/// The save the disk actor is writing.
#[derive(Debug)]
struct InFlight {
    job: SaveJob,
    base: Option<SaveJob>,
    /// The config as it was saved before: rows that differ show what they
    /// were once this save is written.
    before: Store,
    /// A display change to put on trial once it is written.
    trial: bool,
}

/// When an edit is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveWhen {
    /// A moment after the latest change.
    #[default]
    AfterChange,
    /// Only when asked (Ctrl+S or Save), after a look at the diff.
    OnRequest,
}

impl SaveWhen {
    /// The word kept in the preferences file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::AfterChange => "on-change",
            Self::OnRequest => "on-request",
        }
    }

    pub fn read(word: &str) -> Option<Self> {
        [Self::AfterChange, Self::OnRequest]
            .into_iter()
            .find(|when| when.word() == word)
    }
}

/// The config being edited, what is saved of it, and its history.
#[derive(Debug, Default)]
pub struct Session {
    /// The config as edited here; `None` until it loads.
    pub store: Option<Store>,
    /// The config before the latest change: rows that differ show what
    /// they were.
    pub saved: Option<Store>,
    /// What happened to the config, oldest first.
    pub history: Vec<Entry>,
    /// Where the app runs, and so what it may do.
    pub platform: Platform,
    /// Why the latest edit is not saved.
    pub problem: Option<Problem>,
    /// niri cannot read the config on disk: saving waits.
    health: Option<Health>,
    /// Text typed into setting fields, and what is wrong with it.
    pub drafts: Drafts,
    /// A saved display change that goes back unless kept.
    pub trial: Option<Trial>,
    /// Loaded files this app cannot write (read-only, a Nix store link).
    pub read_only: Vec<PathBuf>,
    pub save_when: SaveWhen,
    /// The newest history entry was made in this session: the header
    /// offers to undo it.
    pub saved_this_session: bool,
    /// What went wrong on the way, for the status line.
    pub error: Option<SessionError>,
    /// When to save: a moment after the latest edit.
    save_due: Option<Instant>,
    /// The config's revision when its wait to save started or its save
    /// was settled: a new one is a new edit.
    save_revision: Option<u64>,
    /// The config before the latest edit, for saving the edits before it
    /// when niri turns that one down.
    before_latest_edit: Option<Store>,
    /// The config as the latest edit left it: what `before_latest_edit`
    /// becomes at the next one.
    config_now: Option<Store>,
    /// The save being written.
    in_flight: Option<InFlight>,
    /// A save was asked for while another was in flight.
    save_again: bool,
    /// The next save is a display change to try.
    trial_on_save: bool,
    /// Commands for the disk actor, not sent yet.
    commands: Vec<Command>,
}

impl Session {
    /// A session for `platform`. On the desktop saving follows the
    /// preference in the app's config folder.
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            save_when: if platform.writes_files() {
                crate::start::save_when()
            } else {
                SaveWhen::AfterChange
            },
            ..Self::default()
        }
    }

    /// Read the config at `main` and the history.
    pub fn start(&mut self, main: PathBuf) {
        self.commands.push(Command::Start { main });
    }

    /// The commands queued for the disk actor, to send.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    /// Ask the disk actor `command`.
    pub fn ask(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// Files with edits not saved yet.
    pub fn pending(&self) -> usize {
        self.store.as_ref().map_or(0, Store::pending_count)
    }

    /// A save is being written.
    pub const fn saving(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Where the edits stand.
    pub fn save_state(&self) -> SaveState<'_> {
        let files = self.pending();
        if self.health.is_some() && files > 0 {
            return SaveState::WaitingForNiri;
        }
        if let Some(problem) = &self.problem {
            return SaveState::NotSaved(problem);
        }
        if self.saving() {
            return SaveState::Saving;
        }
        match (files, self.save_when) {
            (0, _) => SaveState::Saved,
            (files, SaveWhen::OnRequest) => SaveState::OnRequest { files },
            (_, SaveWhen::AfterChange) => SaveState::Saving,
        }
    }

    /// A file's name as the app shows it, also for a file the config no
    /// longer loads.
    pub fn file_label(&self, path: &Path) -> String {
        match &self.store {
            Some(store) => store.file_label(path),
            None => crate::store::tilde(path),
        }
    }

    /// The config loaded: what is on disk is what is saved.
    pub fn loaded(&mut self, store: Store) {
        self.store = Some(store);
        self.saved.clone_from(&self.store);
        self.config_now.clone_from(&self.store);
        self.forget_edits();
    }

    /// Take a reply of the disk actor. The ones about the History and
    /// Files pages are the pages' business, and come back.
    pub fn reply(&mut self, reply: Reply) -> Option<Reply> {
        match reply {
            Reply::Loaded(Ok(loaded)) => {
                if self.platform.writes_files() {
                    self.read_only = loaded.read_only;
                }
                self.loaded(loaded.store);
                None
            }
            Reply::History(history) => {
                self.history = history;
                None
            }
            Reply::Checked(result) => {
                self.checked_at_start(result);
                None
            }
            Reply::Saved { revision, outcome } => {
                self.saved(revision, outcome);
                None
            }
            Reply::Reloaded(reloaded) => {
                self.reloaded(reloaded);
                None
            }
            Reply::Diagnosed(result) => {
                self.diagnosed(result);
                None
            }
            Reply::Before {
                purpose: Purpose::Restore { label },
                files,
                ..
            } => {
                self.restore(&files, label);
                None
            }
            other @ (Reply::Loaded(Err(_))
            | Reply::Texts { .. }
            | Reply::Before { .. }
            | Reply::Listing { .. }
            | Reply::Backup { .. }
            | Reply::Snapshot { .. }
            | Reply::Failed(_)) => Some(other),
        }
    }

    /// Drop any wait to save and what was typed: the config's text was
    /// replaced underneath.
    fn forget_edits(&mut self) {
        self.problem = None;
        self.save_due = None;
        self.save_revision = None;
        self.drafts.clear();
    }

    // ---- whether niri reads the config ----

    /// niri cannot read the config on disk, while that is so.
    pub const fn health(&self) -> Option<&Health> {
        self.health.as_ref()
    }

    /// niri read the config again after a change on disk, or `failed` to:
    /// what other programs wrote is loaded either way. Saving waits while
    /// niri cannot read it and goes on once it can.
    pub fn niri_reloaded(&mut self, failed: bool) {
        let waited = self.health.take().is_some();
        self.follow_disk();
        let Some(store) = &self.store else {
            return;
        };
        if failed {
            self.health = Some(Health::new(
                "niri could not load your config after a change on disk. It keeps running the last version that loaded.",
                None,
                None,
            ));
            self.commands.push(Command::Diagnose {
                main: store.path().to_path_buf(),
            });
        } else if waited {
            self.rearm();
        }
    }

    /// niri's words on why it cannot read the config, once it gives them.
    pub fn diagnosed(&mut self, result: Result<(), String>) {
        if let (Some(health), Err(words)) = (&self.health, result) {
            self.health = Some(Health::new(
                health.message.clone(),
                Some(words),
                self.store.as_ref(),
            ));
        }
    }

    /// niri's verdict on the config as it was at start: one it cannot read
    /// already is not blamed on the next change.
    pub fn checked_at_start(&mut self, result: Result<(), String>) {
        if let Err(words) = result {
            self.health = Some(Health::new(
                "niri can't read your config as it is now. It keeps running the last version that loaded.",
                Some(words),
                self.store.as_ref(),
            ));
        }
    }

    /// The problem niri had is fixed here, or put aside: what waited can
    /// be saved.
    pub fn health_fixed(&mut self) {
        if self.health.take().is_some() {
            self.rearm();
        }
    }

    // ---- the wait before a save ----

    /// After anything that may have changed the config: when its text is
    /// new, the wait to save (re)starts.
    pub fn edited(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        if self.config_now.as_ref().map(Store::revision) == Some(store.revision()) {
            return;
        }
        self.before_latest_edit = self.config_now.replace(store.clone());
        self.schedule();
    }

    /// When the staged text is new, (re)start the wait before saving.
    fn schedule(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        let revision = store.revision();
        if self.save_revision == Some(revision) {
            return;
        }
        self.save_revision = Some(revision);
        self.problem = None;
        // saved on request only: the header offers Review and Save instead
        self.save_due = (store.has_pending() && self.save_when == SaveWhen::AfterChange)
            .then(|| Instant::now() + SAVE_DELAY);
    }

    /// What waited for something else (niri reading the config again, saving
    /// as changes are made, a save in flight) can be saved now: the wait
    /// starts over.
    pub fn rearm(&mut self) {
        self.save_revision = None;
        self.schedule();
    }

    /// What is staged now had its save: a problem with it stays until the
    /// next edit.
    fn settled(&mut self) {
        self.save_revision = self.store.as_ref().map(Store::revision);
    }

    /// When the wait to save is over, while there is one.
    pub const fn due(&self) -> Option<Instant> {
        self.save_due
    }

    /// End the wait to save now, as if its moment had come.
    pub fn hurry(&mut self) {
        if self.save_due.is_some() {
            self.save_due = Some(Instant::now());
        }
    }

    /// The wait may be over: save if it is.
    pub fn tick(&mut self) {
        if self.save_due.is_some_and(|due| Instant::now() >= due) {
            self.save();
        }
    }

    // ---- saving ----

    /// Save the staged edits now. While niri cannot read the config,
    /// saving waits; while a save is being written, this one follows it.
    pub fn save(&mut self) {
        self.send_save(None);
    }

    /// [`Self::save`], naming the history entry a restore when `restored`
    /// says what was put back.
    fn send_save(&mut self, restored: Option<String>) {
        self.save_due = None;
        // niri cannot read the config: nothing new is written until it can
        if self.health.is_some() && self.platform.writes_files() {
            return;
        }
        if self.in_flight.is_some() {
            self.save_again = true;
            return;
        }
        let Some(store) = &self.store else {
            return;
        };
        let job = store.job();
        if job.changed().next().is_none() {
            self.settled();
            return;
        }
        if let Some(file) = job
            .changed()
            .find(|file| self.read_only.contains(&file.path))
        {
            self.problem = Some(Problem::ReadOnly(store.file_label(&file.path)));
            self.settled();
            return;
        }
        let before = store.saved_version();
        let source = if restored.is_some() {
            Source::Restored
        } else {
            Source::App
        };
        let summary = restored
            .into_iter()
            .chain(describe(&before, store))
            .collect();
        // the edits before the latest, saved instead if niri turns it down
        let base = self
            .before_latest_edit
            .as_ref()
            .filter(|base| base.has_pending());
        let base_summary = base
            .map(|base| describe(&base.saved_version(), base))
            .unwrap_or_default();
        let base = base.map(Store::job);
        self.commands.push(Command::Save(Save {
            job: job.clone(),
            base: base.clone(),
            source,
            summary,
            base_summary,
            checked: false,
        }));
        self.in_flight = Some(InFlight {
            job,
            base,
            before,
            trial: std::mem::take(&mut self.trial_on_save),
        });
        self.settled();
    }

    /// What came of the save made at `revision`.
    fn saved(&mut self, revision: u64, outcome: Outcome) {
        let Some(flight) = self
            .in_flight
            .take_if(|flight| flight.job.revision == revision)
        else {
            return;
        };
        match outcome {
            Outcome::Written { entry, history } => {
                if let Some(store) = &mut self.store {
                    store.saved(&flight.job);
                }
                self.saved = Some(flight.before);
                self.problem = None;
                if let Some(failed) = history {
                    self.error = Some(SessionError::HistoryNotWritten(failed));
                }
                self.remember(entry);
                if flight.trial {
                    self.start_trial();
                }
            }
            Outcome::BaseWritten { entry, diagnostics } => {
                if let (Some(store), Some(base)) = (&mut self.store, &flight.base) {
                    store.saved(base);
                }
                self.saved = Some(flight.before);
                self.remember(entry);
                self.problem = Some(Problem::Rejected(diagnostics));
            }
            Outcome::Refused(refusal) => {
                let label = |path: &Path| self.file_label(path);
                self.problem = Some(match refusal {
                    Refusal::Rejected(words) => Problem::Rejected(words),
                    Refusal::ChangedOnDisk(path) => Problem::ChangedOnDisk(label(&path)),
                    Refusal::Unwritable(path) => Problem::ReadOnly(label(&path)),
                    Refusal::Failed(why) => Problem::Failed(why),
                });
            }
        }
        // edits made while it was written get a save of their own
        if std::mem::take(&mut self.save_again) {
            self.save();
        } else if self.problem.is_none() {
            self.rearm();
        }
    }

    fn remember(&mut self, entry: Entry) {
        self.saved_this_session = true;
        self.history.push(entry);
        let excess = self.history.len().saturating_sub(crate::history::KEEP);
        self.history.drain(..excess);
    }

    /// The staged edits as a unified diff, to apply by hand (a read-only
    /// config, a config built by Nix).
    pub fn pending_patch(&self) -> String {
        self.store.as_ref().map_or_else(String::new, |store| {
            store
                .pending_diffs()
                .iter()
                .map(diff::render_unified)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// A change's files as a unified diff.
    pub fn patch(&self, files: &[FileChange]) -> String {
        files
            .iter()
            .map(|file| {
                diff::render_unified(&diff::FileDiff {
                    path: self.file_label(&file.path),
                    hunks: diff::diff(&file.before, &file.after),
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ---- going back ----

    /// The header's Undo: drop edits not saved yet, else undo the latest
    /// change in the history.
    pub fn undo_latest(&mut self) {
        let Some(store) = &mut self.store else {
            return;
        };
        if store.has_pending() {
            store.revert_pending();
            self.forget_edits();
            return;
        }
        if let Some(latest) = self.history.last() {
            let time = latest.time;
            self.go_back(time, None);
        }
    }

    /// Write the config back as it was at `version`, as one new change that
    /// Undo takes back. Nothing happens for the current version.
    pub fn restore_version(&mut self, version: Version) {
        let today = today();
        let (back_to, label) = match version {
            Version::Start => {
                let Some(oldest) = self.history.first() else {
                    return;
                };
                (
                    oldest.time,
                    format!(
                        "Restored the config from before {}",
                        moment(oldest.time, today)
                    ),
                )
            }
            Version::After(time) => {
                let Some(next) = self.history.iter().find(|entry| entry.time > time) else {
                    return;
                };
                (
                    next.time,
                    format!("Restored the version from {}", moment(time, today)),
                )
            }
        };
        self.go_back(back_to, Some(label));
    }

    /// Put every file back the way it was before the change at `time`,
    /// undoing it and everything after it, as one new change.
    fn go_back(&mut self, time: u128, label: Option<String>) {
        self.commands.push(Command::Before {
            time,
            purpose: Purpose::Restore { label },
        });
    }

    /// The files as they were before a change came back: stage them and
    /// save at once.
    fn restore(&mut self, files: &[(PathBuf, String)], restored: Option<String>) {
        let Some(store) = &mut self.store else {
            return;
        };
        for (path, text) in files {
            if let Err(e) = store.stage_file(path, text) {
                self.error = Some(SessionError::CouldNotGoBack(e));
                return;
            }
        }
        self.forget_edits();
        self.send_save(restored);
    }

    // ---- what other programs write ----

    /// Ask the disk actor what other programs wrote. Waits while the app
    /// has edits of its own not saved yet: saving them reports the clash.
    fn follow_disk(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        if store.has_pending() {
            return;
        }
        self.commands.push(Command::Reload {
            main: store.path().to_path_buf(),
            saved: store
                .saved_texts()
                .map(|(path, text)| (path.to_path_buf(), text.to_string()))
                .collect(),
        });
    }

    /// What other programs wrote, loaded anew. Edits made here since the
    /// question was asked are not replaced: their save reports the clash.
    fn reloaded(&mut self, reloaded: Option<(crate::disk::Loaded, Entry)>) {
        let Some((loaded, entry)) = reloaded else {
            return;
        };
        if self.store.as_ref().is_some_and(Store::has_pending) {
            return;
        }
        if self.platform.writes_files() {
            self.read_only = loaded.read_only;
        }
        self.saved = self.store.replace(loaded.store);
        self.config_now.clone_from(&self.store);
        self.forget_edits();
        self.history.push(entry);
    }

    // ---- display changes on trial ----

    /// Make a display change with `edit` and put it on trial. Edits waiting
    /// for their save are saved first, so the trial covers this change
    /// alone. A picked value is saved and tried at once. A `typed` one is
    /// tried when its save comes, after the typing stops.
    pub fn change_display(
        &mut self,
        typed: bool,
        edit: impl FnOnce(&mut Store, &mut Drafts) -> Result<(), String>,
    ) -> Result<(), String> {
        if !typed {
            self.save();
        }
        let Some(store) = &mut self.store else {
            return Ok(());
        };
        edit(store, &mut self.drafts)?;
        self.trial_on_save = true;
        if !typed {
            self.save();
        }
        Ok(())
    }

    /// Put the change just saved on trial, or add it to the running trial,
    /// which starts its countdown over.
    fn start_trial(&mut self) {
        let Some(entry) = self.history.last() else {
            return;
        };
        let what = entry.summary.first().cloned().unwrap_or_default();
        let back_to = self
            .trial
            .as_ref()
            .map_or(entry.time, |trial| trial.back_to);
        self.trial = Some(Trial {
            back_to,
            what,
            seconds_left: TRIAL_SECONDS,
        });
    }

    /// Count the trial down by a second. At zero the change goes back.
    pub fn trial_tick(&mut self) {
        if let Some(trial) = &mut self.trial {
            trial.seconds_left = trial.seconds_left.saturating_sub(1);
            if trial.seconds_left == 0 {
                self.end_trial();
            }
        }
    }

    /// Keep the display change on trial.
    pub fn keep_trial(&mut self) {
        self.trial = None;
    }

    /// Take the display change on trial back.
    pub fn end_trial(&mut self) {
        if let Some(trial) = self.trial.take() {
            self.go_back(trial.back_to, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::disk::actor::Actor;
    use crate::disk::{Disk, Folders};

    /// Send the session's commands to the actor and its replies back,
    /// until neither has more to say.
    fn drain(session: &mut Session, actor: &mut Actor) -> Vec<Reply> {
        let mut left = Vec::new();
        loop {
            let commands = session.take_commands();
            if commands.is_empty() {
                return left;
            }
            for command in commands {
                for reply in actor.handle(command) {
                    left.extend(session.reply(reply));
                }
            }
        }
    }

    /// A session on the config `text`, in a folder of its own on disk.
    fn on_disk(text: &str) -> (tempfile::TempDir, Session, Actor) {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("config.kdl");
        fs::write(&main, text).unwrap();
        let disk = Disk::files(Folders::under(dir.path()));
        let mut actor = Actor::new(disk, Some(dir.path().join("history")));
        let mut session = Session::new(Platform::InMemory);
        session.start(main);
        drain(&mut session, &mut actor);
        (dir, session, actor)
    }

    fn set_gaps(session: &mut Session, gaps: f64) {
        session
            .store
            .as_mut()
            .unwrap()
            .set_layout_gaps(gaps)
            .unwrap();
        session.edited();
    }

    #[test]
    fn an_edit_made_while_a_save_is_written_stays_for_its_own_save() {
        // a save on disk asks niri first
        if !crate::validate::niri_available() {
            return;
        }
        let (dir, mut session, mut actor) = on_disk("layout {\n    gaps 8\n}\n");
        set_gaps(&mut session, 16.0);
        session.save();
        // the actor has not answered yet: another edit comes in
        set_gaps(&mut session, 24.0);
        let commands = session.take_commands();
        assert_eq!(commands.len(), 1, "one save in flight");
        for command in commands {
            for reply in actor.handle(command) {
                session.reply(reply);
            }
        }
        let main = dir.path().join("config.kdl");
        assert_eq!(
            fs::read_to_string(&main).unwrap(),
            "layout {\n    gaps 16\n}\n"
        );
        assert_eq!(session.pending(), 1, "the later edit is not saved yet");
        session.save();
        drain(&mut session, &mut actor);
        assert_eq!(
            fs::read_to_string(&main).unwrap(),
            "layout {\n    gaps 24\n}\n"
        );
        assert_eq!(session.pending(), 0);
        assert_eq!(session.history.len(), 2);
    }

    #[test]
    fn a_file_another_program_wrote_is_not_written_over() {
        let (dir, mut session, mut actor) = on_disk("layout {\n    gaps 8\n}\n");
        let main = dir.path().join("config.kdl");
        fs::write(&main, "layout {\n    gaps 10\n}\n").unwrap();
        set_gaps(&mut session, 16.0);
        session.save();
        drain(&mut session, &mut actor);
        assert!(matches!(session.problem, Some(Problem::ChangedOnDisk(_))));
        assert_eq!(
            fs::read_to_string(&main).unwrap(),
            "layout {\n    gaps 10\n}\n"
        );
        assert_eq!(session.pending(), 1, "the edit is kept");
    }

    #[test]
    fn an_edit_niri_turns_down_leaves_the_ones_before_it_saved() {
        if !crate::validate::niri_available() {
            return;
        }
        let (dir, mut session, mut actor) = on_disk("layout {\n    gaps 8\n}\n");
        set_gaps(&mut session, 16.0);
        let main = dir.path().join("config.kdl");
        session
            .store
            .as_mut()
            .unwrap()
            .stage_file(&main, "layout {\n    gaps 16\n    gapz 3\n}\n")
            .unwrap();
        session.edited();
        session.save();
        drain(&mut session, &mut actor);
        assert!(matches!(session.problem, Some(Problem::Rejected(_))));
        assert_eq!(
            fs::read_to_string(&main).unwrap(),
            "layout {\n    gaps 16\n}\n"
        );
        assert_eq!(session.pending(), 1, "only the turned-down edit is left");
    }

    #[test]
    fn undo_writes_back_the_text_before_the_latest_change() {
        // a save on disk asks niri first
        if !crate::validate::niri_available() {
            return;
        }
        let (dir, mut session, mut actor) = on_disk("layout {\n    gaps 8\n}\n");
        set_gaps(&mut session, 16.0);
        session.save();
        drain(&mut session, &mut actor);
        session.undo_latest();
        drain(&mut session, &mut actor);
        let main = dir.path().join("config.kdl");
        assert_eq!(
            fs::read_to_string(&main).unwrap(),
            "layout {\n    gaps 8\n}\n"
        );
        assert_eq!(session.history.len(), 2, "the undo is a change too");
    }
}
