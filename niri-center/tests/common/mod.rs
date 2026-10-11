//! What the store's tests share: reading configs from disk, and a disk
//! whose backups, snapshots and probes stay in the test's folder.
#![allow(dead_code)] // each test file uses some of these

use std::path::{Path, PathBuf};

use niri_center::disk::{Disk, DiskError, Folders};
use niri_center::store::Store;

/// A disk that only reads: it writes nothing, so its folders need not exist.
fn reader() -> Disk {
    Disk::files(Folders::under(Path::new("/nonexistent")))
}

/// The config at `path`, read from disk.
pub fn load(path: impl AsRef<Path>) -> Store {
    reader().load(path.as_ref()).unwrap().store
}

/// The files of the config at `path` this app cannot write.
pub fn read_only(path: impl AsRef<Path>) -> Vec<PathBuf> {
    reader().load(path.as_ref()).unwrap().read_only
}

/// The real files, with backups, snapshots and probes in folders of
/// those names in `dir`.
pub fn folders_in(dir: &Path) -> Disk {
    Disk::files(Folders::under(dir))
}

pub fn save(disk: &mut Disk, store: &mut Store) -> Result<(), DiskError> {
    disk.save_store(store)
}

/// Whether niri reads the store's texts as a config.
pub fn check(store: &Store) -> Result<(), DiskError> {
    let dir = tempfile::tempdir().unwrap();
    Disk::files(Folders::under(dir.path())).check(&store.job())
}

/// The newest backup of `name`, staged as that file's text.
pub fn stage_backup(disk: &Disk, store: &mut Store, name: &str) -> Result<(), String> {
    let text = disk.backup(name).map_err(|e| e.to_string())?;
    store.stage_named(name, &text).map_err(|e| e.to_string())
}

/// The snapshot `name`, staged over the files it matches.
pub fn stage_snapshot(disk: &Disk, store: &mut Store, name: &str) -> Result<(), String> {
    let files = disk.snapshot(name).map_err(|e| e.to_string())?;
    store.stage_snapshot(&files).map_err(|e| e.to_string())
}
