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
use crate::store::Store;
use crate::summary::describe;

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

/// What the disk actor answers.
#[derive(Debug, Clone)]
pub enum Reply {
    Loaded(Result<Loaded, String>),
    /// The history, oldest first, with what other programs changed while
    /// the app was closed as its newest entry.
    History(Vec<Entry>),
    /// Whether niri reads the config as it was at start.
    Checked(Result<(), String>),
    /// What became of the save made at `revision`.
    Saved {
        revision: u64,
        outcome: Outcome,
    },
    /// What other programs wrote, loaded anew, and its history entry.
    Reloaded(Option<(Loaded, Entry)>),
    Diagnosed(Result<(), String>),
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
        text: Result<String, String>,
    },
    Snapshot {
        name: String,
        files: Result<Vec<(String, String)>, String>,
    },
    /// A snapshot could not be made or deleted.
    Failed(String),
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
    Failed(String),
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
            | DiskError::Snapshot { .. }) => Self::Failed(other.to_string()),
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
            Command::Reload { main, saved } => vec![Reply::Reloaded(self.reload(&main, &saved))],
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
                let text = self.disk.backup(&name).map_err(|e| e.to_string());
                vec![Reply::Backup { name, text }]
            }
            Command::CreateSnapshot { name, files } => {
                match self.disk.create_snapshot(&name, files) {
                    Ok(()) => vec![self.listing()],
                    Err(e) => vec![Reply::Failed(e.to_string()), self.listing()],
                }
            }
            Command::Snapshot { name } => {
                let files = self.disk.snapshot(&name).map_err(|e| e.to_string());
                vec![Reply::Snapshot { name, files }]
            }
            Command::DeleteSnapshot { name } => match self.disk.delete_snapshot(&name) {
                Ok(()) => vec![self.listing()],
                Err(e) => vec![Reply::Failed(e.to_string()), self.listing()],
            },
        }
    }

    fn start(&mut self, main: &Path) -> Vec<Reply> {
        let loaded = match self.disk.load(main) {
            Ok(loaded) => loaded,
            Err(e) => return vec![Reply::Loaded(Err(e.to_string()))],
        };
        if let Some(dir) = &self.history_dir {
            self.history = history::load(dir);
        }
        self.catch_up(&loaded.store);
        let checked = self.disk.check_on_disk(main);
        vec![
            Reply::Loaded(Ok(loaded)),
            Reply::History(self.history.clone()),
            Reply::Checked(checked),
        ]
    }

    /// Files that differ from what the history last saw were edited while
    /// the app was closed: record that, so it can be undone too.
    fn catch_up(&mut self, store: &Store) {
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
            return;
        }
        let mut before = store.clone();
        for file in &files {
            let _ = before.stage_file(&file.path, &file.before);
        }
        let summary = describe(&before, store);
        let _ = self.record(Source::Outside, summary, files);
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
                if let Err(e) = history::record(dir, &entry, &files) {
                    failed = Some(e.to_string());
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

    /// Load what other programs wrote since `saved`, and record it.
    fn reload(&mut self, main: &Path, saved: &[(PathBuf, String)]) -> Option<(Loaded, Entry)> {
        let changed = self.disk.changed(
            saved
                .iter()
                .map(|(path, text)| (path.as_path(), text.as_str())),
        );
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
        let (entry, _) = self.record(Source::Outside, summary, changed);
        Some((fresh, entry))
    }

    fn diagnose(&self, main: &Path) -> Result<(), String> {
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
