//! What happened to the config files, oldest first: every save the app
//! made and every edit made in another program that the app noticed.
//!
//! Each entry keeps the full text of every file it touched, before and
//! after, so undoing it writes back exactly what was there: no reformatting,
//! like every other write.
//!
//! On disk an entry is a directory named by its time in milliseconds:
//! `entry` (what made it, then one summary line each), `files` (one path a
//! line) and `<n>.before` / `<n>.after` for the n-th file. Loading reads
//! the first two; the texts are read when something asks for them, so a
//! long history costs little until a change in it is opened or undone.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many entries to keep (oldest go first).
pub const KEEP: usize = 200;

/// What made a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A save from this app.
    App,
    /// Another program wrote the file; the app noticed afterwards.
    Outside,
    /// A save from this app that put back an earlier version; its first
    /// summary line says which.
    Restored,
}

impl Source {
    const fn word(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Outside => "outside",
            Self::Restored => "restored",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        match word {
            "app" => Some(Self::App),
            "outside" => Some(Self::Outside),
            "restored" => Some(Self::Restored),
            _ => None,
        }
    }
}

/// One file's text before and after a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// One change to the config set: what it did, in words, and the files it
/// touched. Their texts are kept apart ([`read_texts`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Milliseconds since the Unix epoch; also the entry's name on disk.
    pub time: u128,
    pub source: Source,
    /// What changed, one line each, in the app's words.
    pub summary: Vec<String>,
    /// The files it touched, in order.
    pub paths: Vec<PathBuf>,
}

impl Entry {
    pub fn new(time: u128, source: Source, summary: Vec<String>, files: &[FileChange]) -> Self {
        Self {
            time,
            source,
            summary,
            paths: files.iter().map(|file| file.path.clone()).collect(),
        }
    }
}

/// Milliseconds since the Unix epoch.
pub fn now() -> u128 {
    crate::clock::now_millis()
}

/// Write `entry` with the texts of its `files` into `dir`. A half-written
/// entry never shows: it is put together under a hidden name and renamed
/// into place.
pub fn record(dir: &Path, entry: &Entry, files: &[FileChange]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    // two entries in the same millisecond: the later one moves up
    let mut time = entry.time;
    while dir.join(time.to_string()).exists() {
        time += 1;
    }
    let partial = dir.join(format!(".{time}"));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir(&partial)?;
    let written = write_entry(&partial, entry, files)
        .and_then(|()| fs::rename(&partial, dir.join(time.to_string())));
    if written.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    written
}

fn write_entry(dir: &Path, entry: &Entry, files: &[FileChange]) -> io::Result<()> {
    let mut head = entry.source.word().to_string();
    for line in &entry.summary {
        head.push('\n');
        head.push_str(&line.replace('\n', " "));
    }
    fs::write(dir.join("entry"), head)?;
    let paths: Vec<String> = entry
        .paths
        .iter()
        .map(|path| path.to_string_lossy().replace('\n', " "))
        .collect();
    fs::write(dir.join("files"), paths.join("\n"))?;
    for (index, file) in files.iter().enumerate() {
        fs::write(dir.join(format!("{index}.before")), &file.before)?;
        fs::write(dir.join(format!("{index}.after")), &file.after)?;
    }
    Ok(())
}

/// Every entry in `dir`, oldest first. Unreadable entries are left out.
pub fn load(dir: &Path) -> Vec<Entry> {
    let Ok(listing) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = listing
        .flatten()
        .filter_map(|item| {
            let time = item.file_name().to_str()?.parse::<u128>().ok()?;
            read_entry(&item.path(), time)
        })
        .collect();
    entries.sort_by_key(|entry| entry.time);
    entries
}

/// An entry's head: what made it, its summary and the files it touched.
/// An entry missing a text is left out, as one half written.
fn read_entry(dir: &Path, time: u128) -> Option<Entry> {
    let head = fs::read_to_string(dir.join("entry")).ok()?;
    let mut lines = head.lines();
    let source = Source::from_word(lines.next()?)?;
    let summary = lines.map(str::to_string).collect();
    let paths: Vec<PathBuf> = fs::read_to_string(dir.join("files"))
        .ok()?
        .lines()
        .map(PathBuf::from)
        .collect();
    let complete = (0..paths.len()).all(|index| {
        dir.join(format!("{index}.before")).is_file()
            && dir.join(format!("{index}.after")).is_file()
    });
    complete.then_some(Entry {
        time,
        source,
        summary,
        paths,
    })
}

/// The texts of `entry`'s files, read from the history in `dir`.
pub fn read_texts(dir: &Path, entry: &Entry) -> Option<Vec<FileChange>> {
    let dir = dir.join(entry.time.to_string());
    entry
        .paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            Some(FileChange {
                path: path.clone(),
                before: fs::read_to_string(dir.join(format!("{index}.before"))).ok()?,
                after: fs::read_to_string(dir.join(format!("{index}.after"))).ok()?,
            })
        })
        .collect()
}

/// Keep only the newest `keep` entries. Entries are known by their
/// directory names, so nothing is read.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    let mut times: Vec<u128> = listing
        .flatten()
        .filter_map(|item| item.file_name().to_str()?.parse().ok())
        .collect();
    times.sort_unstable();
    let excess = times.len().saturating_sub(keep);
    for time in times.iter().take(excess) {
        let _ = fs::remove_dir_all(dir.join(time.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change() -> Vec<FileChange> {
        vec![FileChange {
            path: PathBuf::from("/home/me/.config/niri/config.kdl"),
            before: "layout {\n    gaps 16\n}\n".into(),
            after: "layout {\n    gaps 24\n}\n".into(),
        }]
    }

    fn entry(time: u128, source: Source) -> Entry {
        Entry::new(
            time,
            source,
            vec!["Gaps 16 → 24".into(), "Focus ring width 4 → 2".into()],
            &change(),
        )
    }

    #[test]
    fn entries_read_back_exactly_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let newer = entry(2000, Source::Outside);
        let older = entry(1000, Source::App);
        record(dir.path(), &newer, &change()).unwrap();
        record(dir.path(), &older, &change()).unwrap();
        assert_eq!(load(dir.path()), vec![older.clone(), newer]);
        assert_eq!(read_texts(dir.path(), &older), Some(change()));
    }

    #[test]
    fn same_millisecond_entries_both_survive() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), &entry(5, Source::App), &change()).unwrap();
        record(dir.path(), &entry(5, Source::App), &change()).unwrap();
        let times: Vec<u128> = load(dir.path()).iter().map(|e| e.time).collect();
        assert_eq!(times, vec![5, 6]);
    }

    #[test]
    fn prune_drops_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        for time in 1..=5 {
            record(dir.path(), &entry(time, Source::App), &change()).unwrap();
        }
        prune(dir.path(), 2);
        let times: Vec<u128> = load(dir.path()).iter().map(|e| e.time).collect();
        assert_eq!(times, vec![4, 5]);
    }

    #[test]
    fn half_written_and_foreign_entries_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), &entry(1, Source::App), &change()).unwrap();
        fs::create_dir(dir.path().join(".7")).unwrap();
        fs::create_dir(dir.path().join("8")).unwrap();
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert_eq!(load(dir.path()).len(), 1);
    }
}
