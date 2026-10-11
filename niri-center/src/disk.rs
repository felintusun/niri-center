//! The disk: the one part of the library that reads and writes config files.
//!
//! A [`Store`] holds documents and edits only. Loading reads the files into
//! one, and saving takes a [`SaveJob`]: every loaded file with the text the
//! store believes is on disk and the text to write. A job is written whole
//! or not at all, and never over text it did not expect.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::history::FileChange;
use crate::store::{Store, StoreError};
use crate::xdg::Places;
use crate::{backup, snapshot, validate};

pub mod actor;

/// How many backups to keep (newest win).
const KEEP_BACKUPS: usize = 20;

/// What a save writes: every loaded file, `before` the text the store
/// believes is on disk, `after` the text to write. Files whose two texts
/// are the same are there for `niri validate`, which reads the whole set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveJob {
    /// The store's revision when the job was made.
    pub revision: u64,
    pub main: PathBuf,
    pub files: Vec<FileChange>,
}

impl SaveJob {
    /// The files the job changes.
    pub fn changed(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|file| file.before != file.after)
    }
}

/// What went wrong reading or writing.
#[derive(Debug, thiserror::Error)]
pub enum DiskError {
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("could not write {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("{path} changed on disk since it was loaded")]
    ChangedOnDisk { path: PathBuf },
    #[error("config failed `niri validate`:\n{diagnostics}")]
    Rejected { diagnostics: String },
    #[error("niri validate could not run: {0}")]
    Validator(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("no backup of {name}")]
    NoBackup { name: String },
    #[error("snapshot {name}: {source}")]
    Snapshot { name: String, source: io::Error },
}

/// A config read from disk, and the files in it the app cannot write.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub store: Store,
    pub read_only: Vec<PathBuf>,
}

/// Where backups, snapshots and validation probes go on the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    pub backups: PathBuf,
    pub snapshots: PathBuf,
    pub probes: PathBuf,
}

impl Folders {
    /// The app's own, where XDG puts them.
    pub fn of(places: &Places) -> Self {
        Self {
            backups: places.backups.clone(),
            snapshots: places.snapshots.clone(),
            probes: places.probes.clone(),
        }
    }

    /// All three in `dir`, for tests and tools.
    pub fn under(dir: &Path) -> Self {
        Self {
            backups: dir.join("backups"),
            snapshots: dir.join("snapshots"),
            probes: dir.join("probes"),
        }
    }
}

/// The browser demo's and the tests' disk: files, backups and snapshots
/// kept in memory.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    files: BTreeMap<PathBuf, String>,
    /// Each backup: the file's name, when it was taken, its text.
    backups: Vec<(String, u128, String)>,
    /// Each snapshot: its name and its files by name.
    snapshots: Vec<(String, Vec<(String, String)>)>,
}

/// Where the files are.
#[derive(Debug, Clone)]
pub enum Backend {
    Files(Folders),
    Memory(Memory),
}

#[derive(Debug, Clone)]
pub struct Disk {
    backend: Backend,
}

impl Disk {
    /// The real files, with backups and snapshots in `folders`.
    pub const fn files(folders: Folders) -> Self {
        Self {
            backend: Backend::Files(folders),
        }
    }

    /// Files held in memory, starting with `files`.
    pub fn memory(files: impl IntoIterator<Item = (PathBuf, String)>) -> Self {
        Self {
            backend: Backend::Memory(Memory {
                files: files.into_iter().collect(),
                ..Memory::default()
            }),
        }
    }

    fn read(&self, path: &Path) -> io::Result<String> {
        match &self.backend {
            Backend::Files(_) => fs::read_to_string(path),
            Backend::Memory(memory) => memory
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    /// Read the config at `main` and its includes. The rule library's file
    /// beside it is read too when nothing includes it yet, so adding the
    /// first rule to it keeps what it holds.
    pub fn load(&self, main: &Path) -> Result<Loaded, DiskError> {
        #[allow(clippy::wildcard_enum_match_arm)] // only a read error is the disk's
        let mut store = Store::read(main.to_path_buf(), |path| self.read(path)).map_err(
            |error| match error {
                StoreError::Io { path, source } => DiskError::Read { path, source },
                other => DiskError::Store(other),
            },
        )?;
        let spare = store.config_dir().join(crate::library::FILE);
        if !store.saved_texts().any(|(path, _)| path == spare)
            && let Ok(text) = self.read(&spare)
        {
            store.keep_spare(spare, text);
        }
        let read_only = match &self.backend {
            Backend::Files(_) => read_only(store.saved_texts().map(|(path, _)| path)),
            Backend::Memory(_) => Vec::new(),
        };
        Ok(Loaded { store, read_only })
    }

    /// Whether niri reads the job's texts as a config: the whole set,
    /// mirrored into a probe folder so relative includes resolve the same.
    /// In memory there is no niri to ask.
    pub fn check(&self, job: &SaveJob) -> Result<(), DiskError> {
        match &self.backend {
            Backend::Files(folders) => check(&folders.probes, job),
            Backend::Memory(_) => Ok(()),
        }
    }

    /// Save all of `store`'s edits, asking niri first, and take what was
    /// written as its saved text: one save from start to end, for tests
    /// and tools without an actor.
    pub fn save_store(&mut self, store: &mut Store) -> Result<(), DiskError> {
        let job = store.job();
        self.save(&job, false)?;
        store.saved(&job);
        Ok(())
    }

    /// Whether niri reads the config at `main` as it is on disk: its words
    /// when it does not. In memory there is no niri to ask.
    pub fn check_on_disk(&self, main: &Path) -> Result<(), String> {
        match &self.backend {
            Backend::Files(_) => validate::reads(main),
            Backend::Memory(_) => Ok(()),
        }
    }

    /// Write the job's changed files, after making sure each still holds
    /// the text the job expects. `checked`: niri has accepted the job's
    /// texts already; otherwise it is asked first. Backups are taken, then
    /// every file is written beside its target and renamed into place.
    pub fn save(&mut self, job: &SaveJob, checked: bool) -> Result<(), DiskError> {
        for file in job.changed() {
            let on_disk = match self.read(&file.path) {
                Ok(text) => text,
                // a file this save creates
                Err(e) if e.kind() == io::ErrorKind::NotFound && file.before.is_empty() => {
                    String::new()
                }
                Err(source) => {
                    return Err(DiskError::Read {
                        path: file.path.clone(),
                        source,
                    });
                }
            };
            if on_disk != file.before {
                return Err(DiskError::ChangedOnDisk {
                    path: file.path.clone(),
                });
            }
        }
        if !checked {
            self.check(job)?;
        }
        match &mut self.backend {
            Backend::Files(folders) => write_files(folders, job),
            Backend::Memory(memory) => {
                let now = crate::clock::now_nanos();
                for file in job.changed() {
                    if let Some(name) = short_name(&file.path) {
                        memory
                            .backups
                            .push((name.to_string(), now, file.before.clone()));
                    }
                    memory.files.insert(file.path.clone(), file.after.clone());
                }
                Ok(())
            }
        }
    }

    /// The files whose text on disk is no longer what `saved` says: another
    /// program wrote them. `before` is the saved text, `after` the disk's.
    pub fn changed<'a>(&self, saved: impl Iterator<Item = (&'a Path, &'a str)>) -> Vec<FileChange> {
        saved
            .filter_map(|(path, text)| {
                let on_disk = self.read(path).ok()?;
                (on_disk != text).then(|| FileChange {
                    path: path.to_path_buf(),
                    before: text.to_string(),
                    after: on_disk,
                })
            })
            .collect()
    }

    /// Each backed-up file by name, with when its newest backup was taken
    /// (nanoseconds since the Unix epoch).
    pub fn latest_backups(&self) -> Vec<(String, u128)> {
        let all: Vec<(String, u128)> = match &self.backend {
            Backend::Files(folders) => backup::list_backups(&folders.backups)
                .into_iter()
                .map(|backup| (backup.display_name, backup.timestamp_nanos))
                .collect(),
            Backend::Memory(memory) => memory
                .backups
                .iter()
                .map(|(name, time, _)| (name.clone(), *time))
                .collect(),
        };
        let mut latest: Vec<(String, u128)> = Vec::new();
        for (name, time) in all {
            match latest.iter_mut().find(|(known, _)| *known == name) {
                Some((_, newest)) => *newest = (*newest).max(time),
                None => latest.push((name, time)),
            }
        }
        latest.sort();
        latest
    }

    /// The text of the newest backup of the file named `name`.
    pub fn backup(&self, name: &str) -> Result<String, DiskError> {
        let found = match &self.backend {
            Backend::Files(folders) => backup::restore_latest(&folders.backups, name),
            Backend::Memory(memory) => memory
                .backups
                .iter()
                .filter(|(file, _, _)| file == name)
                .max_by_key(|(_, time, _)| *time)
                .map(|(_, _, text)| text.clone()),
        };
        found.ok_or_else(|| DiskError::NoBackup {
            name: name.to_string(),
        })
    }

    /// The snapshots' names, oldest first.
    pub fn snapshots(&self) -> Vec<String> {
        match &self.backend {
            Backend::Files(folders) => snapshot::list(&folders.snapshots)
                .into_iter()
                .map(|meta| meta.name)
                .collect(),
            Backend::Memory(memory) => memory
                .snapshots
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        }
    }

    /// Keep `files` (each file's name and text) as the snapshot `name`.
    pub fn create_snapshot(
        &mut self,
        name: &str,
        files: Vec<(String, String)>,
    ) -> Result<(), DiskError> {
        match &mut self.backend {
            Backend::Files(folders) => snapshot::create(&folders.snapshots, name, &files, "")
                .map(|_| ())
                .map_err(|source| DiskError::Snapshot {
                    name: name.to_string(),
                    source,
                }),
            Backend::Memory(memory) => {
                memory.snapshots.retain(|(known, _)| known != name);
                memory.snapshots.push((name.to_string(), files));
                Ok(())
            }
        }
    }

    /// The files of the snapshot `name`, each by name with its text.
    pub fn snapshot(&self, name: &str) -> Result<Vec<(String, String)>, DiskError> {
        match &self.backend {
            Backend::Files(folders) => snapshot::load(&folders.snapshots, name)
                .map(|loaded| loaded.files)
                .map_err(|source| DiskError::Snapshot {
                    name: name.to_string(),
                    source,
                }),
            Backend::Memory(memory) => memory
                .snapshots
                .iter()
                .find(|(known, _)| known == name)
                .map(|(_, files)| files.clone())
                .ok_or_else(|| DiskError::Snapshot {
                    name: name.to_string(),
                    source: io::Error::from(io::ErrorKind::NotFound),
                }),
        }
    }

    pub fn delete_snapshot(&mut self, name: &str) -> Result<(), DiskError> {
        match &mut self.backend {
            Backend::Files(folders) => {
                snapshot::delete(&folders.snapshots, name).map_err(|source| DiskError::Snapshot {
                    name: name.to_string(),
                    source,
                })
            }
            Backend::Memory(memory) => {
                memory.snapshots.retain(|(known, _)| known != name);
                Ok(())
            }
        }
    }
}

/// Where a save of the config at `main` writes: its folder and, when it is
/// a symlink, the folder it resolves to (saves write through the link).
/// These are the sandbox's config write roots.
pub fn config_dirs(main: &Path) -> Vec<PathBuf> {
    let folder = main
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut dirs = vec![folder];
    let resolved = fs::canonicalize(main)
        .ok()
        .and_then(|resolved| resolved.parent().map(Path::to_path_buf));
    if let Some(resolved) = resolved
        && !dirs.contains(&resolved)
    {
        dirs.push(resolved);
    }
    dirs
}

/// The files of `paths` this app cannot write: a read-only file or folder,
/// a link into a read-only store (NixOS, home-manager), or a folder outside
/// the sandbox's reach. Found by trying, as a save would, to create a file
/// beside each; nothing is left behind.
fn read_only<'a>(paths: impl Iterator<Item = &'a Path>) -> Vec<PathBuf> {
    paths
        .filter(|path| {
            let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let writable = fs::OpenOptions::new().append(true).open(&resolved).is_ok()
                && resolved.parent().is_some_and(|dir| {
                    tempfile::Builder::new()
                        .prefix(".niri-center-probe")
                        .tempfile_in(dir)
                        .is_ok()
                });
            !writable
        })
        .map(Path::to_path_buf)
        .collect()
}

/// Mirror the job's texts into a fresh probe folder, keeping the layout so
/// relative includes resolve the same, and run `niri validate` there. The
/// probe folder is a fresh one in `probes`, removed after: it works inside
/// the sandbox, and two runs never share one.
fn check(probes: &Path, job: &SaveJob) -> Result<(), DiskError> {
    let dir = fs::create_dir_all(probes)
        .and_then(|()| tempfile::Builder::new().prefix("run-").tempdir_in(probes))
        .map_err(|source| DiskError::Write {
            path: probes.to_path_buf(),
            source,
        })?;
    mirror(dir.path(), job).and_then(|main| {
        let report = validate::validate(&main).map_err(|e| DiskError::Validator(e.to_string()))?;
        if report.ok {
            Ok(())
        } else {
            Err(DiskError::Rejected {
                diagnostics: report.diagnostics(),
            })
        }
    })
}

/// Write the job's texts under `root`, each at its own path below it: the
/// main config's mirrored path.
fn mirror(root: &Path, job: &SaveJob) -> Result<PathBuf, DiskError> {
    let place = |real: &Path| root.join(real.strip_prefix("/").unwrap_or(real));
    for file in &job.files {
        let copy = place(&file.path);
        copy.parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&copy, &file.after))
            .map_err(|source| DiskError::Write {
                path: file.path.clone(),
                source,
            })?;
    }
    Ok(place(&job.main))
}

/// Write the job's changed files: each staged beside its target first (a
/// folder that cannot be written fails here, before any config changes),
/// then backed up, then renamed into place. Writes go through symlinks.
fn write_files(folders: &Folders, job: &SaveJob) -> Result<(), DiskError> {
    let mut staged = Vec::new();
    for file in job.changed() {
        match Staged::new(&file.path, &file.after) {
            Ok(write) => staged.push((&file.path, write)),
            Err(source) => {
                staged.into_iter().for_each(|(_, write)| write.abandon());
                return Err(DiskError::Write {
                    path: file.path.clone(),
                    source,
                });
            }
        }
    }
    for file in job.changed() {
        // a new file has nothing to back up
        if file.before.is_empty() && !file.path.exists() {
            continue;
        }
        let name = short_name(&file.path).unwrap_or("config.kdl");
        if let Err(source) = backup::write_backup(&folders.backups, name, &file.before) {
            staged.into_iter().for_each(|(_, write)| write.abandon());
            return Err(DiskError::Write {
                path: folders.backups.clone(),
                source,
            });
        }
    }
    let mut staged = staged.into_iter();
    while let Some((real, write)) = staged.next() {
        if let Err(source) = write.commit() {
            staged.for_each(|(_, write)| write.abandon());
            return Err(DiskError::Write {
                path: real.clone(),
                source,
            });
        }
    }
    let _ = backup::prune(&folders.backups, KEEP_BACKUPS);
    Ok(())
}

/// New text written to a temp file beside the (symlink-resolved) target,
/// waiting to be renamed over it: an atomic write that keeps permissions
/// and writes through symlinks instead of replacing them.
struct Staged {
    temp_path: PathBuf,
    resolved: PathBuf,
}

impl Staged {
    /// Fails without touching `target` when its folder cannot be written.
    fn new(target: &Path, content: &str) -> io::Result<Self> {
        let resolved = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
        let dir = resolved
            .parent()
            .ok_or_else(|| io::Error::other("no parent directory"))?;
        let file_name = resolved
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("config.kdl");
        let temp_path = dir.join(format!(".{file_name}.temp_path"));
        fs::write(&temp_path, content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&resolved) {
                let perms = fs::Permissions::from_mode(meta.permissions().mode());
                let _ = fs::set_permissions(&temp_path, perms);
            }
        }
        Ok(Self {
            temp_path,
            resolved,
        })
    }

    fn commit(self) -> io::Result<()> {
        fs::rename(&self.temp_path, &self.resolved)
    }

    /// Remove the temp file; the target was never touched.
    fn abandon(self) {
        let _ = fs::remove_file(&self.temp_path);
    }
}

/// A file's name alone: `binds.kdl`.
fn short_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}
