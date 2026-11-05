//! The disk actor: the one owner of the config files on disk and of the
//! history of changes to them.
//!
//! It is plain state. [`Actor::handle`] answers one [`Command`] with the
//! [`Reply`]s it makes, and [`Actor::run`] does that for every command until
//! all senders are gone, then hands the state back. Commands are worked
//! through in the order they come, so a save never overtakes another.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{Disk, DiskError, Loaded, SaveJob};
use crate::history::{self, Entry, FileChange, Source};
use crate::session::SaveWhen;
use crate::store::Store;
use crate::summary::describe;
use crate::validate::ReadsError;

/// What the disk actor is asked to do.
#[derive(Debug, Clone)]
pub enum Command {
    /// Read the config at `main` and the history, record what other
    /// programs changed while the app was closed, and ask niri whether it
    /// reads the config as it is.
    Start {
        main: PathBuf,
    },
    /// Write a save.
    Save(Save),
    /// niri read the config again: what other programs wrote since
    /// `saved`, each loaded file with the text the app last saw on disk.
    Reload {
        main: PathBuf,
        saved: Vec<(PathBuf, String)>,
    },
    /// niri's words on the config on disk.
    Diagnose {
        main: PathBuf,
    },
    /// The texts of the change made at `time`.
    Texts {
        time: u128,
        purpose: Purpose,
    },
    /// Every file the change at `time` or a later one touched, as it was
    /// before them.
    Before {
        time: u128,
        purpose: Purpose,
    },
    /// The backups and snapshots there are.
    Listing,
    /// The newest backup of the file named `name`.
    Backup {
        name: String,
    },
    CreateSnapshot {
        name: String,
        files: Vec<(String, String)>,
    },
    Snapshot {
        name: String,
    },
    DeleteSnapshot {
        name: String,
    },
    /// Keep when edits are saved, for the next start.
    KeepSaveWhen(SaveWhen),
}

/// A save: the edits to write, the edits before the latest of them to
/// write instead when niri turns the latest down, and how the history
/// names each.
#[derive(Debug, Clone)]
pub struct Save {
    pub job: SaveJob,
    pub base: Option<SaveJob>,
    pub source: Source,
    pub summary: Vec<String>,
    pub base_summary: Vec<String>,
    /// niri accepted the job's texts already.
    pub checked: bool,
}

/// What texts are asked for: the asker's own word for it, handed back
/// with the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// To show the change on the History page.
    Show,
    /// To copy the change as a diff.
    Copy,
    /// To write the files back, named in the history by `label` (a
    /// restore), or as an undo.
    Restore { label: Option<String> },
    /// To show what restoring would change.
    Preview,
}

/// A [`DiskError`] as replies carry it. Replies are `Clone` and an
/// `io::Error` or a [`crate::store::StoreError`] is not, so those are their
/// text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiskFailure {
    #[error("failed to read {path}: {reason}")]
    Read { path: PathBuf, reason: String },
    #[error("could not write {path}: {reason}")]
    Write { path: PathBuf, reason: String },
    #[error("{path} changed on disk since it was loaded")]
    ChangedOnDisk { path: PathBuf },
    #[error("config failed `niri validate`:\n{diagnostics}")]
    Rejected { diagnostics: String },
    #[error("niri validate could not run: {0}")]
    Validator(String),
    #[error("{0}")]
    Store(String),
    #[error("no backup of {name}")]
    NoBackup { name: String },
    #[error("snapshot {name}: {reason}")]
    Snapshot { name: String, reason: String },
    #[error("The preference could not be kept: {reason}")]
    Preference { reason: String },
    /// Why the history could not keep a change.
    #[error("{0}")]
    History(String),
}

impl From<DiskError> for DiskFailure {
    fn from(error: DiskError) -> Self {
        match error {
            DiskError::Read { path, source } => Self::Read {
                path,
                reason: source.to_string(),
            },
            DiskError::Write { path, source } => Self::Write {
                path,
                reason: source.to_string(),
            },
            DiskError::ChangedOnDisk { path } => Self::ChangedOnDisk { path },
            DiskError::Rejected { diagnostics } => Self::Rejected { diagnostics },
            DiskError::Validator(why) => Self::Validator(why),
            DiskError::Store(source) => Self::Store(source.to_string()),
            DiskError::NoBackup { name } => Self::NoBackup { name },
            DiskError::Snapshot { name, source } => Self::Snapshot {
                name,
                reason: source.to_string(),
            },
            DiskError::Preference { source } => Self::Preference {
                reason: source.to_string(),
            },
        }
    }
}

/// What the disk actor answers.
#[derive(Debug, Clone)]
pub enum Reply {
    Loaded(Result<Loaded, DiskFailure>),
    /// The history, oldest first, with what other programs changed while
    /// the app was closed as its newest entry.
    History(Vec<Entry>),
    /// Whether niri reads the config as it was at start.
    Checked(Result<(), ReadsError>),
    /// What became of the save made at `revision`.
    Saved {
        revision: u64,
        outcome: Outcome,
    },
    /// What other programs wrote, loaded anew, and its history entry.
    Reloaded(Option<(Loaded, Entry)>),
    Diagnosed(Result<(), ReadsError>),
    Texts {
        time: u128,
        purpose: Purpose,
        files: Vec<FileChange>,
    },
    Before {
        time: u128,
        purpose: Purpose,
        files: Vec<(PathBuf, String)>,
    },
    Listing {
        backups: Vec<(String, u128)>,
        snapshots: Vec<String>,
    },
    Backup {
        name: String,
        text: Result<String, DiskFailure>,
    },
    Snapshot {
        name: String,
        files: Result<Vec<(String, String)>, DiskFailure>,
    },
    /// A snapshot could not be made or deleted, or a preference kept.
    Failed(DiskFailure),
}

/// What became of a save.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Written, and recorded as `entry`. `history`: why the entry could
    /// not be kept on disk, if it could not.
    Written {
        entry: Entry,
        history: Option<String>,
    },
    /// niri turned the latest edits down in these words; the edits before
    /// them were written, as `entry`.
    BaseWritten { entry: Entry, diagnostics: String },
    /// Nothing was written.
    Refused(Refusal),
}

/// Why nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// niri turned the edits down: its words.
    Rejected(String),
    /// Another program wrote this file since the app read it.
    ChangedOnDisk(PathBuf),
    /// This file cannot be written here.
    Unwritable(PathBuf),
    Failed(DiskFailure),
}

impl From<DiskError> for Refusal {
    fn from(error: DiskError) -> Self {
        match error {
            DiskError::Rejected { diagnostics } => Self::Rejected(diagnostics),
            DiskError::ChangedOnDisk { path } => Self::ChangedOnDisk(path),
            DiskError::Write { path, .. } => Self::Unwritable(path),
            other @ (DiskError::Read { .. }
            | DiskError::Validator(_)
            | DiskError::Store(_)
            | DiskError::NoBackup { .. }
            | DiskError::Snapshot { .. }
            | DiskError::Preference { .. }) => Self::Failed(other.into()),
        }
    }
}

/// The disk and the history of changes to it.
#[derive(Debug, Clone)]
pub struct Actor {
    disk: Disk,
    /// Where the history is kept; `None` keeps it in memory.
    history_dir: Option<PathBuf>,
    history: Vec<Entry>,
    /// The texts of entries kept in memory, by time.
    texts: BTreeMap<u128, Vec<FileChange>>,
}

impl Actor {
    pub const fn new(disk: Disk, history_dir: Option<PathBuf>) -> Self {
        Self {
            disk,
            history_dir,
            history: Vec::new(),
            texts: BTreeMap::new(),
        }
    }

    /// Answer every command until all senders are gone, then hand the
    /// state back; `None` when handling a command panicked. The work
    /// blocks (files, `niri validate`), so on the desktop each command runs
    /// on tokio's blocking pool, the state moving there and back.
    pub async fn run(
        self,
        mut commands: UnboundedReceiver<Command>,
        replies: UnboundedSender<Reply>,
    ) -> Option<Self> {
        let mut actor = self;
        while let Some(command) = commands.recv().await {
            #[cfg(not(target_arch = "wasm32"))]
            let (moved, out) = tokio::task::spawn_blocking(move || {
                let out = actor.handle(command);
                (actor, out)
            })
            .await
            .ok()?;
            #[cfg(target_arch = "wasm32")]
            let (moved, out) = {
                let out = actor.handle(command);
                (actor, out)
            };
            actor = moved;
            for reply in out {
                if replies.send(reply).is_err() {
                    return Some(actor);
                }
            }
        }
        Some(actor)
    }

    /// Answer one command.
    pub fn handle(&mut self, command: Command) -> Vec<Reply> {
        match command {
            Command::Start { main } => self.start(&main),
            Command::Save(save) => vec![self.save(save)],
            Command::Reload { main, saved } => {
                let (reloaded, unrecorded) = self.reload(&main, &saved).unzip();
                let mut replies = vec![Reply::Reloaded(reloaded)];
                replies.extend(
                    unrecorded
                        .flatten()
                        .map(DiskFailure::History)
                        .map(Reply::Failed),
                );
                replies
            }
            Command::Diagnose { main } => vec![Reply::Diagnosed(self.diagnose(&main))],
            Command::Texts { time, purpose } => vec![Reply::Texts {
                time,
                purpose,
                files: self.texts(time),
            }],
            Command::Before { time, purpose } => vec![Reply::Before {
                time,
                purpose,
                files: self.before(time),
            }],
            Command::Listing => vec![self.listing()],
            Command::Backup { name } => {
                let text = self.disk.backup(&name).map_err(DiskFailure::from);
                vec![Reply::Backup { name, text }]
            }
            Command::CreateSnapshot { name, files } => {
                match self.disk.create_snapshot(&name, files) {
                    Ok(()) => vec![self.listing()],
                    Err(error) => vec![Reply::Failed(error.into()), self.listing()],
                }
            }
            Command::Snapshot { name } => {
                let files = self.disk.snapshot(&name).map_err(DiskFailure::from);
                vec![Reply::Snapshot { name, files }]
            }
            Command::DeleteSnapshot { name } => match self.disk.delete_snapshot(&name) {
                Ok(()) => vec![self.listing()],
                Err(error) => vec![Reply::Failed(error.into()), self.listing()],
            },
            Command::KeepSaveWhen(when) => match self.disk.keep_save_when(when) {
                Ok(()) => Vec::new(),
                Err(error) => vec![Reply::Failed(error.into())],
            },
        }
    }

    fn start(&mut self, main: &Path) -> Vec<Reply> {
        let loaded = match self.disk.load(main) {
            Ok(loaded) => loaded,
            Err(error) => return vec![Reply::Loaded(Err(error.into()))],
        };
        if let Some(dir) = &self.history_dir {
            self.history = history::load(dir);
        }
        let unrecorded = self.catch_up(&loaded.store);
        let checked = self.disk.check_on_disk(main);
        let mut replies = vec![
            Reply::Loaded(Ok(loaded)),
            Reply::History(self.history.clone()),
            Reply::Checked(checked),
        ];
        replies.extend(unrecorded.map(DiskFailure::History).map(Reply::Failed));
        replies
    }

    /// Files that differ from what the history last saw were edited while
    /// the app was closed: record that, so it can be undone too. Says why
    /// when the history could not keep it.
    fn catch_up(&mut self, store: &Store) -> Option<String> {
        let mut files = Vec::new();
        for (path, text) in store.saved_texts() {
            // the newest entry that touched the file: only its texts are read
            let Some(newest) = self
                .history
                .iter()
                .rev()
                .find(|entry| entry.paths.iter().any(|touched| touched == path))
                .map(|entry| entry.time)
            else {
                continue;
            };
            let last = self
                .texts(newest)
                .into_iter()
                .find(|file| file.path == path);
            if let Some(last) = last
                && last.after != text
            {
                files.push(FileChange {
                    path: path.to_path_buf(),
                    before: last.after,
                    after: text.to_string(),
                });
            }
        }
        if files.is_empty() {
            return None;
        }
        // the store as the history last saw it, to say what changed
        let mut before = store.clone();
        for file in &files {
            if before.stage_file(&file.path, &file.before).is_err() {
                // a file it no longer includes has nothing to say
            }
        }
        let summary = describe(&before, store);
        let (_, unrecorded) = self.record(Source::Outside, summary, files);
        unrecorded
    }

    fn save(&mut self, save: Save) -> Reply {
        let Save {
            job,
            base,
            source,
            summary,
            base_summary,
            checked,
        } = save;
        let revision = job.revision;
        let outcome = match self.disk.save(&job, checked) {
            Ok(()) => self.written(source, summary, &job),
            Err(DiskError::Rejected { diagnostics }) => match base {
                // the edits before the latest, which niri may accept
                Some(base) => match self.disk.save(&base, false) {
                    Ok(()) => {
                        let entry = self.record(Source::App, base_summary, changed(&base)).0;
                        Outcome::BaseWritten { entry, diagnostics }
                    }
                    Err(_) => Outcome::Refused(Refusal::Rejected(diagnostics)),
                },
                None => Outcome::Refused(Refusal::Rejected(diagnostics)),
            },
            Err(other) => Outcome::Refused(other.into()),
        };
        Reply::Saved { revision, outcome }
    }

    fn written(&mut self, source: Source, summary: Vec<String>, job: &SaveJob) -> Outcome {
        let (entry, history) = self.record(source, summary, changed(job));
        Outcome::Written { entry, history }
    }

    /// Keep a new entry: on disk when the history lives there, in memory
    /// always. Gives the entry and why it could not be kept on disk.
    fn record(
        &mut self,
        source: Source,
        summary: Vec<String>,
        files: Vec<FileChange>,
    ) -> (Entry, Option<String>) {
        // a change is known by its time: two in one millisecond move apart
        let time = self
            .history
            .last()
            .map_or_else(history::now, |last| history::now().max(last.time + 1));
        let entry = Entry::new(time, source, summary, &files);
        let mut failed = None;
        match &self.history_dir {
            Some(dir) => {
                if let Err(error) = history::record(dir, &entry, &files) {
                    failed = Some(error.to_string());
                }
                history::prune(dir, history::KEEP);
            }
            None => {
                self.texts.insert(time, files);
            }
        }
        self.history.push(entry.clone());
        let excess = self.history.len().saturating_sub(history::KEEP);
        for gone in self.history.drain(..excess) {
            self.texts.remove(&gone.time);
        }
        (entry, failed)
    }

    /// The texts of the change made at `time`; none when they cannot be
    /// read any more.
    fn texts(&self, time: u128) -> Vec<FileChange> {
        if let Some(files) = self.texts.get(&time) {
            return files.clone();
        }
        let entry = self.history.iter().find(|entry| entry.time == time);
        self.history_dir
            .as_deref()
            .zip(entry)
            .and_then(|(dir, entry)| history::read_texts(dir, entry))
            .unwrap_or_default()
    }

    /// Every file the change at `time` or a later one touched, as it was
    /// before them: the earliest change to a file holds that, and the
    /// texts of later ones are not read.
    fn before(&self, time: u128) -> Vec<(PathBuf, String)> {
        let mut files: Vec<(PathBuf, String)> = Vec::new();
        for entry in self.history.iter().filter(|entry| entry.time >= time) {
            if entry
                .paths
                .iter()
                .all(|path| files.iter().any(|(known, _)| known == path))
            {
                continue;
            }
            for file in self.texts(entry.time) {
                if !files.iter().any(|(path, _)| *path == file.path) {
                    files.push((file.path, file.before));
                }
            }
        }
        files
    }

    /// Load what other programs wrote since `saved`, and record it: the
    /// config anew with its history entry, and why the history could not
    /// keep it when it could not.
    fn reload(
        &mut self,
        main: &Path,
        saved: &[(PathBuf, String)],
    ) -> Option<((Loaded, Entry), Option<String>)> {
        let changed = self.disk.changed(saved);
        if changed.is_empty() {
            return None;
        }
        // a file that no longer reads is the broken-config panel's business
        let fresh = self.disk.load(main).ok()?;
        let old = Store::read(main.to_path_buf(), |path| {
            saved
                .iter()
                .find(|(known, _)| known == path)
                .map(|(_, text)| text.clone())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        })
        .ok()?;
        let summary = describe(&old, &fresh.store);
        let (entry, unrecorded) = self.record(Source::Outside, summary, changed);
        Some(((fresh, entry), unrecorded))
    }

    fn diagnose(&self, main: &Path) -> Result<(), ReadsError> {
        self.disk.check_on_disk(main)
    }

    fn listing(&self) -> Reply {
        Reply::Listing {
            backups: self.disk.latest_backups(),
            snapshots: self.disk.snapshots(),
        }
    }
}

/// The files `job` changes, with their texts before and after.
fn changed(job: &SaveJob) -> Vec<FileChange> {
    job.changed().cloned().collect()
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::store::StoreError;

    fn denied() -> io::Error {
        io::Error::other("denied")
    }

    #[test]
    fn a_failure_reads_as_the_disk_error_it_came_from() {
        let path = || PathBuf::from("/niri/a.kdl");
        for (error, said) in [
            (
                DiskError::Read {
                    path: path(),
                    source: denied(),
                },
                "failed to read /niri/a.kdl: denied",
            ),
            (
                DiskError::Write {
                    path: path(),
                    source: denied(),
                },
                "could not write /niri/a.kdl: denied",
            ),
            (
                DiskError::ChangedOnDisk { path: path() },
                "/niri/a.kdl changed on disk since it was loaded",
            ),
            (
                DiskError::Rejected {
                    diagnostics: "unexpected node".into(),
                },
                "config failed `niri validate`:\nunexpected node",
            ),
            (
                DiskError::Validator("no niri".into()),
                "niri validate could not run: no niri",
            ),
            (
                DiskError::Store(StoreError::NoBinds),
                "no `binds` block in the loaded config set",
            ),
            (
                DiskError::NoBackup {
                    name: "a.kdl".into(),
                },
                "no backup of a.kdl",
            ),
            (
                DiskError::Snapshot {
                    name: "before".into(),
                    source: denied(),
                },
                "snapshot before: denied",
            ),
            (
                DiskError::Preference { source: denied() },
                "The preference could not be kept: denied",
            ),
        ] {
            assert_eq!(error.to_string(), said);
            assert_eq!(DiskFailure::from(error).to_string(), said);
        }
    }

    #[test]
    fn a_refusal_keeps_only_what_it_has_no_case_for_as_a_failure() {
        assert_eq!(
            Refusal::from(DiskError::Validator("no niri".into())),
            Refusal::Failed(DiskFailure::Validator("no niri".into()))
        );
        assert_eq!(
            Refusal::from(DiskError::ChangedOnDisk {
                path: "/niri/a.kdl".into()
            }),
            Refusal::ChangedOnDisk("/niri/a.kdl".into())
        );
    }

    #[test]
    fn what_cannot_be_found_is_answered_with_its_name() {
        let mut actor = Actor::new(Disk::memory([]), None);
        let replies = actor.handle(Command::Backup {
            name: "a.kdl".into(),
        });
        let [Reply::Backup { text: Err(why), .. }] = replies.as_slice() else {
            panic!("{replies:?}");
        };
        assert_eq!(why.to_string(), "no backup of a.kdl");
        let replies = actor.handle(Command::Snapshot {
            name: "before".into(),
        });
        let [
            Reply::Snapshot {
                files: Err(why), ..
            },
        ] = replies.as_slice()
        else {
            panic!("{replies:?}");
        };
        assert!(why.to_string().starts_with("snapshot before: "), "{why}");
    }

    #[test]
    fn a_history_that_cannot_keep_a_reload_says_why_in_the_ios_words() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("history");
        std::fs::write(&blocked, "a file where the folder goes").unwrap();
        let main = PathBuf::from("/niri/config.kdl");
        let on_disk = [(main.clone(), "layout {\n    gaps 9\n}\n".to_string())];
        let mut actor = Actor::new(Disk::memory(on_disk), Some(blocked));
        let replies = actor.handle(Command::Reload {
            main: main.clone(),
            saved: vec![(main, "layout {\n    gaps 8\n}\n".to_string())],
        });
        let [
            Reply::Reloaded(Some(_)),
            Reply::Failed(why @ DiskFailure::History(reason)),
        ] = replies.as_slice()
        else {
            panic!("{replies:?}");
        };
        assert!(!reason.is_empty());
        assert_eq!(why.to_string(), *reason);
    }

    #[test]
    fn a_history_that_cannot_keep_what_changed_while_closed_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let history_dir = dir.path().join("history");
        let main = PathBuf::from("/niri/config.kdl");
        let files = [FileChange {
            path: main.clone(),
            before: String::new(),
            after: "layout {\n    gaps 8\n}\n".to_string(),
        }];
        let entry = Entry::new(1, Source::App, Vec::new(), &files);
        history::record(&history_dir, &entry, &files).unwrap();
        let locked = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(&history_dir, locked).unwrap();
        let on_disk = [(main.clone(), "layout {\n    gaps 9\n}\n".to_string())];
        let mut actor = Actor::new(Disk::memory(on_disk), Some(history_dir.clone()));
        // root writes where the mode says no: nothing fails to report
        let root = std::fs::write(history_dir.join("probe"), "").is_ok();
        let replies = actor.handle(Command::Start { main });
        let open = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&history_dir, open).unwrap();
        if root {
            return;
        }
        let [
            Reply::Loaded(Ok(_)),
            Reply::History(_),
            Reply::Checked(Ok(())),
            Reply::Failed(why @ DiskFailure::History(reason)),
        ] = replies.as_slice()
        else {
            panic!("{replies:?}");
        };
        assert!(!reason.is_empty());
        assert_eq!(why.to_string(), *reason);
    }

    #[test]
    fn a_config_that_cannot_be_loaded_says_why() {
        let main = PathBuf::from("/niri/config.kdl");
        let mut actor = Actor::new(Disk::memory([]), None);
        let replies = actor.handle(Command::Start { main: main.clone() });
        let [Reply::Loaded(Err(DiskFailure::Read { path, .. }))] = replies.as_slice() else {
            panic!("{replies:?}");
        };
        assert_eq!(*path, main);
        let broken = [(main.clone(), "layout {".to_string())];
        let mut actor = Actor::new(Disk::memory(broken), None);
        let replies = actor.handle(Command::Start { main });
        let [Reply::Loaded(Err(why @ DiskFailure::Store(_)))] = replies.as_slice() else {
            panic!("{replies:?}");
        };
        assert!(
            why.to_string().starts_with("failed to parse config: "),
            "{why}"
        );
    }
}
