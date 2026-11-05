//! Named, restorable config snapshots (beyond the rolling auto-backups).
//!
//! A snapshot captures the current state of every config file under a
//! user-chosen name, stored in `$XDG_DATA_HOME/niri-center/snapshots/<name>/`.
//! Unlike the timestamped backups written on every save, snapshots are
//! explicit checkpoints the user creates, lists, restores, and deletes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Metadata for a listed snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub name: String,
    pub created_nanos: u128,
    pub note: String,
}

/// One config file a snapshot keeps.
#[derive(Debug, Clone)]
pub struct SnapshotFile {
    /// The config file's display name.
    pub(crate) name: String,
    pub(crate) text: String,
}

/// Restored snapshot contents, by config-file display name.
#[derive(Debug, Default)]
pub struct SnapshotFiles {
    pub files: Vec<SnapshotFile>,
    pub note: String,
}

/// Snapshot directory name for a user-supplied name.
fn slug(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, '-' | '_' | '.'))
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        None
    } else {
        Some(cleaned)
    }
}

fn invalid(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid snapshot name: {name:?}"),
    )
}

/// Store `files` under snapshot `name`.
pub fn create(dir: &Path, name: &str, files: &[SnapshotFile], note: &str) -> io::Result<PathBuf> {
    let slug = slug(name).ok_or_else(|| invalid(name))?;
    let target = dir.join(&slug);
    fs::create_dir_all(&target)?;
    for file in files {
        fs::write(target.join(file.name.replace('/', "_")), &file.text)?;
    }
    let nanos = crate::clock::now_nanos();
    fs::write(target.join(".meta"), format!("{nanos}\n{name}\n{note}"))?;
    Ok(target)
}

/// List snapshots, oldest first.
pub fn list(dir: &Path) -> Vec<SnapshotMeta> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let (display, created_nanos, note) = fs::read_to_string(path.join(".meta")).map_or_else(
            |_| (name.clone(), 0, String::new()),
            |text| {
                let mut lines = text.lines();
                let created = lines
                    .next()
                    .and_then(|text| text.parse::<u128>().ok())
                    .unwrap_or(0);
                let stored_name = lines.next().unwrap_or(&name);
                let display = if stored_name.is_empty() {
                    &name
                } else {
                    stored_name
                };
                let note = lines.collect::<Vec<_>>().join("\n");
                (display.to_string(), created, note)
            },
        );
        out.push(SnapshotMeta {
            name: display,
            created_nanos,
            note,
        });
    }
    out.sort_by_key(|snapshot| snapshot.created_nanos);
    out
}

/// Load a snapshot's files and note.
pub fn load(dir: &Path, name: &str) -> io::Result<SnapshotFiles> {
    let root = dir.join(slug(name).ok_or_else(|| invalid(name))?);
    let mut files = Vec::new();
    for entry in fs::read_dir(&root)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some(".meta") {
            continue;
        }
        let display = entry.file_name().to_string_lossy().into_owned();
        files.push(SnapshotFile {
            name: display,
            text: fs::read_to_string(&path)?,
        });
    }
    let note = fs::read_to_string(root.join(".meta")).map_or_else(
        |_| String::new(),
        |text| text.lines().skip(2).collect::<Vec<_>>().join("\n"),
    );
    Ok(SnapshotFiles { files, note })
}

/// Remove a snapshot directory.
pub fn delete(dir: &Path, name: &str) -> io::Result<()> {
    fs::remove_dir_all(dir.join(slug(name).ok_or_else(|| invalid(name))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, text: &str) -> SnapshotFile {
        SnapshotFile {
            name: name.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn create_list_load_delete_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap().path().join("snapshots");
        fs::create_dir_all(&dir).unwrap();

        create(
            &dir,
            "my setup",
            &[file("config.kdl", "a"), file("keybinds.kdl", "b")],
            "first snapshot",
        )
        .unwrap();

        let metas = list(&dir);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].name, "my setup");
        assert_eq!(metas[0].note, "first snapshot");

        let loaded = load(&dir, "my setup").unwrap();
        assert_eq!(loaded.files.len(), 2);
        assert!(
            loaded
                .files
                .iter()
                .any(|file| file.name == "config.kdl" && file.text == "a")
        );
        assert_eq!(loaded.note, "first snapshot");

        delete(&dir, "my setup").unwrap();
        assert!(list(&dir).is_empty());
    }

    /// A folder by the slug, each file under its own name, and `.meta` with
    /// the time, the name and the note: what snapshots already on people's
    /// disks hold, so a change here strands them.
    #[test]
    fn a_snapshot_written_earlier_still_loads() {
        let holder = tempfile::TempDir::new().unwrap();
        let folder = holder.path().join("mysetup");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("config.kdl"), "layout {\n}\n").unwrap();
        fs::write(folder.join("keybinds.kdl"), "binds {\n}\n").unwrap();
        fs::write(folder.join(".meta"), "17\nmy setup\nfirst line\nsecond").unwrap();

        let metas = list(holder.path());
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].name, "my setup");
        assert_eq!(metas[0].created_nanos, 17);

        let loaded = load(holder.path(), "my setup").unwrap();
        assert_eq!(loaded.note, "first line\nsecond");
        let mut files: Vec<(String, String)> = loaded
            .files
            .into_iter()
            .map(|file| (file.name, file.text))
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                ("config.kdl".to_string(), "layout {\n}\n".to_string()),
                ("keybinds.kdl".to_string(), "binds {\n}\n".to_string()),
            ]
        );
    }

    #[test]
    fn a_snapshot_on_disk_is_its_files_and_a_meta_file() {
        let holder = tempfile::TempDir::new().unwrap();
        let folder = create(
            holder.path(),
            "my setup",
            &[file("config.kdl", "a"), file("nested/binds.kdl", "b")],
            "the note",
        )
        .unwrap();
        assert_eq!(folder, holder.path().join("mysetup"));

        let mut names: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, [".meta", "config.kdl", "nested_binds.kdl"]);
        assert_eq!(fs::read(folder.join("config.kdl")).unwrap(), b"a");
        assert_eq!(fs::read(folder.join("nested_binds.kdl")).unwrap(), b"b");

        let meta = fs::read_to_string(folder.join(".meta")).unwrap();
        let (time, rest) = meta.split_once('\n').unwrap();
        assert!(time.parse::<u128>().is_ok(), "{meta}");
        assert_eq!(rest, "my setup\nthe note");
    }

    #[test]
    fn rejects_path_traversal_names() {
        let dir = tempfile::TempDir::new().unwrap().path().join("snapshots");
        fs::create_dir_all(&dir).unwrap();
        // `../escape` sanitizes to `escape`; absolute-path chars are dropped
        let ok = slug("../escape").unwrap();
        assert_ne!(ok, "../escape");
        assert!(slug("").is_none());
        assert!(slug("..").is_none());
    }
}
